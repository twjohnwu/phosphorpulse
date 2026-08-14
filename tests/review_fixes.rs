use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-review-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        Self(path)
    }
    fn path(&self) -> &Path { &self.0 }
}

impl Drop for TempDir {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn stdin() -> Vec<u8> {
    fs::read("tests/golden/main-default/stdin.json").expect("read render stdin")
}

fn render(config_dir: &Path, input: &[u8], extra: impl FnOnce(&mut Command)) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    child.arg("render").env("PPULSE_CONFIG_DIR", config_dir)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    extra(&mut child);
    let mut child = child.spawn().expect("start render");
    child.stdin.take().expect("render stdin").write_all(input).expect("write render stdin");
    child.wait_with_output().expect("wait for render")
}

fn write_settings(config_dir: &Path, value: serde_json::Value) {
    fs::create_dir_all(config_dir).expect("create config directory");
    fs::write(config_dir.join("settings.json"), serde_json::to_vec(&value).expect("serialize settings"))
        .expect("write settings");
}

#[test]
fn test_gauge_bounds_fail_loud() {
    for bar_width in [serde_json::json!(1e300), serde_json::json!(-1)] {
        let temp = TempDir::new("gauge-bounds");
        let config_dir = temp.path().join("config");
        write_settings(&config_dir, serde_json::json!({
            "rows": [{"segments": ["ctx"]}],
            "gauge": {"barWidth": bar_width}
        }));
        let output = render(&config_dir, &stdin(), |_| {});
        assert!(output.status.success(), "invalid gauge config must exit 0: {:?}", output);
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
        assert!(stdout.contains("warning"), "invalid gauge config must warn: {stdout:?}");
        assert_eq!(stdout.lines().count(), 1, "invalid gauge config must only warn: {stdout:?}");
    }
}

#[test]
fn test_row_color_override() {
    let temp = TempDir::new("row-color");
    let config_dir = temp.path().join("config");
    write_settings(&config_dir, serde_json::json!({
        "colorDepth": "truecolor",
        "segments": {"model": {"fg": "#0000FF"}},
        "rows": [{"color": {"fg": "#FF0000"}, "segments": ["model", "effort"]}]
    }));
    let output = render(&config_dir, &stdin(), |_| {});
    assert!(output.status.success(), "fg render failed: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("\x1b[38;2;255;0;0m"),
        "row fg must color segments without segment overrides");
    assert!(stdout.contains("\x1b[38;2;0;0;255m"),
        "segment fg must override row fg");

    write_settings(&config_dir, serde_json::json!({
        "colorDepth": "truecolor",
        "segments": {"model": {"bg": "#0000FF"}},
        "rows": [{"color": {"bg": "#00FF00"}, "segments": ["model", "effort"]}]
    }));
    let output = render(&config_dir, &stdin(), |_| {});
    assert!(output.status.success(), "bg render failed: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("\x1b[48;2;0;255;0m"),
        "row bg must color segments without segment overrides");
    assert!(stdout.contains("\x1b[48;2;0;0;255m"),
        "segment bg must override row bg");
}

fn write_executable(path: &Path, name: &str) {
    fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$PPULSE_LOOKUP_COUNTER\"\nprintf 'stub\\n'\n"))
        .expect("write stub");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("stub metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make stub executable");
    }
}

#[test]
fn test_no_forks_without_external_segments() {
    let temp = TempDir::new("pure-segments");
    let config_dir = temp.path().join("config");
    let stub_bin = temp.path().join("bin");
    let counter = temp.path().join("counter");
    fs::create_dir_all(&stub_bin).expect("create stub bin");
    write_settings(&config_dir, serde_json::json!({
        "rows": [{"segments": ["model", "dir", "ctx"]}]
    }));
    for name in ["git", "node", "python3"] { write_executable(&stub_bin.join(name), name); }
    let warm_counter = temp.path().join("warm-counter");
    for name in ["git", "node", "python3"] {
        assert!(Command::new(stub_bin.join(name)).env("PPULSE_LOOKUP_COUNTER", &warm_counter)
            .stdout(Stdio::null()).status().expect("warm stub").success());
    }
    let path = format!("{}:{}", stub_bin.display(), std::env::var("PATH").expect("PATH set"));
    let output = render(&config_dir, &stdin(), |command| {
        command.env("PATH", path).env("PPULSE_LOOKUP_COUNTER", &counter);
    });
    assert!(output.status.success(), "pure-segment render failed: {:?}", output.stderr);
    assert!(!counter.exists() || fs::read_to_string(&counter).expect("read counter").is_empty(),
        "pure segments must not fork external lookups");
    assert!(!config_dir.join("lookups/cache.json").exists(), "pure segments must not create cache");
}

