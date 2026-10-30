//! RED coverage for the `ctx` split: `ctxGauge` (gauge half, a gauge segment)
//! and `ctxTokens` (token tail, dim, not a gauge). `ctx` itself is unchanged.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{
    config::model::Config,
    render::{is_gauge, themes},
    tui::{
        app::{AppState, ScreenId, SegmentPickerTarget, UiMode},
        draft_ops::effective_segment_fg,
        screens::{RowsSegmentsScreen, Screen, colors_separators},
    },
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const THEMES: [&str; 3] = ["matrix-tron", "solarized-dark", "solarized-light"];
const TOKENS: &str = "↑20.0k ↓1.2k cr:18.5k cw:900";
const USER: &str = "#123456";

static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "ctx_split_{}_{}",
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

/// Sample stdin with all four token fields present.
fn stdin(pct: i64) -> Value {
    json!({
        "session_id": "ctx-split",
        "cwd": "/tmp/ctx-split",
        "model": {"display_name": "Fable"},
        "context_window": {
            "used_percentage": pct,
            "total_input_tokens": 20_000,
            "total_output_tokens": 1_200,
            "current_usage": {
                "cache_read_input_tokens": 18_500,
                "cache_creation_input_tokens": 900
            }
        }
    })
}

/// Defaults minus `segments` and the gauge colour keys, themed, truecolor.
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

fn render_stdin(config: &Config, segments: &[&str], raw: Value) -> String {
    let mut value = Value::Object(config.0.clone());
    value["rows"] = json!([{"layout": "fixed", "segments": segments}]);
    phosphorpulse::render::render_value(raw, value)
}

