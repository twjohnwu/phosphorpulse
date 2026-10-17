//! RED coverage for the v0.3.0 swatch gaps: TUI swatches must honour
//! `gauge.okColor`, and `validate_renderable` must reject a non-object
//! `subagent.colors`.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{
    config::model::Config,
    tui::draft_ops::{effective_segment_fg, effective_subagent_fg},
};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const OK: &str = "#123456";
const GAUGE_IDS: [&str; 5] = ["limit5h", "limit7d", "limitModel", "ctx", "cache"];
/// Gauge ids that render with sample stdin in a bare temp dir (limitModel and
/// cache need a usage cache / live countdown), so only these are render-checked.
const RENDERABLE_GAUGES: [&str; 3] = ["limit5h", "limit7d", "ctx"];

static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "swatch_gauge_override_{}_{}",
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
    _lock: MutexGuard<'static, ()>,
    _temp: TempDir,
}

impl EnvGuard {
    fn new() -> Self {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = TempDir::new();
        let names = ["HOME", "PPULSE_CONFIG_DIR", "PPULSE_NOW_MS", "COLUMNS"];
        let vars = names.iter().map(|n| (*n, std::env::var_os(n))).collect();
        unsafe {
            std::env::set_var("HOME", temp.path());
            std::env::set_var("PPULSE_CONFIG_DIR", temp.path());
            std::env::set_var("PPULSE_NOW_MS", NOW.to_string());
            std::env::set_var("COLUMNS", "160");
        }
        Self { vars, _lock: lock, _temp: temp }
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

fn stdin() -> Value {
    json!({
        "session_id": "swatch-gauge",
        "cwd": "/tmp/swatch-gauge",
        "model": {"display_name": "Fable"},
        "context_window": {"used_percentage": 10},
        "rate_limits": {
            "five_hour": {"used_percentage": 10, "resets_at": (NOW + 3_600_000) / 1000},
            "seven_day": {"used_percentage": 10, "resets_at": (NOW + 3_600_000) / 1000}
        }
    })
}

/// Defaults minus `segments` and gauge colour keys, truecolor, matrix-tron,
/// then `gauge.okColor` set to `OK`.
fn with_ok_color() -> Config {
    let mut config = Config::defaults();
    config.0.remove("segments");
    config.0.insert("activeTemplate".into(), json!("matrix-tron"));
    config.0.insert("colorDepth".into(), json!("truecolor"));
    config.0.insert("gauge".into(), json!({"okColor": OK}));
    config
}

fn render(config: &Config, segment: &str) -> String {
    let mut value = Value::Object(config.0.clone());
    value["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    phosphorpulse::render::render_value(stdin(), value)
}

/// First truecolor foreground (`ESC[38;2;R;G;Bm`) in `raw`, as upper-case hex.
fn first_fg(raw: &str) -> Option<String> {
    let start = raw.find("\x1b[38;2;")? + "\x1b[38;2;".len();
    let end = start + raw[start..].find('m')?;
    let rgb: Vec<u8> = raw[start..end].split(';').filter_map(|n| n.parse().ok()).collect();
    (rgb.len() == 3).then(|| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

/// RED: every gauge swatch follows `gauge.okColor`, and matches the renderer.
#[test]
fn test_d01_gauge_swatch_follows_ok_color() {
    let _env = EnvGuard::new();
    let draft = with_ok_color();
    for id in GAUGE_IDS {
        assert_eq!(
            effective_segment_fg(&draft, id).to_uppercase(),
            OK.to_uppercase(),
            "swatch for {id}"
        );
    }
    for id in RENDERABLE_GAUGES {
        let emitted = first_fg(&render(&draft, id)).unwrap_or_else(|| panic!("no fg for {id}"));
        assert_eq!(emitted, OK.to_uppercase(), "renderer for {id}");
        assert_eq!(effective_segment_fg(&draft, id).to_uppercase(), emitted, "swatch vs renderer {id}");
    }
}

/// Guard (green today): a per-segment fg beats `gauge.okColor`.
#[test]
fn test_d02_segment_fg_beats_ok_color_in_swatch() {
    let mut draft = with_ok_color();
    draft.0.insert("segments".into(), json!({"limit5h": {"fg": "#654321"}}));
    assert_eq!(effective_segment_fg(&draft, "limit5h").to_uppercase(), "#654321");
}

/// Guard (green today): non-gauge swatches ignore `gauge.okColor`.
#[test]
fn test_d03_non_gauge_swatch_unchanged_by_ok_color() {
    let draft = with_ok_color();
    let mut plain = draft.clone();
    plain.0.remove("gauge");
    for id in ["model", "fastMode", "dir", "cost"] {
        assert_eq!(effective_segment_fg(&draft, id), effective_segment_fg(&plain, id), "segment {id}");
    }
}

/// RED: subagent ctx swatch follows `gauge.okColor`; its own fg wins when set.
#[test]
fn test_d04_subagent_ctx_swatch_follows_ok_color() {
    let draft = with_ok_color();
    assert_eq!(effective_subagent_fg(&draft, "ctx").to_uppercase(), OK.to_uppercase());

    let mut set = draft.clone();
    set.0.insert("subagent".into(), json!({"colors": {"ctx": {"fg": "#654321"}}}));
    assert_eq!(effective_subagent_fg(&set, "ctx").to_uppercase(), "#654321");
}

/// RED: a non-object `subagent.colors` is rejected, naming `/subagent/colors`.
#[test]
fn test_d05_validate_rejects_non_object_subagent_colors() {
    let mut config = Config::defaults();
    config.0.insert("subagent".into(), json!({"colors": "red"}));
    let err = config
        .validate_renderable()
        .expect_err("non-object subagent.colors must be rejected");
    assert!(err.contains("/subagent/colors"), "error must name /subagent/colors: {err}");
}
