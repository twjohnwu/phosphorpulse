//! RED coverage for v0.3.0 batch 2: the subagent line has its own colours
//! (`subagent.colors.<id>.fg|bg`) and no longer reads `segments.<id>`.

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
    tui::{
        draft_ops::{cycle_subagent_fg, effective_subagent_fg},
        screens::colors_separators,
    },
};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;
const USER: &str = "#123456";
const SUB: &str = "#654321";
const THEMES: [&str; 3] = ["matrix-tron", "solarized-dark", "solarized-light"];
// `SUBAGENT_SEGMENT_IDS` (editor.rs:76).
const SUBAGENT_IDS: [&str; 8] =
    ["name", "desc", "model", "ctx", "elapsed", "effort", "tokenCount", "activity"];
/// Ids the sample task cannot make visible (`desc` needs a resolved role
/// identity, `activity` needs a sparkline); compared against the palette role.
const PALETTE_ROLE: [(&str, &str); 2] = [("desc", "text.dim"), ("activity", "text.dim")];

static LOCK: Mutex<()> = Mutex::new(());

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "subagent_colors_{}_{}",
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
        "session_id": "subagent-colors",
        "cwd": "/tmp/subagent-colors",
        "model": {"display_name": "Fable"},
        "effort": {"level": "high"},
        "context_window": {"used_percentage": 42}
    })
}

fn tasks() -> Value {
    json!({"tasks": [{
        "id": "t1", "label": "worker", "model": "claude-sonnet-5",
        "contextWindowSize": 200_000, "tokenCount": 50_000,
        "startTime": NOW - 130_000, "effort": "medium"
    }]})
}

/// Defaults minus `segments` and gauge colours, themed via `activeTemplate`.
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

fn with(config: &Config, path: &[&str], value: Value) -> Config {
    let mut next = config.clone();
    let mut node = &mut next.0;
    for key in &path[..path.len() - 1] {
        node = node
            .entry(*key)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("object");
    }
    node.insert(path[path.len() - 1].into(), value);
    next
}

/// Subagent line content (the `content` field of the first JSONL record) with
/// only `id` shown.
fn render_sub(config: &Config, id: &str) -> String {
    let cfg = with(config, &["subagent", "segments"], json!([id]));
    let raw = phosphorpulse::render::render_subagent_value(tasks(), Value::Object(cfg.0));
    let line = raw.lines().next().unwrap_or_else(|| panic!("no subagent output for {id}"));
    let record: Value = serde_json::from_str(line).expect("jsonl record");
    record["content"].as_str().expect("content string").to_owned()
}

fn render_main(config: &Config, id: &str) -> String {
    let mut value = Value::Object(config.0.clone());
    value["rows"] = json!([{"layout": "fixed", "segments": [id]}]);
    phosphorpulse::render::render_value(stdin(), value)
}

