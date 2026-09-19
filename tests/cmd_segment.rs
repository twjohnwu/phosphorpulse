use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::config::model::Config;
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "cmd_segment_{label}_{}_{}",
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
    ppulse_config_dir: Option<OsString>,
    ppulse_now_ms: Option<OsString>,
    columns: Option<OsString>,
}

impl EnvGuard {
    fn new(config_dir: &Path) -> Self {
        let guard = Self {
            ppulse_config_dir: std::env::var_os("PPULSE_CONFIG_DIR"),
            ppulse_now_ms: std::env::var_os("PPULSE_NOW_MS"),
            columns: std::env::var_os("COLUMNS"),
        };
        unsafe {
            std::env::set_var("PPULSE_CONFIG_DIR", config_dir);
            std::env::set_var("PPULSE_NOW_MS", NOW.to_string());
            std::env::set_var("COLUMNS", "120");
        }
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            restore_env("PPULSE_CONFIG_DIR", &self.ppulse_config_dir);
            restore_env("PPULSE_NOW_MS", &self.ppulse_now_ms);
            restore_env("COLUMNS", &self.columns);
        }
    }
}

unsafe fn restore_env(name: &str, value: &Option<OsString>) {
    match value {
        Some(value) => unsafe { std::env::set_var(name, value) },
        None => unsafe { std::env::remove_var(name) },
    }
}

fn command_config(segment: &str, preserve_colors: bool) -> Value {
    let mut config = Value::Object(Config::defaults().0);
    config["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    config["commands"] = json!({
        "k8s": {
            "command": "true",
            "ttlSec": 5,
            "maxWidth": 12
        }
    });
    if preserve_colors {
        config["commands"]["k8s"]["preserveColors"] = json!(true);
    }
    config["segments"] = json!({"cmd:k8s": {"fg": "#00CDCD"}});
    config
}

fn stdin_value() -> Value {
    json!({"cwd": "/tmp/proj"})
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

fn color_depth_from_oracle() -> &'static str {
    let mut stdin = stdin_value();
    stdin["rate_limits"] = json!({"seven_day": {"used_percentage": 65}});
    let mut config = Value::Object(Config::defaults().0);
    config["rows"] = json!([{"layout": "fixed", "segments": ["limit7d"]}]);
    let raw = phosphorpulse::render::render_value(stdin, config);
    let sgr = first_fg_sgr(&raw).expect("limit7d oracle has a foreground SGR");
    if sgr.starts_with("\x1b[38;2;") {
        "truecolor"
    } else if sgr.starts_with("\x1b[38;5;") {
        "256color"
    } else {
        "16color"
    }
}

fn write_cache(config_dir: &Path, cwd: &str, fetched_at: i64, output: &str) {
    let commands_dir = config_dir.join("commands");
    fs::create_dir_all(&commands_dir).expect("create commands directory");
    let key = phosphorpulse::cmd::cache_key("k8s", Some(cwd));
    let bytes = serde_json::to_vec(&json!({
        "fetchedAt": fetched_at,
        "nextFetchAt": NOW + 4_000,
        "command": "true",
        "output": output
    }))
    .expect("serialize command cache fixture");
    fs::write(commands_dir.join(format!("{key}.json")), bytes)
        .expect("write command cache fixture");
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("read commands directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read commands directory entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if entry.file_type().expect("read entry type").is_dir() {
                visit(base, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(base)
                        .expect("file is beneath commands directory")
                        .to_path_buf(),
                    fs::read(&path).expect("snapshot command cache file"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

/// REQ-03 / S-03: command segments render cached display and freshness rules read-only.
#[test]
fn test_s03_display_rules() {
    struct Case {
        label: &'static str,
        segment: &'static str,
        fetched_at: i64,
        output: &'static str,
        preserve_colors: bool,
        expected_text: &'static str,
    }

    let cases = [
        Case {
            label: "a",
            segment: "cmd:k8s",
            fetched_at: NOW - 1_000,
            output: "prod-cluster",
            preserve_colors: false,
            expected_text: "prod-cluster",
        },
        Case {
            label: "b",
            segment: "cmd:k8s",
            fetched_at: NOW - 1_000,
            output: "\x1b[31mprod-cluster-east\x1b[0m",
            preserve_colors: false,
            expected_text: "prod-cluste…",
        },
        Case {
            label: "c",
            segment: "cmd:k8s",
            fetched_at: NOW - 1_000,
            output: "\x1b[31mprod-cluster-east\x1b[0m",
            preserve_colors: true,
            expected_text: "prod-cluste…",
        },
        Case {
            label: "d",
            segment: "cmd:k8s",
            fetched_at: NOW - 120_000,
            output: "prod",
            preserve_colors: false,
            expected_text: "prod",
        },
        Case {
            label: "e",
            segment: "cmd:k8s",
            fetched_at: NOW - 90_000_000,
            output: "prod",
            preserve_colors: false,
            expected_text: "--",
        },
        Case {
            label: "f",
            segment: "cmd:k8s",
            fetched_at: NOW - 1_000,
            output: "",
            preserve_colors: false,
            expected_text: "--",
        },
        Case {
            label: "g",
            segment: "cmd:k8s",
            fetched_at: NOW - 1_000,
            output: "proj",
            preserve_colors: false,
            expected_text: "proj",
        },
        Case {
            label: "h",
            segment: "cmd:nope",
            fetched_at: NOW - 1_000,
            output: "prod",
            preserve_colors: false,
            expected_text: "--",
        },
    ];

    for case in cases {
        let temp = TempDir::new(case.label);
        let _env = EnvGuard::new(temp.path());
        let commands_dir = temp.path().join("commands");
        write_cache(temp.path(), "/tmp/proj", case.fetched_at, case.output);
        if case.label == "g" {
            write_cache(temp.path(), "/tmp/other", NOW - 1_000, "other");
        }
        let before = snapshot_files(&commands_dir);
        let depth = color_depth_from_oracle();
        let expected_custom_fg = phosphorpulse::render::color("#00CDCD", depth, false);
        let expected_dim_fg = phosphorpulse::render::color("#008F11", depth, false);

        let raw = phosphorpulse::render::render_value(
            stdin_value(),
            command_config(case.segment, case.preserve_colors),
        );
        let after = snapshot_files(&commands_dir);
        assert_eq!(
            after, before,
            "S-03 case {} leaves every command cache byte unchanged and creates no files",
            case.label
        );

        let plain = strip_ansi(&raw);
        assert_eq!(plain.trim(), case.expected_text, "S-03 case {}", case.label);
        match case.label {
            "a" => assert_eq!(
                first_fg_sgr(&raw),
                Some(expected_custom_fg.as_str()),
                "S-03 case a configured foreground"
            ),
            "b" => assert!(
                !raw.contains("\x1b[31m"),
                "S-03 case b strips cached ANSI color"
            ),
            "c" => assert!(
                raw.contains("\x1b[31m"),
                "S-03 case c preserves cached ANSI color"
            ),
            "d" => assert_eq!(
                first_fg_sgr(&raw),
                Some(expected_dim_fg.as_str()),
                "S-03 case d uses palette text.dim foreground"
            ),
            _ => {}
        }
    }
}
