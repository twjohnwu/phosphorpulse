use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
const SAMPLE: &str = "tests/golden/main-default";

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-s08-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("read fixture entry type").is_dir() {
            copy_tree(&entry.path(), &destination_path);
        } else {
            fs::copy(entry.path(), destination_path).expect("copy fixture file");
        }
    }
}

fn release_binary() -> PathBuf {
    let debug = env!("CARGO_BIN_EXE_phosphorpulse");
    let release = debug.replace("/debug/", "/release/");
    assert_ne!(debug, release, "could not derive release binary from debug path: {debug}");
    let release = PathBuf::from(release);
    assert!(
        release.is_file(),
        "release binary missing: {} (build it separately with cargo build --release)",
        release.display()
    );
    let modified = fs::metadata(&release)
        .expect("read release binary metadata")
        .modified()
        .expect("read release binary mtime");
    assert!(
        modified.elapsed().map_or(true, |age| age <= Duration::from_secs(60 * 60)),
        "release binary is older than 1 hour: {}",
        release.display()
    );
    release
}

fn render(
    binary: &Path,
    cwd: &Path,
    config: &Path,
    home: &Path,
    path: &Path,
    stdin: &[u8],
) -> Duration {
    let started = Instant::now();
    let mut child = Command::new(binary)
        .arg("render")
        .env_clear()
        .current_dir(cwd)
        .env("PATH", path)
        .env("HOME", home)
        .env("PPULSE_CONFIG_DIR", config)
        .env("PPULSE_NOW_MS", "1755150000000")
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn phosphorpulse render");
    child
        .stdin
        .take()
        .expect("open render stdin")
        .write_all(stdin)
        .expect("write render stdin");
    let output = child.wait_with_output().expect("wait for phosphorpulse render");
    assert!(
        output.status.success(),
        "render failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim_end()
    );
    started.elapsed()
}

fn measure<F>(mut run: F) -> (Duration, Duration, Duration)
where
    F: FnMut() -> Duration,
{
    let _ = run(); // Discard run 1.
    let mut timings = (0..20).map(|_| run()).collect::<Vec<_>>();
    timings.sort_unstable();
    (timings[0], timings[9], timings[19])
}

fn print_stats(label: &str, (min, median, max): (Duration, Duration, Duration)) {
    println!(
        "{label}: min={:.3}ms median={:.3}ms max={:.3}ms",
        min.as_secs_f64() * 1_000.0,
        median.as_secs_f64() * 1_000.0,
        max.as_secs_f64() * 1_000.0,
    );
}

// REQ-07 / S-08: gate the warmed hermetic median; report the cold real-PATH median.
#[test]
fn test_s08_render_median_budget() {
    let binary = release_binary();
    let fixture = Path::new(SAMPLE);
    let stdin = fs::read(fixture.join("stdin.json")).expect("read sample stdin");

    let hermetic = TempDir::new("hermetic");
    let hermetic_config = hermetic.path().join("config");
    let hermetic_home = hermetic.path().join("home");
    fs::create_dir_all(&hermetic_home).expect("create hermetic HOME");
    fs::create_dir_all(&hermetic_config).expect("create hermetic config");
    fs::copy(fixture.join("settings.json"), hermetic_config.join("settings.json"))
        .expect("copy hermetic settings");
    copy_tree(&fixture.join("state"), &hermetic_config);
    let stub_bin = fs::canonicalize(fixture.join("bin")).expect("resolve fixture PATH stubs");
    for entry in fs::read_dir(&stub_bin).expect("read fixture PATH stubs") {
        let stub = entry.expect("read fixture PATH stub").path();
        let status = Command::new(&stub)
            .env_clear()
            .env("PATH", &stub_bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("warm fixture PATH stub");
        assert!(status.success(), "fixture PATH stub failed: {}", stub.display());
    }
    let _ = render(
        &binary,
        &fixture.join("cwd"),
        &hermetic_config,
        &hermetic_home,
        &stub_bin,
        &stdin,
    ); // Untimed warm-up: cache fixture is now hot.
    let warm = measure(|| {
        render(
            &binary,
            &fixture.join("cwd"),
            &hermetic_config,
            &hermetic_home,
            &stub_bin,
            &stdin,
        )
    });
    print_stats("hermetic-warm", warm);
    assert!(
        warm.1 <= Duration::from_millis(30),
        "hermetic-warm median {:.3}ms exceeds 30ms budget",
        warm.1.as_secs_f64() * 1_000.0
    );

    let cold = TempDir::new("cold");
    let cold_config = cold.path().join("config");
    let cold_home = cold.path().join("home");
    fs::create_dir_all(&cold_home).expect("create cold HOME");
    fs::create_dir_all(&cold_config).expect("create cold config");
    fs::copy(fixture.join("settings.json"), cold_config.join("settings.json"))
        .expect("copy cold settings");
    let repo = std::env::current_dir().expect("resolve repository cwd");
    let mut cold_stdin: serde_json::Value = serde_json::from_slice(&stdin).expect("parse sample stdin");
    cold_stdin["cwd"] = serde_json::Value::String(repo.to_string_lossy().into_owned());
    let cold_stdin = serde_json::to_vec(&cold_stdin).expect("serialize cold stdin");
    let real_path = std::env::var_os("PATH").expect("real system PATH is set");
    let real_path = PathBuf::from(real_path);
    let cold_stats = measure(|| {
        render(
            &binary,
            &repo,
            &cold_config,
            &cold_home,
            &real_path,
            &cold_stdin,
        )
    });
    print_stats("cold-real-path (report-only)", cold_stats);
}
