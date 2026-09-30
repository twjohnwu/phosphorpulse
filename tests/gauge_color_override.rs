use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{config::model::Config, render::color};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const CONFIGURED: &str = "#123456";
const HOT: &str = "#FF3737";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "gauge_color_override_{}_{}",
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
    vars: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn new(root: &Path) -> Self {
        let names = ["HOME", "PPULSE_CONFIG_DIR", "PPULSE_NOW_MS", "COLUMNS"];
        let vars = names.iter().map(|n| (*n, std::env::var_os(n))).collect();
        unsafe {
            std::env::set_var("HOME", root);
            std::env::set_var("PPULSE_CONFIG_DIR", root);
            std::env::set_var("PPULSE_NOW_MS", NOW.to_string());
            std::env::set_var("COLUMNS", "120");
        }
        Self { vars }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            for (name, value) in &self.vars {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn config_with_fg(segment: &str, fg: Option<&str>) -> Value {
    let mut config = Value::Object(Config::defaults().0);
    config["colorDepth"] = json!("truecolor");
    config["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    if let Some(fg) = fg {
        config["segments"] = json!({ segment: {"fg": fg} });
    }
    config
}

fn stdin() -> Value {
    json!({
        "session_id": "gauge-color-override",
        "cwd": "/tmp/gauge-color-override",
        "model": {"display_name": "Fable"},
        "context_window": {"used_percentage": 42},
        "cost": {"total_cost_usd": 1.5},
        "rate_limits": {
            "five_hour": {"used_percentage": 10, "resets_at": (NOW + 3_600_000) / 1000},
            "seven_day": {"used_percentage": 90, "resets_at": (NOW + 3_600_000) / 1000}
        }
    })
}

fn render(segment: &str, fg: Option<&str>) -> String {
    phosphorpulse::render::render_value(stdin(), config_with_fg(segment, fg))
}

/// regression: configured segment fg must not mask gauge warn/hot/dim
#[test]
fn test_gauge_threshold_beats_configured_fg() {
    let temp = TempDir::new();
    let _env = EnvGuard::new(temp.path());
    let configured = color(CONFIGURED, "truecolor", false);
    let hot = color(HOT, "truecolor", false);

    let raw = render("limit7d", Some(CONFIGURED));
    assert!(raw.contains(&hot), "limit7d at 90% must use hot: {raw:?}");
    assert!(!raw.contains(&configured), "limit7d hot masked: {raw:?}");

    let raw = render("limit5h", Some(CONFIGURED));
    assert!(raw.contains(&configured), "limit5h ok uses configured: {raw:?}");

    let raw = render("cost", Some(CONFIGURED));
    assert!(raw.contains(&configured), "cost uses configured: {raw:?}");

    let usage_dir = temp.path().join("usage");
    fs::create_dir_all(&usage_dir).expect("create usage directory");
    let cache = json!({
        "fetchedAt": NOW - 10_000,
        "nextFetchAt": NOW + 290_000,
        "limits": [{"displayName": "Fable", "percent": 100, "resetsAt": null, "isActive": true}]
    });
    fs::write(usage_dir.join("cache.json"), serde_json::to_vec(&cache).unwrap())
        .expect("write cache fixture");
    let raw = render("limitModel", Some(CONFIGURED));
    assert!(raw.contains(&hot), "limitModel at 100% must use hot: {raw:?}");
    assert!(!raw.contains(&configured), "limitModel hot masked: {raw:?}");
}
