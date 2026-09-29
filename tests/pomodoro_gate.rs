use phosphorpulse::config::default_config;
use phosphorpulse::segments::pomodoro::{format_remaining, project};
use serde_json::{json, Value};

const T: f64 = 1_790_000_000_000.0;

fn min(m: f64) -> f64 {
    m * 60_000.0
}

/// REQ-01 / S-01
#[test]
fn test_s01_block_during_rest_validation() {
    let cases = [
        ("A", json!({}), true),
        ("B", json!({"blockDuringRest": true}), true),
        ("C", json!({"blockDuringRest": false}), true),
        ("D", json!({"blockDuringRest": "true"}), false),
        ("E", json!({"blockDuringRest": 1}), false),
    ];
    for (name, pomodoro, ok) in cases {
        let mut config = default_config();
        config.0.insert("pomodoro".into(), pomodoro);
        let result = config.validate_renderable();
        if ok {
            assert!(result.is_ok(), "case {name}: {result:?}");
        } else {
            assert_eq!(
                result,
                Err("/pomodoro/blockDuringRest: must be a boolean".to_string()),
                "case {name}"
            );
        }
    }
}

fn state(phase: &str, round: f64, start: f64, activity: f64) -> Value {
    json!({
        "phase": phase,
        "round": round,
        "phaseStartMs": T + min(start),
        "lastActivityMs": T + min(activity),
        "sessions": {},
    })
}

/// REQ-02 / S-02
#[test]
fn test_s02_advance_projection_and_format() {
    // (row, state, now offset min, phase, round, rest remaining ms)
    let rows = [
        ("A", state("work", 1.0, 0.0, 24.0), 26.0, "shortBreak", 1.0, Some(240_000.0)),
        ("B", state("longBreak", 4.0, 0.0, -1.0), 16.0, "stopped", 4.0, None),
        ("C", state("work", 4.0, 0.0, 24.0), 26.0, "longBreak", 4.0, Some(840_000.0)),
        ("D", state("stopped", 1.0, 0.0, 0.0), 60.0, "stopped", 1.0, None),
        ("E", state("shortBreak", 1.0, 0.0, -1.0), 6.0, "work", 2.0, None),
        ("F", state("longBreak", 4.0, 0.0, 10.0), 16.0, "work", 1.0, None),
        ("G", state("work", 1.0, 0.0, 10.0), 26.0, "stopped", 1.0, None),
        ("H", state("shortBreak", 1.0, 0.0, -9.0), 6.0, "stopped", 1.0, None),
    ];
    for (name, doc, now, phase, round, rest) in rows {
        let p = project(doc, T + min(now), 25.0).unwrap_or_else(|| panic!("row {name}: None"));
        assert_eq!(p.phase, phase, "row {name} phase");
        assert_eq!(p.round, round, "row {name} round");
        assert_eq!(p.rest_remaining_ms, rest, "row {name} rest");
    }
    assert_eq!(format_remaining(240_000.0), "04:00");
    assert_eq!(format_remaining(239_001.0), "04:00");
    assert_eq!(format_remaining(1.0), "00:01");
    assert_eq!(format_remaining(0.0), "00:00");
}

// ---- REQ-03 gate helpers (binary end-to-end) ----

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Own config JSON: defaults plus optional `pomodoro` fields and `language`.
fn config_json(block: Option<bool>, language: Option<&str>, work_min: Option<f64>) -> String {
    let mut config = default_config();
    let mut pomodoro = serde_json::Map::new();
    if let Some(b) = block {
        pomodoro.insert("blockDuringRest".into(), json!(b));
    }
    if let Some(w) = work_min {
        pomodoro.insert("workMin".into(), json!(w));
    }
    config.0.insert("pomodoro".into(), Value::Object(pomodoro));
    if let Some(lang) = language {
        config.0.insert("language".into(), json!(lang));
    }
    serde_json::to_string(&config.0).unwrap()
}

fn gate_state(n: f64, phase: &str, round: f64, start_ago_min: f64, activity_ago_min: f64) -> String {
    json!({
        "phase": phase,
        "round": round,
        "phaseStartMs": n - min(start_ago_min),
        "lastActivityMs": n - min(activity_ago_min),
        "sessions": {},
    })
    .to_string()
}

fn write_files(dir: &Path, config: Option<&str>, shared: Option<&str>) {
    if let Some(c) = config {
        fs::write(dir.join("settings.json"), c).unwrap();
    }
    if let Some(s) = shared {
        fs::create_dir_all(dir.join("pomodoro")).unwrap();
        fs::write(dir.join("pomodoro/shared.json"), s).unwrap();
    }
}

