//! RED coverage for v0.3.0 batch 1: TUI swatches follow the active theme and
//! `gauge.{ok,warn,hot}Color` reach the real render.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{
    config::model::Config,
    render::themes,
    tui::draft_ops::{effective_gauge_color, effective_segment_fg},
};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const USER: &str = "#123456";
const THEMES: [&str; 3] = ["matrix-tron", "solarized-dark", "solarized-light"];
// `builtin_main_segment_ids()` (editor.rs:51) minus `flex`.
const SEGMENT_IDS: [&str; 19] = [
    "model", "effort", "git", "dir", "ctx", "limit5h", "limit7d", "node", "python", "version",
    "cost", "burn", "pomodoro", "session", "fastMode", "outputStyle", "thinking", "limitModel",
    "cache",
];
/// Segments that cannot be made visible with sample stdin in a bare temp dir
/// (git needs a repo, node/python need project files, pomodoro needs gate
/// state, limitModel needs a usage cache, cache needs a live prompt-cache
/// countdown). Compared against the palette role they emit instead.
const PALETTE_ROLE: [(&str, &str); 6] = [
    ("git", "git.ok"),
    ("node", "node"),
    ("python", "python"),
    ("pomodoro", "pomodoro.work"),
    ("limitModel", "ok"),
    ("cache", "ok"),
];

static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "theme_colors_{}_{}",
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

fn stdin(five_hour: i64) -> Value {
    json!({
        "session_id": "theme-colors",
        "session_name": "work",
        "cwd": "/tmp/theme-colors",
        "version": "1.2.3",
        "model": {"display_name": "Fable"},
        "effort": {"level": "high"},
        "fast_mode": true,
        "thinking": {"enabled": false},
        "output_style": {"name": "explanatory"},
        "context_window": {"used_percentage": 42},
        "cost": {"total_cost_usd": 1.5, "total_duration_ms": 600_000},
        "rate_limits": {
            "five_hour": {"used_percentage": five_hour, "resets_at": (NOW + 3_600_000) / 1000},
            "seven_day": {"used_percentage": 10, "resets_at": (NOW + 3_600_000) / 1000}
        }
    })
}

/// A draft/config with no user colours: defaults minus `segments` and the
/// gauge colour keys, themed via `activeTemplate`.
fn base(theme: &str) -> Config {
    let mut config = Config::defaults();
    config.0.remove("segments");
    if let Some(gauge) = config.0.get_mut("gauge").and_then(Value::as_object_mut) {
        for key in ["okColor", "warnColor", "hotColor"] {
            gauge.remove(key);
        }
    }
    config.0.insert("activeTemplate".into(), json!(theme));
    config.0.insert("colorDepth".into(), json!("truecolor"));
    config
}