#[test]
fn test_invalid_color_fail_safe() {
    for value in ["red", "#f"] {
        let temp = TempDir::new("invalid-color");
        let config_dir = temp.path().join("config");
        write_settings(&config_dir, serde_json::json!({
            "segments": {"model": {"fg": value}},
            "rows": [{"segments": ["model"]}]
        }));

        let output = render(&config_dir, &stdin(), |command| {
            command.env("COLORTERM", "truecolor");
        });
        assert!(output.status.success(), "invalid color config must exit 0: {output:?}");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");

        // Frozen TS: `red` fails liteValidate at /segments/model/fg and emits
        // one fail-loud config-invalid warning; it never renders an ANSI color.
        // Frozen TS: `#f` takes the same schema-invalid warning branch.
        assert!(stdout.contains("warning") && stdout.contains("config invalid"),
            "invalid color must fail loud: {stdout:?}");
        assert_eq!(stdout.lines().count(), 1, "invalid color must only warn: {stdout:?}");
        assert!(!stdout.contains("\x1b["), "invalid color must not render ANSI: {stdout:?}");
    }
}

#[test]
fn test_row_color_invalid_fail_safe() {
    for value in ["red", "#f"] {
        let temp = TempDir::new("invalid-row-color");
        let config_dir = temp.path().join("config");
        write_settings(&config_dir, serde_json::json!({
            "rows": [{"color": {"fg": value}, "segments": ["model"]}]
        }));

        let output = render(&config_dir, &stdin(), |command| {
            command.env("COLORTERM", "truecolor");
        });
        assert!(output.status.success(), "invalid row color config must exit 0: {output:?}");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");

        // Match segment-level overrides: frozen TS rejects both values during
        // validation and emits one fail-loud config-invalid warning.
        assert!(stdout.contains("warning") && stdout.contains("config invalid"),
            "invalid row color must fail loud: {stdout:?}");
        assert_eq!(stdout.lines().count(), 1, "invalid row color must only warn: {stdout:?}");
        assert!(!stdout.contains("\x1b["), "invalid row color must not render ANSI: {stdout:?}");
    }
}

#[test]
fn test_timeout_kill_is_cross_platform() {
    let implementation = fs::read_to_string("src/segments/external.rs")
        .expect("read external segment implementation");
    assert!(implementation.contains("child.kill()"),
        "timeout must use std::process::Child::kill");
    assert!(!implementation.contains("/bin/kill"),
        "timeout must not shell out to a platform-specific killer");
}

fn input_with_cwd(cwd: &Path) -> Vec<u8> {
    let mut input: serde_json::Value = serde_json::from_slice(&stdin()).expect("parse render stdin");
    input["cwd"] = serde_json::json!(cwd);
    serde_json::to_vec(&input).expect("serialize render stdin")
}

fn write_lookup_stub(path: &Path, body: &str) {
    fs::write(
        path,
        format!("#!/bin/sh\nprintf '%s\\n' '{}' >> \"$PPULSE_LOOKUP_COUNTER\"\n{body}\n", path.file_name().expect("stub name").to_string_lossy()),
    )
    .expect("write lookup stub");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("stub metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make stub executable");
    }
}

