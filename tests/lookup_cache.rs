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
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-s05-{}-{}-{}",
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

fn write_executable(path: &Path, program: &str) {
    fs::write(path, program).expect("write stub executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("read stub permissions").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make stub executable");
    }
}

fn counter_lines(counter: &Path) -> Vec<String> {
    fs::read_to_string(counter)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn assert_no_lookup_temps(lookups: &Path) {
    if let Ok(entries) = fs::read_dir(lookups) {
        for entry in entries {
            let name = entry.expect("read lookup directory entry").file_name();
            assert!(
                !name.to_string_lossy().contains(".tmp-"),
                "stale lookup temporary file remains: {}",
                name.to_string_lossy()
            );
        }
    }
}

fn run_render(
    config_dir: &Path,
    stub_repo: &Path,
    stub_bin: &Path,
    counter: &Path,
    stdin: &[u8],
    now_ms: i64,
) -> Output {
    let path = format!(
        "{}:{}",
        stub_bin.display(),
        std::env::var("PATH").expect("PATH is set for stub lookup")
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("render")
        .current_dir(stub_repo)
        .env("PPULSE_CONFIG_DIR", config_dir)
        .env("PPULSE_NOW_MS", now_ms.to_string())
        .env("PPULSE_LOOKUP_COUNTER", counter)
        .env("PATH", path)
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render");
    child
        .stdin
        .take()
        .expect("render stdin")
        .write_all(stdin)
        .expect("write render stdin");
    child.wait_with_output().expect("wait for phosphorpulse render")
}

// REQ-05 / S-05: per-cwd git and global runtime lookup caches honor TTL and future misses.
#[test]
fn test_s05_cache_ttl_semantics() {
    let temp = TempDir::new();
    let config_dir = temp.path().join("config");
    let lookups_dir = config_dir.join("lookups");
    let cache_path = lookups_dir.join("cache.json");
    let stub_repo = temp.path().join("stub-repo");
    let stub_bin = temp.path().join("bin");
    let counter = temp.path().join("lookup-counter");
    fs::create_dir_all(&config_dir).expect("create config directory");
    fs::create_dir_all(&stub_repo).expect("create stub repository directory");
    fs::create_dir_all(&stub_bin).expect("create stub bin directory");
    fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "activeTemplate": "matrix-tron",
            "colorDepth": "auto",
            "rows": [{
                "layout": "auto",
                "segments": ["git", "node", "python"]
            }]
        }))
        .expect("serialize settings containing git/node/python segments"),
    )
    .expect("write settings containing git/node/python segments");

    for (name, output) in [
        ("git", "## main...origin/main [ahead 1]"),
        ("node", "v24.18.1"),
        ("python3", "Python 3.12.0"),
    ] {
        write_executable(
            &stub_bin.join(name),
            &format!("#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$PPULSE_LOOKUP_COUNTER\"\nprintf '%s\\n' '{output}'\n"),
        );
    }
    let warmup_counter = temp.path().join("lookup-warmup-counter");
    for name in ["git", "node", "python3"] {
        let status = Command::new(stub_bin.join(name))
            .env("PPULSE_LOOKUP_COUNTER", &warmup_counter)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("warm stub executable");
        assert!(status.success(), "stub warm-up must succeed: {name}");
    }

    let mut stdin: serde_json::Value = serde_json::from_slice(
        &fs::read("tests/golden/main-default/stdin.json").expect("read golden stdin"),
    )
    .expect("parse golden stdin");
    stdin["cwd"] = serde_json::Value::String(stub_repo.to_string_lossy().into_owned());
    let stdin = serde_json::to_vec(&stdin).expect("serialize rewritten stdin");
    let now = 1_755_150_000_000_i64;

    let first = run_render(&config_dir, &stub_repo, &stub_bin, &counter, &stdin, now);
    assert!(first.status.success(), "first render failed: {:?}", first.stderr);
    assert!(cache_path.is_file(), "first render must create lookups/cache.json");
    let cache: serde_json::Value = serde_json::from_slice(&fs::read(&cache_path).expect("read cache"))
        .expect("cache.json must be valid JSON");
    for key in ["node", "python"] {
        assert!(cache[key]["fetchedAt"].as_i64().is_some(), "{key} fetchedAt must be epoch ms");
    }
    assert!(
        cache["git"][stub_repo.to_string_lossy().as_ref()]["fetchedAt"]
            .as_i64()
            .is_some(),
        "git must be keyed by the render cwd with an epoch-ms fetchedAt"
    );
    assert_no_lookup_temps(&lookups_dir);
    let after_first = counter_lines(&counter);
    assert_eq!(after_first.len(), 3, "first render must fork git, node, and python3 once");

    let second = run_render(&config_dir, &stub_repo, &stub_bin, &counter, &stdin, now);
    assert!(second.status.success(), "second render failed: {:?}", second.stderr);
    assert_eq!(counter_lines(&counter), after_first, "fresh cache must perform zero forks");
    assert_no_lookup_temps(&lookups_dir);

    let third = run_render(
        &config_dir,
        &stub_repo,
        &stub_bin,
        &counter,
        &stdin,
        now + 60_001,
    );
    assert!(third.status.success(), "third render failed: {:?}", third.stderr);
    let after_third = counter_lines(&counter);
    assert_eq!(after_third.len(), after_first.len() + 3, "expired cache must re-fork all lookups");
    let refreshed: serde_json::Value = serde_json::from_slice(&fs::read(&cache_path).expect("read refreshed cache"))
        .expect("refreshed cache must be valid JSON");
    assert_eq!(refreshed["node"]["fetchedAt"].as_i64(), Some(now + 60_001));
    assert_eq!(refreshed["python"]["fetchedAt"].as_i64(), Some(now + 60_001));
    assert_eq!(
        refreshed["git"][stub_repo.to_string_lossy().as_ref()]["fetchedAt"].as_i64(),
        Some(now + 60_001)
    );
    assert_no_lookup_temps(&lookups_dir);

    fs::write(
        &cache_path,
        serde_json::to_vec(&serde_json::json!({
            "git": {stub_repo.to_string_lossy().as_ref(): {"value": "cached", "fetchedAt": now + 120_000}},
            "node": {"value": "cached", "fetchedAt": now + 120_000},
            "python": {"value": "cached", "fetchedAt": now + 120_000}
        }))
        .expect("serialize future cache"),
    )
    .expect("write future cache");
    let fourth = run_render(&config_dir, &stub_repo, &stub_bin, &counter, &stdin, now);
    assert!(fourth.status.success(), "fourth render failed: {:?}", fourth.stderr);
    assert_eq!(
        counter_lines(&counter).len(),
        after_third.len() + 3,
        "future fetchedAt must be treated as a cache miss"
    );
    assert_no_lookup_temps(&lookups_dir);
}
