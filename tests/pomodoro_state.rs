use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-s06-{}-{}-{}",
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

fn assert_ts_persisted_schema(value: &serde_json::Value) {
    let doc = value.as_object().expect("shared.json must be an object");
    assert!(matches!(doc.get("phase").and_then(serde_json::Value::as_str), Some("work" | "shortBreak" | "longBreak" | "stopped")));
    for field in ["round", "phaseStartMs", "lastActivityMs"] {
        assert!(
            doc.get(field).and_then(serde_json::Value::as_f64).is_some_and(|n| n.is_finite() && n >= 0.0),
            "{field} must be a finite non-negative number"
        );
    }
    let sessions = doc.get("sessions").and_then(serde_json::Value::as_object).expect("sessions must be an object");
    for id in sessions.keys() {
        assert!(
            !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
            "session id must be whitelisted: {id}"
        );
    }
    if let Some(last_notified) = doc.get("lastNotifiedAtMs") {
        assert!(
            last_notified.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0),
            "lastNotifiedAtMs must be a finite non-negative number"
        );
    }
}

fn write_settings(config_dir: &Path) {
    fs::create_dir_all(config_dir).expect("create config directory");
    fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "activeTemplate": "matrix-tron",
            "colorDepth": "auto",
            "pomodoro": { "workMin": 25 },
            "rows": [{ "layout": "auto", "segments": ["pomodoro"] }]
        }))
        .expect("serialize pomodoro settings"),
    )
    .expect("write pomodoro settings");
}

fn write_fixture_state(config_dir: &Path, now_ms: i64, last_notified_at_ms: Option<i64>) {
    let mut state: serde_json::Value = serde_json::from_slice(
        &fs::read("tests/golden/pomodoro-even/state/pomodoro/shared.json")
            .expect("read real TS-written pomodoro fixture"),
    )
    .expect("parse real TS-written pomodoro fixture");
    state["phaseStartMs"] = serde_json::json!(now_ms - 25 * 60_000);
    state["lastActivityMs"] = serde_json::json!(now_ms);
    state["sessions"] = serde_json::json!({
        "9fa8736b-dae3-41c6-8846-b5fdc7f0227c": { "signature": "2:828", "lastSeenMs": now_ms }
    });
    if let Some(last_notified_at_ms) = last_notified_at_ms {
        state["lastNotifiedAtMs"] = serde_json::json!(last_notified_at_ms);
    }
    let state_dir = config_dir.join("pomodoro");
    fs::create_dir_all(&state_dir).expect("create pomodoro state directory");
    fs::write(
        state_dir.join("shared.json"),
        serde_json::to_vec(&state).expect("serialize fixture-derived state"),
    )
    .expect("write fixture-derived state");
}

fn run_render(config_dir: &Path, stub_bin: &Path, counter: &Path, stdin: &[u8], now_ms: i64) -> Output {
    let path = format!(
        "{}:{}",
        stub_bin.display(),
        std::env::var("PATH").expect("PATH is set for stub lookup")
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("render")
        .env("PPULSE_CONFIG_DIR", config_dir)
        .env("PPULSE_NOW_MS", now_ms.to_string())
        .env("PPULSE_OSASCRIPT_COUNTER", counter)
        .env("PATH", path)
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render");
    child.stdin.take().expect("render stdin").write_all(stdin).expect("write render stdin");
    child.wait_with_output().expect("wait for phosphorpulse render")
}

fn wait_for_counter_len(counter: &Path, expected: usize) {
    for _ in 0..100 {
        if counter_lines(counter).len() == expected {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(counter_lines(counter).len(), expected, "osascript invocation count");
}

// REQ-05 / S-06: fixture-compatible pomodoro state renders, persists, and throttles notifications.
#[test]
fn test_s06_pomodoro_compat_and_notify() {
    let temp = TempDir::new();
    let stub_bin = temp.path().join("bin");
    let counter = temp.path().join("osascript-counter");
    let warmup_counter = temp.path().join("osascript-warmup-counter");
    fs::create_dir_all(&stub_bin).expect("create stub bin directory");
    write_executable(
        &stub_bin.join("osascript"),
        "#!/bin/sh\nprintf '%s\\n' invoked >> \"$PPULSE_OSASCRIPT_COUNTER\"\n",
    );
    let warmup = Command::new(stub_bin.join("osascript"))
        .env("PPULSE_OSASCRIPT_COUNTER", &warmup_counter)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("warm osascript stub executable");
    assert!(warmup.success(), "osascript stub warm-up must succeed");
    fs::remove_file(&counter).ok();

    let stdin = fs::read("tests/golden/pomodoro-even/stdin.json").expect("read golden stdin");
    let now = 1_755_150_100_000_i64;

    let throttled_config = temp.path().join("throttled-config");
    write_settings(&throttled_config);
    write_fixture_state(&throttled_config, now, Some(now - 29_999));
    let throttled = run_render(&throttled_config, &stub_bin, &counter, &stdin, now);
    assert!(throttled.status.success(), "throttled render failed: {:?}", String::from_utf8_lossy(&throttled.stderr));
    assert!(String::from_utf8_lossy(&throttled.stdout).contains("☕ 05:00"), "work-period boundary must render short-break mm:ss: {:?}", String::from_utf8_lossy(&throttled.stdout));
    assert!(counter_lines(&counter).is_empty(), "within 30 seconds must not invoke osascript");
    let throttled_state: serde_json::Value = serde_json::from_slice(&fs::read(throttled_config.join("pomodoro/shared.json")).expect("read throttled state")).expect("throttled state must be JSON");
    assert_ts_persisted_schema(&throttled_state);
    assert_eq!(throttled_state["lastNotifiedAtMs"].as_i64(), Some(now - 29_999), "within 30 seconds must not update lastNotifiedAtMs");

    let notify_config = temp.path().join("notify-config");
    write_settings(&notify_config);
    write_fixture_state(&notify_config, now, Some(now - 30_001));
    let notify = run_render(&notify_config, &stub_bin, &counter, &stdin, now);
    assert!(notify.status.success(), "notify render failed: {:?}", String::from_utf8_lossy(&notify.stderr));
    assert!(String::from_utf8_lossy(&notify.stdout).contains("☕ 05:00"), "work-period boundary must render short-break mm:ss");
    wait_for_counter_len(&counter, 1);
    let notified_state: serde_json::Value = serde_json::from_slice(&fs::read(notify_config.join("pomodoro/shared.json")).expect("read notified state")).expect("notified state must be JSON");
    assert_ts_persisted_schema(&notified_state);
    assert_eq!(notified_state["lastNotifiedAtMs"].as_i64(), Some(now), "beyond 30 seconds must update lastNotifiedAtMs to now");

    let invalid_config = temp.path().join("invalid-config");
    write_settings(&invalid_config);
    let invalid_state_path = invalid_config.join("pomodoro/shared.json");
    fs::create_dir_all(invalid_state_path.parent().expect("state parent")).expect("create invalid state directory");
    fs::write(&invalid_state_path, b"{ definitely not JSON").expect("write corrupt fixture");
    let invalid = run_render(&invalid_config, &stub_bin, &counter, &stdin, now);
    assert!(invalid.status.success(), "corrupt state must not crash render: {:?}", String::from_utf8_lossy(&invalid.stderr));
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("⏱ 25:00"), "corrupt state must be treated as absent and render fresh state");
    let fresh_state: serde_json::Value = serde_json::from_slice(&fs::read(&invalid_state_path).expect("read fresh state after corrupt fixture")).expect("fresh state must replace corrupt JSON");
    assert_ts_persisted_schema(&fresh_state);
}
