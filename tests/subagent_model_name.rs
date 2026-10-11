//! Regression test for subagent model display names: `model_short()` must join
//! consecutive numeric version parts of at most two digits with '.', while a
//! longer numeric part (a date stamp) stays separated by a space.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-subagent-model-{}-{}-{}",
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

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Render one synthesized subagent task through `render --subagent` and return
/// the ANSI-stripped row text.
fn render_row(model: &str) -> String {
    let temp = TempDir::new();
    let config = temp.path().join("config");
    fs::create_dir_all(&config).expect("create config directory");
    fs::copy(
        "tests/golden/subagent-default/settings.json",
        config.join("settings.json"),
    )
    .expect("copy subagent settings");

    let stdin = serde_json::json!({
        "session_id": "00000000-0000-0000-0000-000000000000",
        "columns": 120,
        "tasks": [{
            "id": "task0001",
            "type": "local_agent",
            "status": "running",
            "description": "model name probe",
            "label": "model name probe",
            "startTime": 1755149000000_i64,
            "model": model,
            "effort": "medium",
            "contextWindowSize": 1000000,
            "tokenCount": 1000,
        }],
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .args(["render", "--subagent"])
        .env_clear()
        .env("HOME", temp.path())
        .env("PPULSE_CONFIG_DIR", &config)
        .env("PPULSE_NOW_MS", "1755150000000")
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render --subagent");
    child
        .stdin
        .take()
        .expect("open stdin")
        .write_all(stdin.to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for render");
    assert!(
        output.status.success(),
        "render --subagent for {model:?} exited {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let row: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON row");
    strip_ansi(row["content"].as_str().expect("row has string content"))
}

#[test]
fn test_model_short_joins_version_digits() {
    let cases = [
        ("claude-sonnet-5-5", "Sonnet 5.5"),
        ("claude-haiku-5-5", "Haiku 5.5"),
        ("claude-sonnet-5", "Sonnet 5"),
        ("claude-opus-4-1-20250805", "Opus 4.1 20250805"),
        ("claude-3-5-sonnet-20241022", "3.5 Sonnet 20241022"),
        ("claude-fable-5-1", "Fable 5.1"),
    ];
    for (model, expected) in cases {
        let row = render_row(model);
        assert!(
            row.contains(expected),
            "model {model:?}: expected row to contain {expected:?}, got {row:?}"
        );
    }
}
