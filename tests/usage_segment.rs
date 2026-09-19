use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::config::model::Config;
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const RESET_DELTA: i64 = 495_420_000;

struct ColumnsGuard {
    columns: Option<OsString>,
}

impl ColumnsGuard {
    fn new() -> Self {
        let guard = Self {
            columns: std::env::var_os("COLUMNS"),
        };
        unsafe {
            std::env::set_var("COLUMNS", "120");
        }
        guard
    }
}

impl Drop for ColumnsGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.columns {
                Some(value) => std::env::set_var("COLUMNS", value),
                None => std::env::remove_var("COLUMNS"),
            }
        }
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "usage_segment_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create test directory");
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
    ppulse_now_ms: Option<OsString>,
}

impl EnvGuard {
    fn new(root: &Path) -> Self {
        let guard = Self {
            home: std::env::var_os("HOME"),
            ppulse_config_dir: std::env::var_os("PPULSE_CONFIG_DIR"),
            ppulse_now_ms: std::env::var_os("PPULSE_NOW_MS"),
        };
        unsafe {
            std::env::set_var("HOME", root);
            std::env::set_var("PPULSE_CONFIG_DIR", root);
            std::env::set_var("PPULSE_NOW_MS", NOW.to_string());
        }
        guard
    }

    fn use_config_dir(&self, config_dir: &Path) {
        unsafe {
            std::env::set_var("PPULSE_CONFIG_DIR", config_dir);
        }
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
            match &self.ppulse_now_ms {
                Some(value) => std::env::set_var("PPULSE_NOW_MS", value),
                None => std::env::remove_var("PPULSE_NOW_MS"),
            }
        }
    }
}

