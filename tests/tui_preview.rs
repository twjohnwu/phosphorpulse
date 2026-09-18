//! RED coverage for the isolated, synchronous TUI PreviewPane.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use phosphorpulse::config::model::Config;
use serde_json::json;

#[path = "../src/tui/preview.rs"]
mod preview;

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "phosphorpulse-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create per-test temporary directory");
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

struct EnvGuard {
    home: Option<OsString>,
    ppulse_config_dir: Option<OsString>,
}

impl EnvGuard {
    fn use_temp_home(home: &Path) -> Self {
        let guard = Self {
            home: std::env::var_os("HOME"),
            ppulse_config_dir: std::env::var_os("PPULSE_CONFIG_DIR"),
        };
        // This test must never let preview resolution reach the caller's HOME.
        unsafe {
            std::env::set_var("HOME", home);
            std::env::remove_var("PPULSE_CONFIG_DIR");
        }
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.ppulse_config_dir {
                Some(value) => std::env::set_var("PPULSE_CONFIG_DIR", value),
                None => std::env::remove_var("PPULSE_CONFIG_DIR"),
            }
        }
    }
}

fn tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("read monitored directory") {
            let entry = entry.expect("read monitored entry");
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, snapshot);
            } else {
                snapshot.insert(
                    path.strip_prefix(root)
                        .expect("monitored entry is below root")
                        .to_path_buf(),
                    fs::read(&path).expect("read monitored file"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn preview_draft() -> Config {
    Config(
        json!({
            "activeTemplate": "matrix-tron",
            "colorDepth": "truecolor",
            "rows": [{"layout": "auto", "segments": ["model", "ctx", "pomodoro", "git", "node", "python"]}],
            "subagent": {"segments": ["name", "model", "ctx", "elapsed"]},
            "gauge": {"barWidth": 8, "warnPct": 65, "hotPct": 85},
            "pomodoro": {"workMin": 25, "refreshSec": 5}
        })
        .as_object()
        .expect("preview draft is an object")
        .clone(),
    )
}

/// REQ-04 / R4 / S-17: PreviewPane synchronously renders fixed main and
/// subagent samples with ANSI, never writes the real config directory or forks
/// live git/node/python lookups; elapsed time is reported, not asserted.
#[test]
fn test_s17_preview_isolation() {
    let temp = TempDir::new("s17-preview-isolation");
    let home = temp.path().join("home");
    let real_config = home.join(".claude/phosphorpulse");
    fs::create_dir_all(real_config.join("pomodoro")).expect("create real config canary");
    fs::write(
        real_config.join("pomodoro/shared.json"),
        b"real pomodoro canary",
    )
    .expect("write pomodoro canary");
    fs::write(
        real_config.join("lookup-cache.json"),
        b"real lookup cache canary",
    )
    .expect("write cache canary");
    let _environment = EnvGuard::use_temp_home(&home);
    let before = tree_snapshot(&real_config);

    let draft = preview_draft();
    let main_sample = json!({
        "model": {"display_name": "preview-main-model"},
        "workspace": {"current_dir": "/preview/workspace"},
        "context_window": {"used_percentage": 62},
        "session_id": "preview-session"
    });
    let subagent_sample = json!({
        "tasks": [{
            "id": "preview-subagent",
            "description": "preview-subagent sample",
            "model": "claude-preview",
            "tokenCount": 620,
            "contextWindowSize": 1000,
            "startTime": 0
        }]
    });

    let started = Instant::now();
    let first = preview::render_preview(&draft, main_sample.clone(), subagent_sample.clone());
    let second = preview::render_preview(&draft, main_sample, subagent_sample);
    println!("S-17 preview render time: {:?}", started.elapsed());

    assert_eq!(
        tree_snapshot(&real_config),
        before,
        "preview leaves the real config directory byte-for-byte unchanged"
    );
    assert!(
        first.contains("\x1b["),
        "preview includes ANSI escape styling"
    );
    assert!(
        first.contains("preview-main-model"),
        "preview includes the main sample line"
    );
    assert!(
        first.contains("preview-subagent"),
        "preview includes a subagent sample line"
    );
    assert!(
        !first.contains("git:—") && !first.contains("node:—") && !first.contains("py:—"),
        "preview API consumes fixed samples synchronously instead of live git/node/python lookups"
    );
    assert_eq!(
        first, second,
        "repeated synchronous preview renders are deterministic"
    );
}

/// REQ-06 / S-07
/// GIVEN `preview::main_sample()`；draft config 以 `Config::defaults()` 為底，`rows[0].segments` 為 `["model","session","fastMode","outputStyle","thinking"]`，分別測 `nerdFont:true` 與缺鍵；暫存 config_dir
/// WHEN 呼叫 `render_value_preview_at_columns(sample, draft, config_dir, 120)`，測試端移除 ANSI
/// THEN 第一行依序包含（nerd）`\u{f02b} phosphorpulse`、`\u{f0e7} fast`、`\u{f1fc} Concise`、`\u{f05e} think`；（ascii）`#phosphorpulse`、`fast`、`style:Concise`、`no-think`；且 `main_sample()` 的 `session_id`、`model`、`effort`、`cwd`、`version`、`context_window`、`rate_limits`、`cost` 八個鍵仍存在
#[test]
fn test_s07_preview_sample_shows_extra_segments() {
    fn strip_ansi(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut plain = Vec::with_capacity(bytes.len());
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
                index += 2;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if (0x40..=0x7e).contains(&byte) {
                        break;
                    }
                }
            } else {
                plain.push(bytes[index]);
                index += 1;
            }
        }

        String::from_utf8(plain).expect("stripping ANSI preserves UTF-8")
    }

    fn assert_contains_in_order(line: &str, expected: &[&str]) {
        let mut remaining = line;
        for needle in expected {
            assert!(
                remaining.contains(needle),
                "first line should contain {needle:?} in order: {line:?}"
            );
            let offset = remaining
                .find(needle)
                .expect("contains assertion establishes the match");
            remaining = &remaining[offset + needle.len()..];
        }
    }

    let temp = TempDir::new("s07-preview-extra-segments");
    let home = temp.path().join("home");
    let config_dir = temp.path().join("config");
    fs::create_dir_all(&home).expect("create temporary home");
    fs::create_dir_all(&config_dir).expect("create temporary config directory");
    let _environment = EnvGuard::use_temp_home(&home);

    let sample = phosphorpulse::tui::preview::main_sample();
    for key in [
        "session_id",
        "model",
        "effort",
        "cwd",
        "version",
        "context_window",
        "rate_limits",
        "cost",
    ] {
        assert!(sample.get(key).is_some(), "main_sample keeps {key}");
    }

    let mut draft = serde_json::to_value(Config::defaults()).expect("serialize default config");
    draft["rows"] = json!([{
        "layout": "fixed",
        "segments": ["model", "session", "fastMode", "outputStyle", "thinking"]
    }]);

    let mut nerd_draft = draft.clone();
    nerd_draft["nerdFont"] = json!(true);
    let nerd = phosphorpulse::render::render_value_preview_at_columns(
        sample.clone(),
        nerd_draft,
        config_dir.clone(),
        120,
    );
    let nerd_plain = strip_ansi(&nerd);
    let nerd_first_line = nerd_plain.lines().next().unwrap_or_default();
    assert_contains_in_order(
        nerd_first_line,
        &[
            "\u{f02b} phosphorpulse",
            "\u{f0e7} fast",
            "\u{f1fc} Concise",
            "\u{f05e} think",
        ],
    );

    let ascii = phosphorpulse::render::render_value_preview_at_columns(
        sample,
        draft,
        config_dir,
        120,
    );
    let ascii_plain = strip_ansi(&ascii);
    let ascii_first_line = ascii_plain.lines().next().unwrap_or_default();
    assert_contains_in_order(
        ascii_first_line,
        &["#phosphorpulse", "fast", "style:Concise", "no-think"],
    );
}