fn render(config: &Config, segment: &str, five_hour: i64) -> String {
    let mut value = Value::Object(config.0.clone());
    value["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    phosphorpulse::render::render_value(stdin(five_hour), value)
}

/// First truecolor foreground (`ESC[38;2;R;G;Bm`) in `raw`, as upper-case hex.
fn first_fg(raw: &str) -> Option<String> {
    let start = raw.find("\x1b[38;2;")? + "\x1b[38;2;".len();
    let end = start + raw[start..].find('m')?;
    let rgb: Vec<u8> = raw[start..end].split(';').filter_map(|n| n.parse().ok()).collect();
    (rgb.len() == 3).then(|| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

fn palette(theme: &str, role: &str) -> String {
    themes::builtin(theme).palette[role].to_uppercase()
}

fn fg_of(raw: &str) -> String {
    first_fg(raw).unwrap_or_else(|| panic!("no truecolor fg in {raw:?}"))
}

/// A: swatches equal what the renderer emits under each built-in theme.
#[test]
fn test_b01_segment_swatch_matches_renderer_per_theme() {
    let _env = EnvGuard::new();
    for theme in THEMES {
        let draft = base(theme);
        for id in SEGMENT_IDS {
            let expected = match PALETTE_ROLE.iter().find(|(seg, _)| *seg == id) {
                Some((_, role)) => palette(theme, role),
                None => fg_of(&render(&draft, id, 10)),
            };
            assert_eq!(
                effective_segment_fg(&draft, id).to_uppercase(),
                expected,
                "theme {theme}, segment {id}"
            );
        }
    }
}

/// A: gauge colour defaults equal the theme palette.
#[test]
fn test_b02_gauge_color_defaults_follow_theme() {
    for theme in THEMES {
        let draft = base(theme);
        for state in ["ok", "warn", "hot"] {
            assert_eq!(
                effective_gauge_color(&draft, state).to_uppercase(),
                palette(theme, state),
                "theme {theme}, gauge {state}"
            );
        }
    }
}

/// A: an unknown (user template) name falls back to matrix-tron values.
/// Guard: already green today because the hardcoded table is matrix-tron; the
/// theme-aware implementation must keep it so. The solarized probe below is
/// what makes this test RED now.
#[test]
fn test_b03_user_template_name_uses_matrix_tron() {
    let solarized = base("solarized-dark");
    assert_ne!(
        effective_segment_fg(&solarized, "dir").to_uppercase(),
        palette("matrix-tron", "dir"),
        "solarized-dark swatch must differ from matrix-tron"
    );
    let user = base("my-custom-template");
    for id in SEGMENT_IDS {
        let matrix = effective_segment_fg(&base("matrix-tron"), id);
        assert_eq!(effective_segment_fg(&user, id), matrix, "segment {id}");
    }
    for state in ["ok", "warn", "hot"] {
        assert_eq!(
            effective_gauge_color(&user, state).to_uppercase(),
            palette("matrix-tron", state),
            "gauge {state}"
        );
    }
}

/// A2: warnColor reaches limit5h in its warn range.
#[test]
fn test_b04_warn_color_reaches_render() {
    let _env = EnvGuard::new();
    let mut config = base("matrix-tron");
    config.0.insert("gauge".into(), json!({"warnColor": USER}));
    assert_eq!(fg_of(&render(&config, "limit5h", 70)), USER);
}

/// A2: hotColor reaches limit5h in its hot range.
#[test]
fn test_b05_hot_color_reaches_render() {
    let _env = EnvGuard::new();
    let mut config = base("matrix-tron");
    config.0.insert("gauge".into(), json!({"hotColor": USER}));
    assert_eq!(fg_of(&render(&config, "limit5h", 90)), USER);
}

/// A2: okColor applies to the ok range of both ok and ctx.ok gauge roles.
#[test]
fn test_b06_ok_color_applies_to_ok_and_ctx_ok_gauges() {
    let _env = EnvGuard::new();
    let mut config = base("matrix-tron");
    config.0.insert("gauge".into(), json!({"okColor": USER}));
    assert_eq!(fg_of(&render(&config, "limit5h", 10)), USER, "ok role");
    assert_eq!(fg_of(&render(&config, "ctx", 10)), USER, "ctx.ok role");
}

/// A2: a per-segment fg beats okColor in the ok range.
#[test]
fn test_b07_segment_fg_beats_ok_color() {
    let _env = EnvGuard::new();
    let mut config = base("matrix-tron");
    config.0.insert("gauge".into(), json!({"okColor": USER}));
    config.0.insert("segments".into(), json!({"limit5h": {"fg": "#654321"}}));
    // okColor must be live first (RED today), otherwise the precedence check
    // below would be vacuous.
    assert_eq!(fg_of(&render(&config, "ctx", 10)), USER, "okColor live on ctx");
    assert_eq!(fg_of(&render(&config, "limit5h", 10)), "#654321");
}

/// A2: okColor leaves non-gauge segments alone.
#[test]
fn test_b08_ok_color_does_not_touch_non_gauge_segments() {
    let _env = EnvGuard::new();
    let plain = base("matrix-tron");
    let mut config = plain.clone();
    config.0.insert("gauge".into(), json!({"okColor": USER}));
    // okColor must be live first (RED today), so the unchanged checks are not vacuous.
    assert_eq!(fg_of(&render(&config, "limit5h", 10)), USER, "okColor live on gauge");
    for id in ["dir", "model", "cost", "version", "effort"] {
        assert_eq!(render(&config, id, 10), render(&plain, id, 10), "segment {id}");
    }
}

/// A2: validation rejects a non-hex gauge colour and names the key.
#[test]
fn test_b09_validate_rejects_named_gauge_color() {
    for key in ["okColor", "warnColor", "hotColor"] {
        let mut config = base("matrix-tron");
        config.0.insert("gauge".into(), json!({ key: "red" }));
        let err = config
            .validate_renderable()
            .expect_err(&format!("gauge.{key} = \"red\" must be rejected"));
        assert!(err.contains(key), "error must name {key}: {err}");
    }
}
