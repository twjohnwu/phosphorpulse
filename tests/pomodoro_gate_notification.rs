use phosphorpulse::config::default_config;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const MINUTE_MS: f64 = 60_000.0;
const EN: &str = "phosphorpulse: on a break, 04:00 left — send again after the break";

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ppulse-gate-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
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

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64
}

/// Own config JSON: defaults plus `pomodoro.blockDuringRest = true`.
fn config_json() -> String {
    let mut config = default_config();
    config
        .0
        .insert("pomodoro".into(), json!({"blockDuringRest": true}));
    serde_json::to_string(&config.0).unwrap()
}

/// The S-03 shortBreak block row: work, round 1, started 26 min ago, active 2 min ago.
fn short_break_state(n: f64) -> String {
    json!({
        "phase": "work",
        "round": 1.0,
        "phaseStartMs": n - 26.0 * MINUTE_MS,
        "lastActivityMs": n - 2.0 * MINUTE_MS,
        "sessions": {},
    })
    .to_string()
}

struct GateRun {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    write_result: std::io::Result<()>,
}

fn run_gate(dir: &Path, stdin_bytes: Vec<u8>) -> GateRun {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    cmd.arg("pomodoro-gate").env_clear();
    cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.env("HOME", dir);
    cmd.env("PPULSE_CONFIG_DIR", dir);
    cmd.env("CLAUDE_CODE_ENTRYPOINT", "cli");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // Write from a separate thread so a large payload cannot deadlock against
    // unread stdout/stderr; the write result is kept, not ignored.
    let writer = std::thread::spawn(move || stdin.write_all(&stdin_bytes));
    let out = child.wait_with_output().unwrap();
    let write_result = writer.join().unwrap();
    GateRun {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        write_result,
    }
}

fn hook_input(prompt: &str) -> String {
    json!({"hook_event_name": "UserPromptSubmit", "prompt": prompt}).to_string()
}

/// A JSON object of exactly `total` bytes: `prompt` = `head` + ASCII space padding.
fn padded_input(head: &str, total: usize) -> Vec<u8> {
    let prefix = format!(r#"{{"prompt":"{head}"#);
    let suffix = r#""}"#;
    let pad = total - prefix.len() - suffix.len();
    let mut out = String::with_capacity(total);
    out.push_str(&prefix);
    out.push_str(&" ".repeat(pad));
    out.push_str(suffix);
    assert_eq!(out.len(), total);
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert!(parsed.is_object());
    out.into_bytes()
}

/// REQ-03 / S-08
#[test]
fn test_s08_notification_and_indeterminate_stdin() {
    let task_notification =
        "<task-notification>\n<task-id>x</task-id>\n</task-notification>";
    let mut bom_hello = vec![0xEF, 0xBB, 0xBF];
    bom_hello.extend_from_slice(hook_input("hello").as_bytes());

    // (case, stdin, expect pass-through)
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("a", hook_input(task_notification).into_bytes(), true),
        ("b", hook_input(&format!("\n  {task_notification}")).into_bytes(), true),
        (
            "c",
            hook_input("<agent-message from=\"a1\">\nreport\n</agent-message>").into_bytes(),
            true,
        ),
        ("d", hook_input("hello").into_bytes(), false),
        ("e", hook_input("hello <task-notification>").into_bytes(), false),
        ("f", b"{".to_vec(), true),
        ("g", br#"{"hook_event_name":"UserPromptSubmit"}"#.to_vec(), true),
        ("h", br#"{"hook_event_name":"UserPromptSubmit","prompt":1}"#.to_vec(), true),
        ("i", bom_hello, true),
        ("j", padded_input("hello", 9_437_184), true),
        ("k", padded_input("<task-notification>", 2_097_152), true),
    ];

    let mut failures: Vec<String> = Vec::new();
    for (case, stdin_bytes, pass) in cases {
        let dir = TempDir::new("s08");
        fs::write(dir.path().join("settings.json"), config_json()).unwrap();
        fs::create_dir_all(dir.path().join("pomodoro")).unwrap();
        fs::write(
            dir.path().join("pomodoro/shared.json"),
            short_break_state(now_ms()),
        )
        .unwrap();

        let run = run_gate(dir.path(), stdin_bytes);

        if pass {
            if run.code != Some(0) {
                failures.push(format!(
                    "case {case}: expected exit 0, got {:?}, stderr={:?}",
                    run.code, run.stderr
                ));
            }
            if !run.stdout.is_empty() {
                failures.push(format!("case {case}: stdout={:?}", run.stdout));
            }
            if run.code == Some(0) && !run.stderr.is_empty() {
                failures.push(format!("case {case}: stderr={:?}", run.stderr));
            }
        } else {
            if run.code != Some(2) {
                failures.push(format!(
                    "case {case}: expected exit 2, got {:?}, stderr={:?}",
                    run.code, run.stderr
                ));
            }
            if !run.stdout.is_empty() {
                failures.push(format!("case {case}: stdout={:?}", run.stdout));
            }
            if run.stderr.trim_end_matches('\n') != EN || run.stderr.trim_end_matches('\n').contains('\n') {
                failures.push(format!("case {case}: stderr={:?}", run.stderr));
            }
        }
        if (case == "j" || case == "k") && run.write_result.is_err() {
            failures.push(format!(
                "case {case}: write_all failed: {:?}",
                run.write_result
            ));
        }
    }
    assert!(failures.is_empty(), "failing cases:\n{}", failures.join("\n"));
}
