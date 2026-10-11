//! Subagent row features: `segments.name.roleIcons` glyph prefix for the `name`
//! segment, and the opt-in `activity` sparkline segment fed by `tokenSamples`.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-subagent-role-icons-{}-{}-{}",
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

/// Run `render --subagent` with the given settings and raw stdin bytes; return
/// the raw stdout.
fn run_render(settings: &Value, stdin: &[u8]) -> Vec<u8> {
    let temp = TempDir::new();
    let config = temp.path().join("config");
    fs::create_dir_all(&config).expect("create config directory");
    fs::write(config.join("settings.json"), settings.to_string()).expect("write settings");

    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .args(["render", "--subagent"])
        .env_clear()
        .env("HOME", temp.path())
        .env("PPULSE_CONFIG_DIR", &config)
        .env("PPULSE_NOW_MS", "1755150000000")
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render --subagent");
    child
        .stdin
        .take()
        .expect("open stdin")
        .write_all(stdin)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for render");
    assert!(
        output.status.success(),
        "render --subagent exited {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn settings(segments: &[&str], role_icons: Option<Value>) -> Value {
    let mut value = json!({
        "activeTemplate": "matrix-tron",
        "colorDepth": "auto",
        "rows": [
            { "layout": "auto", "segments": ["model", "effort", "git", "dir", "flex", "ctx"] }
        ],
        "subagent": { "segments": segments },
    });
    if let Some(icons) = role_icons {
        value["segments"] = json!({ "name": { "roleIcons": icons } });
    }
    value
}

/// Render one running task and return the ANSI-stripped row text.
fn render_row(settings: &Value, task_extra: Value) -> String {
    let mut task = json!({
        "id": "task0001",
        "type": "local_agent",
        "status": "running",
        "description": "probe",
        "label": "probe",
        "name": "builder",
        "startTime": 1755149000000_i64,
        "model": "claude-sonnet-5-5",
        "effort": "medium",
        "contextWindowSize": 1000000,
        "tokenCount": 1000,
    });
    for (key, value) in task_extra.as_object().expect("extra is an object") {
        task[key] = value.clone();
    }
    let stdin = json!({
        "session_id": "00000000-0000-0000-0000-000000000000",
        "columns": 120,
        "tasks": [task],
    });
    let stdout = run_render(settings, stdin.to_string().as_bytes());
    let row: Value = serde_json::from_slice(&stdout).expect("stdout is one JSON row");
    strip_ansi(row["content"].as_str().expect("row has string content"))
}

#[test]
fn test_role_icon_prefixes_mapped_agent_type() {
    let cfg = settings(&["name"], Some(json!({ "builder": "⚒", "scout": "◎" })));
    let row = render_row(&cfg, json!({ "agentType": "builder" }));
    assert_eq!(row.trim(), "⚒ builder", "mapped type gets glyph + one space");

    let row = render_row(&cfg, json!({ "agentType": "scout", "name": "finder" }));
    assert_eq!(row.trim(), "◎ finder");
}

#[test]
fn test_role_icon_absent_when_unmapped_or_missing() {
    let plain = settings(&["name"], None);
    let baseline = render_row(&plain, json!({}));
    assert_eq!(baseline.trim(), "builder");

    let cfg = settings(&["name"], Some(json!({ "builder": "⚒" })));
    // Type not in the map.
    let unmapped = render_row(&cfg, json!({ "agentType": "scout" }));
    assert_eq!(unmapped, baseline, "unmapped agentType must not prefix");
    // agentType missing.
    let missing = render_row(&cfg, json!({}));
    assert_eq!(missing, baseline, "missing agentType must not prefix");
    // Map absent, agentType present.
    let no_map = render_row(&plain, json!({ "agentType": "builder" }));
    assert_eq!(no_map, baseline, "absent roleIcons must not prefix");
}

#[test]
fn test_activity_sparkline_shapes() {
    let cfg = settings(&["name", "activity"], None);
    let row_for = |samples: Value| render_row(&cfg, json!({ "tokenSamples": samples }));
    let spark = |row: &str| -> String {
        row.chars()
            .filter(|c| "▁▂▃▄▅▆▇█".contains(*c))
            .collect()
    };

    // Missing / empty: segment omitted entirely.
    let omitted = render_row(&cfg, json!({}));
    assert_eq!(omitted.trim(), "builder", "missing tokenSamples renders nothing");
    assert_eq!(row_for(json!([])).trim(), "builder", "empty tokenSamples renders nothing");

    // 1 sample -> 1 char (all-equal degenerate case).
    assert_eq!(spark(&row_for(json!([5]))), "▁");
    // All-equal -> lowest block repeated.
    assert_eq!(spark(&row_for(json!([7, 7, 7, 7]))), "▁▁▁▁");
    // Increasing ramp across the full range.
    assert_eq!(
        spark(&row_for(json!([0, 1, 2, 3, 4, 5, 6, 7]))),
        "▁▂▃▄▅▆▇█"
    );
    // 16 samples -> 16 chars.
    let sixteen: Vec<u32> = (0..16).collect();
    let got = spark(&row_for(json!(sixteen)));
    assert_eq!(got.chars().count(), 16);
    assert_eq!(got.chars().next(), Some('▁'));
    assert_eq!(got.chars().last(), Some('█'));
    // 20 samples -> last 16 only: first four (huge) values are dropped, so the
    // remaining ramp 4..=19 scales from its own min to max.
    let mut twenty = vec![1000, 1000, 1000, 1000];
    twenty.extend(4..20);
    let got = spark(&row_for(json!(twenty)));
    assert_eq!(got.chars().count(), 16);
    assert_eq!(got.chars().next(), Some('▁'));
    assert_eq!(got.chars().last(), Some('█'));
    // Non-numeric entries are skipped.
    assert_eq!(
        spark(&row_for(json!([0, "x", null, 7, {"a": 1}]))),
        "▁█"
    );
}

/// Already passes today: `activity` is not in the default `subagent` list and
/// no `roleIcons` map is configured, so the golden default output is unchanged.
#[test]
fn test_defaults_unchanged() {
    let dir = "tests/golden/subagent-default";
    let settings: Value =
        serde_json::from_slice(&fs::read(format!("{dir}/settings.json")).expect("read settings"))
            .expect("parse settings");
    let stdin = fs::read(format!("{dir}/stdin.json")).expect("read stdin");
    let expected = fs::read(format!("{dir}/expected.bin")).expect("read expected.bin");
    let actual = run_render(&settings, &stdin);
    assert_eq!(
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected),
        "default subagent output must stay byte-identical"
    );
}