fn render(config: &Config, segments: &[&str], pct: i64) -> String {
    render_stdin(config, segments, stdin(pct))
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for n in chars.by_ref() {
                if n == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// First truecolor foreground in `raw`, as upper-case hex.
fn first_fg(raw: &str) -> Option<String> {
    let start = raw.find("\x1b[38;2;")? + "\x1b[38;2;".len();
    let end = start + raw[start..].find('m')?;
    let rgb: Vec<u8> = raw[start..end].split(';').filter_map(|n| n.parse().ok()).collect();
    (rgb.len() == 3).then(|| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

/// Foreground of each styled piece, in order (pieces end at `ESC[0m`).
fn piece_fgs(raw: &str) -> Vec<Option<String>> {
    raw.split("\x1b[0m")
        .filter(|chunk| chunk.contains('\x1b') || !chunk.trim().is_empty())
        .map(first_fg)
        .collect()
}

fn palette(theme: &str, role: &str) -> String {
    themes::builtin(theme).palette[role].to_uppercase()
}

/// ctx gauge text for 42% at the default bar width of 20 (round(8.4) = 8).
fn gauge_text(pct: i64) -> String {
    let n = (pct as f64 * 20.0 / 100.0).round() as usize;
    format!("⬡ {}{} {pct}%", "▰".repeat(n), "▱".repeat(20 - n))
}

#[test]
fn ctx_split_01_gauge_text_and_colour_per_band() {
    let _env = EnvGuard::new();
    let cfg = base("matrix-tron");
    for (pct, role) in [(42, "ctx.ok"), (70, "warn"), (90, "hot")] {
        let raw = render(&cfg, &["ctxGauge"], pct);
        assert_eq!(strip_ansi(&raw), format!(" {} ", gauge_text(pct)), "text at {pct}%");
        assert_eq!(first_fg(&raw), Some(palette("matrix-tron", role)), "fg at {pct}%");
    }
}

#[test]
fn ctx_split_02_tokens_text_and_dim_colour_in_all_bands() {
    let _env = EnvGuard::new();
    for theme in THEMES {
        let cfg = base(theme);
        for pct in [42, 70, 90] {
            let raw = render(&cfg, &["ctxTokens"], pct);
            assert_eq!(strip_ansi(&raw), format!(" {TOKENS} "), "{theme} text at {pct}%");
            assert_eq!(
                first_fg(&raw),
                Some(palette(theme, "text.dim")),
                "{theme} fg at {pct}% (never warn/hot)"
            );
        }
    }
}

#[test]
fn ctx_split_03_tokens_hidden_when_a_field_is_missing() {
    let _env = EnvGuard::new();
    let cfg = base("matrix-tron");
    let shown = strip_ansi(&render(&cfg, &["ctxTokens"], 42));
    assert!(shown.contains(TOKENS), "all fields present must show tokens: {shown:?}");
    let removals: [&[&str]; 4] = [
        &["context_window", "total_input_tokens"],
        &["context_window", "total_output_tokens"],
        &["context_window", "current_usage", "cache_read_input_tokens"],
        &["context_window", "current_usage", "cache_creation_input_tokens"],
    ];
    for path in removals {
        let mut raw = stdin(42);
        let (last, parents) = path.split_last().unwrap();
        let mut node = &mut raw;
        for key in parents {
            node = &mut node[*key];
        }
        node.as_object_mut().unwrap().remove(*last);
        let out = strip_ansi(&render_stdin(&cfg, &["ctxTokens"], raw));
        assert!(out.trim().is_empty(), "missing {last} must hide ctxTokens, got {out:?}");
    }
}

/// The renderer wraps every segment as `" {text} "` and joins with an empty
/// separator, so ["ctxGauge","ctxTokens"] is ` <gauge> ` + ` <tokens> `, while
/// ["ctx"] is ` <gauge> <tokens> ` (single space before the tail). The spec
/// says the split pieces carry no leading space of their own, so the only
/// difference is the extra padding space at the seam: two spaces, not one.
#[test]
fn ctx_split_04_plain_text_matches_ctx_plus_seam_padding() {
    let _env = EnvGuard::new();
    let cfg = base("matrix-tron");
    for pct in [42, 90] {
        let ctx = strip_ansi(&render(&cfg, &["ctx"], pct));
        let split = strip_ansi(&render(&cfg, &["ctxGauge", "ctxTokens"], pct));
        assert_eq!(ctx, format!(" {} {TOKENS} ", gauge_text(pct)), "ctx baseline at {pct}%");
        assert_eq!(split, ctx.replacen(" ↑", "  ↑", 1), "split at {pct}%");
    }
}

#[test]
fn ctx_split_05_tokens_fg_colours_only_the_token_piece() {
    let _env = EnvGuard::new();
    let mut cfg = base("matrix-tron");
    cfg.0.insert("segments".into(), json!({"ctxTokens": {"fg": "#ff0000"}}));
    let raw = render(&cfg, &["ctxGauge", "ctxTokens"], 42);
    assert_eq!(
        piece_fgs(&raw),
        vec![Some(palette("matrix-tron", "ctx.ok")), Some("#FF0000".into())],
        "raw: {raw:?}"
    );
}

#[test]
fn ctx_split_06_gauge_warn_color_hits_gauge_not_tokens() {
    let _env = EnvGuard::new();
    let mut cfg = base("matrix-tron");
    cfg.0.insert("gauge".into(), json!({"warnColor": USER}));
    let raw = render(&cfg, &["ctxGauge", "ctxTokens"], 70);
    assert_eq!(
        piece_fgs(&raw),
        vec![Some(USER.to_uppercase()), Some(palette("matrix-tron", "text.dim"))],
        "raw: {raw:?}"
    );
}

#[test]
fn ctx_split_07_gauge_flag_and_builtin_id_order() {
    assert!(is_gauge("ctxGauge"), "ctxGauge is a gauge");
    assert!(!is_gauge("ctxTokens"), "ctxTokens is not a gauge");

    // `builtin_main_segment_ids` is private; observe it through public surfaces.
    // Colour screen: separator 0 is `1 + non-flex builtin count` (19 -> 21 ids).
    let draft = Config::defaults();
    assert_eq!(colors_separators(&draft)[0], 22, "two new colour rows");

    // Picker lists builtins in order, so indices 20 and 21 are the appended ids.
    for (index, id) in [(20, "ctxGauge"), (21, "ctxTokens")] {
        let mut state = AppState::new(Config::defaults(), ScreenId::RowsSegments);
        state.mode = UiMode::SegmentPicker {
            target: SegmentPickerTarget::Main,
            insert_after: false,
            position: 0,
        };
        state.set_selected(index);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal
            .draw(|frame| RowsSegmentsScreen::new().draw(frame, &state))
            .expect("draw screen");
        let text: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains(&format!("> {id}")), "picker index {index} must be {id}");
    }
}

#[test]
fn ctx_split_08_swatch_default_equals_rendered_fg_per_theme() {
    let _env = EnvGuard::new();
    for theme in THEMES {
        let draft = base(theme);
        for id in ["ctxGauge", "ctxTokens"] {
            let rendered = first_fg(&render(&draft, &[id], 42));
            assert!(rendered.is_some(), "{theme}: {id} must render a coloured piece");
            assert_eq!(
                Some(effective_segment_fg(&draft, id).to_uppercase()),
                rendered,
                "theme {theme}, segment {id}"
            );
        }
        assert_eq!(effective_segment_fg(&draft, "ctxGauge").to_uppercase(), palette(theme, "ctx.ok"));
        assert_eq!(effective_segment_fg(&draft, "ctxTokens").to_uppercase(), palette(theme, "text.dim"));
    }
}