struct GateRun {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_gate(dir: &Path, entrypoint: Option<&str>, extra_env: &[(&str, String)]) -> GateRun {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    cmd.arg("pomodoro-gate").env_clear();
    cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.env("HOME", dir);
    cmd.env("PPULSE_CONFIG_DIR", dir);
    if let Some(e) = entrypoint {
        cmd.env("CLAUDE_CODE_ENTRYPOINT", e);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        // The gate may exit before reading; a broken pipe here is not a test failure.
        let _ = stdin.write_all(br#"{"hook_event_name":"UserPromptSubmit","prompt":"x"}"#);
    }
    let out = child.wait_with_output().unwrap();
    GateRun {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            out.push(path.clone());
            if path.is_dir() {
                walk(&path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// REQ-03 / S-03
#[test]
fn test_s03_gate_decision_table() {
    const EN: &str = "phosphorpulse: on a break, 04:00 left — send again after the break";
    const ZH: &str = "phosphorpulse：休息中，還剩 04:00，休息結束後再送出";
    // (row, block, entrypoint, kind, exit code)
    let rows: [(&str, Option<bool>, Option<&str>, &str, i32); 9] = [
        ("false-or-missing", None, Some("cli"), "short", 0),
        ("true-cli-short", Some(true), Some("cli"), "short", 2),
        ("true-vscode-long", Some(true), Some("claude-vscode"), "long", 2),
        ("true-desktop-short", Some(true), Some("claude-desktop"), "short", 2),
        ("true-sdk-cli", Some(true), Some("sdk-cli"), "short", 0),
        ("true-mcp-long", Some(true), Some("mcp"), "long", 0),
        ("true-unset", Some(true), None, "short", 0),
        ("true-cli-work", Some(true), Some("cli"), "work", 0),
        ("true-cli-stopped", Some(true), Some("cli"), "stopped", 0),
    ];
    for (row, block, entrypoint, kind, code) in rows {
        for language in [None, Some("zh-TW")] {
            let label = format!("row {row} lang {language:?}");
            let dir = TempDir::new("s03");
            let n = now_ms();
            let shared = match kind {
                "short" => gate_state(n, "work", 1.0, 26.0, 2.0),
                "long" => gate_state(n, "work", 4.0, 26.0, 2.0),
                "work" => gate_state(n, "work", 1.0, 10.0, 10.0),
                _ => gate_state(n, "stopped", 1.0, 26.0, 2.0),
            };
            write_files(dir.path(), Some(&config_json(block, language, None)), Some(&shared));
            let files_before = list_files(dir.path());
            let shared_path = dir.path().join("pomodoro/shared.json");
            let bytes_before = fs::read(&shared_path).unwrap();

            let run = run_gate(dir.path(), entrypoint, &[]);

            assert_eq!(run.code, Some(code), "{label}: exit code, stderr={:?}", run.stderr);
            assert_eq!(run.stdout, "", "{label}: stdout");
            if code == 0 {
                assert_eq!(run.stderr, "", "{label}: stderr");
            } else if kind == "short" && row == "true-cli-short" {
                let expected = if language.is_some() { ZH } else { EN };
                assert_eq!(run.stderr.trim_end_matches('\n'), expected, "{label}: stderr");
            } else {
                let line = run.stderr.trim_end_matches('\n');
                assert!(!line.is_empty() && !line.contains('\n'), "{label}: stderr {line:?}");
            }
            assert_eq!(list_files(dir.path()), files_before, "{label}: file list changed");
            assert_eq!(fs::read(&shared_path).unwrap(), bytes_before, "{label}: shared.json changed");
        }
    }
}

/// REQ-03 / S-04
#[test]
fn test_s04_fail_open() {
    let pad = "x".repeat(70_000);
    let cases = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];
    let mut failures: Vec<String> = Vec::new();
    for case in cases {
        let dir = TempDir::new("s04");
        let n = now_ms();
        let mut config = config_json(Some(true), None, None);
        let mut shared = gate_state(n, "work", 1.0, 26.0, 2.0);
        let mut env: Vec<(&str, String)> = Vec::new();
        match case {
            "a" => write_files(dir.path(), Some(&config), None),
            "b" => write_files(dir.path(), Some(&config), Some("{")),
            "c" => {
                shared = json!({
                    "phase": "work", "round": 1.0,
                    "phaseStartMs": n - min(26.0), "lastActivityMs": n - min(2.0),
                    "sessions": {}, "pad": pad,
                })
                .to_string();
                assert!(shared.len() > 65_536);
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            "d" => write_files(dir.path(), None, Some(&shared)),
            "e" => write_files(dir.path(), Some("["), Some(&shared)),
            "f" => {
                let mut c = default_config();
                c.0.insert("pomodoro".into(), json!({"blockDuringRest": true}));
                c.0.insert("pad".into(), json!(pad));
                config = serde_json::to_string(&c.0).unwrap();
                assert!(config.len() > 65_536);
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            "g" => {
                config = config_json(Some(true), None, Some(3.0));
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            "h" => {
                shared = gate_state(n, "shortBreak", 1.0, -60.0, 2.0);
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            "i" => {
                config = config_json(Some(false), None, None);
                env.push(("PPULSE_POMODORO_BLOCK_DURING_REST", "true".to_string()));
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            "k" => {
                shared = gate_state(n, "shortBreak", 1.0, 1.0, -60.0);
                write_files(dir.path(), Some(&config), Some(&shared));
            }
            _ => {
                shared = gate_state(n, "work", 1.0, 24.0, 1.0);
                env.push(("PPULSE_NOW_MS", format!("{}", n + min(2.0))));
                write_files(dir.path(), Some(&config), Some(&shared));
            }
        }
        let run = run_gate(dir.path(), Some("cli"), &env);
        if run.code != Some(0) {
            failures.push(format!("case {case}: exit code {:?}, stderr={:?}", run.code, run.stderr));
        }
        if !run.stdout.is_empty() {
            failures.push(format!("case {case}: stdout={:?}", run.stdout));
        }
        if !run.stderr.is_empty() {
            failures.push(format!("case {case}: stderr={:?}", run.stderr));
        }
    }
    assert!(failures.is_empty(), "failing cases:\n{}", failures.join("\n"));
}
