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