#[test]
fn test_fallback_row_segments_resolve_lookups() {
    let temp = TempDir::new("fallback-lookups");
    let config_dir = temp.path().join("config");
    let stub_bin = temp.path().join("bin");
    let counter = temp.path().join("counter");
    fs::create_dir_all(&stub_bin).expect("create stub bin");
    write_settings(&config_dir, serde_json::json!({
        "colorDepth": "truecolor",
        "rows": [{"layout": "auto", "color": {"fg": "#00FF00"}}, {"layout": "auto"}]
    }));
    write_lookup_stub(&stub_bin.join("git"), "printf '## fallback-branch...origin/fallback-branch\\n'");
    write_lookup_stub(&stub_bin.join("node"), "printf 'v99.1.0\\n'");
    write_lookup_stub(&stub_bin.join("python3"), "printf 'Python 88.2.0\\n'");
    let warm_counter = temp.path().join("warm-counter");
    for name in ["git", "node", "python3"] {
        assert!(Command::new(stub_bin.join(name)).env("PPULSE_LOOKUP_COUNTER", &warm_counter)
            .stdout(Stdio::null()).status().expect("warm stub").success());
    }
    let path = format!("{}:{}", stub_bin.display(), std::env::var("PATH").expect("PATH set"));
    let output = render(&config_dir, &input_with_cwd(temp.path()), |command| {
        command.env("PATH", path).env("PPULSE_LOOKUP_COUNTER", &counter);
    });
    assert!(output.status.success(), "fallback render failed: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    let invoked = fs::read_to_string(&counter).unwrap_or_default();
    assert!(invoked.contains("git") && invoked.contains("node"),
        "fallback template segments must invoke their lookups: {invoked:?}");
    assert!(stdout.contains("fallback-branch"), "git fallback segment missing: {stdout:?}");
    assert!(!stdout.contains("git:—"), "git must not be unavailable: {stdout:?}");
}

#[test]
fn test_large_git_output_no_false_timeout() {
    let temp = TempDir::new("large-git-output");
    let config_dir = temp.path().join("config");
    let stub_bin = temp.path().join("bin");
    let counter = temp.path().join("counter");
    fs::create_dir_all(&stub_bin).expect("create stub bin");
    write_settings(&config_dir, serde_json::json!({"rows": [{"segments": ["git"]}]}));
    write_lookup_stub(&stub_bin.join("git"), "printf '## main...origin/main\\n'; yes ' M file' | head -c 262144");
    let warm_counter = temp.path().join("warm-counter");
    assert!(Command::new(stub_bin.join("git")).env("PPULSE_LOOKUP_COUNTER", &warm_counter)
        .stdout(Stdio::null()).status().expect("warm stub").success());
    let path = format!("{}:{}", stub_bin.display(), std::env::var("PATH").expect("PATH set"));
    let output = render(&config_dir, &input_with_cwd(temp.path()), |command| {
        command.env("PATH", path).env("PPULSE_LOOKUP_COUNTER", &counter);
    });
    assert!(output.status.success(), "large-output render failed: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(fs::read_to_string(&counter).unwrap_or_default().contains("git"), "git stub was not invoked");
    assert!(stdout.contains("main*"), "large git output must retain branch and dirty marker: {stdout:?}");
    assert!(!stdout.contains("git:—"), "large git output must not time out: {stdout:?}");
}

#[test]
fn test_wrapper_grandchild_timeout_returns_fast() {
    use std::time::{Duration, Instant};

    let temp = TempDir::new("wrapper-grandchild-timeout");
    let config_dir = temp.path().join("config");
    let stub_bin = temp.path().join("bin");
    fs::create_dir_all(&stub_bin).expect("create stub bin");
    write_settings(&config_dir, serde_json::json!({"rows": [{"segments": ["git"]}]}));
    write_lookup_stub(&stub_bin.join("git"), "( sleep 10 ) &\nsleep 10");

    let warm_counter = temp.path().join("warm-counter");
    assert!(Command::new(stub_bin.join("git")).env("PPULSE_LOOKUP_COUNTER", &warm_counter)
        .stdout(Stdio::null()).status().expect("warm stub").success());

    let path = format!("{}:{}", stub_bin.display(), std::env::var("PATH").expect("PATH set"));
    let start = Instant::now();
    let output = render(&config_dir, &input_with_cwd(temp.path()), |command| {
        command.env("PATH", path);
    });
    let elapsed = start.elapsed();
    eprintln!("wrapper grandchild render elapsed: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "wrapper grandchild timeout took {elapsed:?}");
    assert!(output.status.success(), "wrapper render failed: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("git:—"), "git timeout fallback missing: {stdout:?}");
}

#[test]
fn test_pin_only_render_preserves_cache() {
    let temp = TempDir::new("pin-only-cache");
    let config_dir = temp.path().join("config");
    let cwd = temp.path().join("project");
    let now = 1_700_000_000_000_i64;
    write_settings(&config_dir, serde_json::json!({
        "rows": [{"segments": ["node"]}]
    }));
    fs::create_dir_all(config_dir.join("lookups")).expect("create lookup directory");
    fs::create_dir_all(&cwd).expect("create project directory");
    fs::write(cwd.join(".nvmrc"), "v20.11.1\n").expect("write node pin");
    let original = serde_json::json!({
        "git": {cwd.to_string_lossy(): {"value": "main", "fetchedAt": now}},
        "node": {"value": "node:20.10.0", "fetchedAt": now},
        "python": {"value": "py:3.12.1", "fetchedAt": now}
    });
    let cache_path = config_dir.join("lookups/cache.json");
    fs::write(&cache_path, serde_json::to_vec(&original).expect("serialize cache"))
        .expect("seed cache");

    let output = render(&config_dir, &input_with_cwd(&cwd), |command| {
        command.env("PPULSE_NOW_MS", now.to_string());
    });
    assert!(output.status.success(), "pin-only render failed: {:?}", output.stderr);
    let after: serde_json::Value = serde_json::from_slice(
        &fs::read(&cache_path).expect("read cache after render"),
    )
    .expect("parse cache after render");
    assert_eq!(after, original, "pin-only render must preserve all warm cache entries");
}