fn single_segment_config(segment: &str) -> Value {
    let mut config = Value::Object(Config::defaults().0);
    config["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    config
}

fn main_line_stdin() -> Value {
    json!({
        "session_id": "usage-segment",
        "cwd": "/tmp/phosphorpulse-usage-segment",
        "model": {"display_name": "Fable"},
        "context_window": {"used_percentage": 42},
        "rate_limits": {}
    })
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn first_fg_sgr(input: &str) -> Option<&str> {
    let mut offset = 0;
    while let Some(start) = input[offset..].find("\x1b[").map(|at| offset + at) {
        let end = input[start..].find('m').map(|at| start + at + 1)?;
        let parameters = &input[start + 2..end - 1];
        let is_foreground = parameters.starts_with("38;2;")
            || parameters.starts_with("38;5;")
            || parameters
                .split(';')
                .filter_map(|value| value.parse::<u8>().ok())
                .any(|value| (30..=37).contains(&value) || (90..=97).contains(&value));
        if is_foreground {
            return Some(&input[start..end]);
        }
        offset = end;
    }
    None
}

fn dim_sgr_from_oracle(warn_sgr: &str) -> String {
    let depth = if warn_sgr.starts_with("\x1b[38;2;") {
        "truecolor"
    } else if warn_sgr.starts_with("\x1b[38;5;") {
        "256color"
    } else {
        "16color"
    };
    phosphorpulse::render::color("#008F11", depth, false)
}

fn fable_limit(fetched_at: i64, resets_at: Option<i64>) -> Value {
    json!({
        "fetchedAt": fetched_at,
        "nextFetchAt": NOW + 290_000,
        "limits": [{
            "displayName": "Fable",
            "percent": 65,
            "resetsAt": resets_at,
            "isActive": true
        }]
    })
}

/// REQ-03 / REQ-04 / S-02: limitModel renders cache freshness without refreshing it.
#[test]
fn test_s02_limit_model_display() {
    let temp = TempDir::new();
    let env = EnvGuard::new(temp.path());
    let _columns = ColumnsGuard::new();

    let mut oracle_stdin = main_line_stdin();
    oracle_stdin["rate_limits"]["seven_day"] = json!({
        "used_percentage": 65,
        "resets_at": (NOW + RESET_DELTA) / 1000
    });
    let oracle_raw =
        phosphorpulse::render::render_value(oracle_stdin, single_segment_config("limit7d"));
    let oracle_plain = strip_ansi(&oracle_raw);
    let oracle_body = oracle_plain
        .trim()
        .strip_prefix("7d ")
        .expect("limit7d oracle has its label");
    let bar = oracle_body
        .split_once(" 65%")
        .expect("limit7d oracle has its percentage")
        .0;
    let warn_sgr = first_fg_sgr(&oracle_raw)
        .expect("limit7d oracle has a foreground SGR")
        .to_owned();
    let dim_sgr = dim_sgr_from_oracle(&warn_sgr);
    let with_reset = format!("Fable {bar} 65% ↺5d17h");
    let without_reset = format!("Fable {bar} 65%");

    let cases: [(&str, Value, &str, Option<&str>); 7] = [
        (
            "a",
            fable_limit(NOW - 10_000, Some(NOW + RESET_DELTA)),
            &with_reset,
            Some(warn_sgr.as_str()),
        ),
        (
            "b",
            fable_limit(NOW - 1_900_000, Some(NOW + RESET_DELTA)),
            &with_reset,
            Some(dim_sgr.as_str()),
        ),
        (
            "c",
            fable_limit(NOW - 90_000_000, Some(NOW + RESET_DELTA)),
            "--",
            None,
        ),
        (
            "d",
            json!({
                "fetchedAt": NOW - 10_000,
                "nextFetchAt": NOW + 290_000,
                "limits": [
                    {"displayName": "Fable", "percent": 65, "resetsAt": NOW + RESET_DELTA, "isActive": true},
                    {"displayName": "Opus", "percent": 12, "resetsAt": null, "isActive": true},
                    {"displayName": "Haiku", "percent": 3, "resetsAt": null, "isActive": false}
                ]
            }),
            &with_reset,
            Some(warn_sgr.as_str()),
        ),
        (
            "e",
            json!({"fetchedAt": NOW - 10_000, "nextFetchAt": NOW + 290_000, "limits": []}),
            "--",
            None,
        ),
        (
            "g",
            fable_limit(NOW - 10_000, None),
            &without_reset,
            Some(warn_sgr.as_str()),
        ),
        (
            "h",
            fable_limit(NOW + 60_000, Some(NOW + RESET_DELTA)),
            "--",
            None,
        ),
    ];

    for (label, cache, expected_text, expected_fg) in cases {
        let config_dir = temp.path().join(label);
        let usage_dir = config_dir.join("usage");
        fs::create_dir_all(&usage_dir).expect("create usage directory");
        let cache_path = usage_dir.join("cache.json");
        let cache_bytes = serde_json::to_vec(&cache).expect("serialize cache fixture");
        fs::write(&cache_path, &cache_bytes).expect("write cache fixture");
        let before = fs::read(&cache_path).expect("snapshot cache fixture");
        env.use_config_dir(&config_dir);

        let raw = phosphorpulse::render::render_value(
            main_line_stdin(),
            single_segment_config("limitModel"),
        );
        let plain = strip_ansi(&raw);
        assert_eq!(plain.trim(), expected_text, "S-02 case {label}");
        if let Some(expected_fg) = expected_fg {
            assert_eq!(
                first_fg_sgr(&raw),
                Some(expected_fg),
                "S-02 case {label} foreground"
            );
        }
        if label == "d" {
            assert!(!plain.contains("Opus"), "S-02 case d selects first active");
        }
        if label == "g" {
            assert!(!plain.contains('↺'), "S-02 case g omits reset countdown");
        }
        assert_eq!(
            fs::read(&cache_path).expect("read cache after render"),
            before,
            "S-02 case {label} leaves cache bytes unchanged"
        );
        assert!(
            !usage_dir.join("refresh.lock").exists(),
            "S-02 case {label} does not claim refresh"
        );
    }
}