fn first_color(raw: &str, lead: &str) -> Option<String> {
    let start = raw.find(lead)? + lead.len();
    let end = start + raw[start..].find('m')?;
    let rgb: Vec<u8> = raw[start..end].split(';').filter_map(|n| n.parse().ok()).collect();
    (rgb.len() == 3).then(|| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

fn fg_of(raw: &str) -> String {
    first_color(raw, "\x1b[38;2;").unwrap_or_else(|| panic!("no truecolor fg in {raw:?}"))
}

fn palette(theme: &str, role: &str) -> String {
    themes::builtin(theme).palette[role].to_uppercase()
}

/// 1: `segments.model.fg` no longer reaches the subagent model; it renders the
/// theme `sub.model` colour. The main-line model still renders it.
#[test]
fn test_c01_segments_fg_does_not_reach_subagent_line() {
    let _env = EnvGuard::new();
    let config = with(&base("matrix-tron"), &["segments", "model", "fg"], json!(USER));
    assert_eq!(fg_of(&render_main(&config, "model")), USER, "main line keeps segments.model.fg");
    assert_eq!(fg_of(&render_sub(&config, "model")), palette("matrix-tron", "sub.model"));
}

/// 2: `subagent.colors.model.fg` recolours only the subagent model.
#[test]
fn test_c02_subagent_fg_applies_only_to_subagent_line() {
    let _env = EnvGuard::new();
    let plain = base("matrix-tron");
    let config = with(&plain, &["subagent", "colors", "model", "fg"], json!(SUB));
    assert_eq!(fg_of(&render_sub(&config, "model")), SUB);
    assert_eq!(render_main(&config, "model"), render_main(&plain, "model"));
}

/// 3: `subagent.colors.<id>.bg` is applied to the subagent segment.
#[test]
fn test_c03_subagent_bg_applies() {
    let _env = EnvGuard::new();
    let config = with(&base("matrix-tron"), &["subagent", "colors", "model", "bg"], json!(SUB));
    assert_eq!(
        first_color(&render_sub(&config, "model"), "\x1b[48;2;").as_deref(),
        Some(SUB)
    );
}

/// 4: ctx gauge, ok range: subagent.colors.ctx.fg > gauge.okColor > theme;
/// `segments.ctx.fg` is ignored.
#[test]
fn test_c04_subagent_ctx_precedence() {
    let _env = EnvGuard::new();
    let theme = base("matrix-tron");
    assert_eq!(fg_of(&render_sub(&theme, "ctx")), palette("matrix-tron", "ok"), "theme");
    let ok = with(&theme, &["gauge", "okColor"], json!(USER));
    assert_eq!(fg_of(&render_sub(&ok, "ctx")), USER, "okColor beats theme");
    let sub = with(&ok, &["subagent", "colors", "ctx", "fg"], json!(SUB));
    assert_eq!(fg_of(&render_sub(&sub, "ctx")), SUB, "subagent colour beats okColor");
    let seg = with(&ok, &["segments", "ctx", "fg"], json!("#ABCDEF"));
    assert_eq!(fg_of(&render_sub(&seg, "ctx")), USER, "segments.ctx.fg ignored");
}

/// 5: validation rejects a non-hex subagent colour and names the path.
#[test]
fn test_c05_validate_rejects_named_subagent_color() {
    for key in ["fg", "bg"] {
        let config = with(&base("matrix-tron"), &["subagent", "colors", "model", key], json!("red"));
        let err = config
            .validate_renderable()
            .expect_err(&format!("subagent.colors.model.{key} = \"red\" must be rejected"));
        assert!(
            err.contains(&format!("/subagent/colors/model/{key}")),
            "error must name the path: {err}"
        );
    }
}

/// 6: the swatch default equals what the renderer emits, per theme.
#[test]
fn test_c06_subagent_swatch_matches_renderer_per_theme() {
    let _env = EnvGuard::new();
    for theme in THEMES {
        let draft = base(theme);
        for id in SUBAGENT_IDS {
            let expected = match PALETTE_ROLE.iter().find(|(seg, _)| *seg == id) {
                Some((_, role)) => palette(theme, role),
                None => fg_of(&render_sub(&draft, id)),
            };
            assert_eq!(
                effective_subagent_fg(&draft, id).to_uppercase(),
                expected,
                "theme {theme}, subagent {id}"
            );
        }
    }
}

/// 6: the effective colour reads the draft override.
#[test]
fn test_c07_effective_subagent_fg_reads_draft() {
    let draft = with(&base("matrix-tron"), &["subagent", "colors", "model", "fg"], json!(SUB));
    assert_eq!(effective_subagent_fg(&draft, "model"), SUB);
}

/// 6: cycling writes `subagent.colors.<id>.fg` and leaves `segments` alone.
#[test]
fn test_c08_cycle_subagent_fg_writes_subagent_colors() {
    let draft = base("matrix-tron");
    let next = cycle_subagent_fg(&draft, "model", 1);
    let color = next.0["subagent"]["colors"]["model"]["fg"].as_str().expect("fg written");
    assert!(color.starts_with('#'), "{color}");
    assert_eq!(effective_subagent_fg(&next, "model"), color);
    assert!(next.0.get("segments").is_none_or(|s| s.get("model").is_none()));
}

/// 7: the colour screen gains one Subagent section (8 rows) between the main
/// segment rows (ends at 20) and the gauge rows: [20, 20+8, 26+8, 29+8, 30+8].
#[test]
fn test_c09_colors_separators_include_subagent_section() {
    let draft = base("matrix-tron");
    assert_eq!(colors_separators(&draft), [20, 28, 34, 37, 38]);
}
