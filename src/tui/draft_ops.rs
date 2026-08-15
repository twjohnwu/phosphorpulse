//! Pure draft editing operations for the TUI.

use phosphorpulse::config::model::Config;
use serde_json::{Map, Value};

const MAX_ROWS: usize = 3;
const COLOR_DEPTHS: [&str; 4] = ["auto", "16color", "256color", "truecolor"];
pub const NAMED_COLORS: [&str; 58] = [
    "#00FF41", "#00CF41", "#50FA7B", "#A3BE8C", "#859900", "#B8BB26", "#008F11", "#14532D",
    "#121612", "#0E120E", "#0A0E0A", "#00E5FF", "#00FFFF", "#8BE9FD", "#88C0D0", "#8EC07C",
    "#2AA198", "#00CDCD", "#8FBCBB", "#003232", "#268BD2", "#81A1C1", "#83A598", "#1E3A8A",
    "#FF79C6", "#FF00FF", "#D33682", "#BD93F9", "#6C71C4", "#D3869B", "#B48EAD", "#FB4934",
    "#FF5555", "#FF3737", "#BF616A", "#DC322F", "#B76E79", "#7F1D1D", "#FFB86C", "#FE8019",
    "#FFA500", "#FF7F50", "#D08770", "#CB4B16", "#FFD700", "#FABD2F", "#F1FA8C", "#FFFF00",
    "#EBCB8B", "#B58900", "#FFFFFF", "#D3D3D3", "#C0C0C0", "#969696", "#657B83", "#586E75",
    "#333333", "#282D2A",
];
const NAMED_COLOR_NAMES: [&str; 58] = [
    "bright green",
    "phosphor green",
    "dracula green",
    "nord green",
    "solarized green",
    "gruvbox green",
    "mid rain green",
    "forest",
    "onyx green",
    "shadow green",
    "void green",
    "light cyan",
    "cyan",
    "dracula cyan",
    "nord cyan",
    "gruvbox aqua",
    "solarized cyan",
    "matrix cyan",
    "nord teal",
    "dark teal",
    "solarized blue",
    "nord blue",
    "gruvbox blue",
    "navy",
    "dracula pink",
    "magenta",
    "solarized magenta",
    "dracula purple",
    "solarized violet",
    "gruvbox purple",
    "nord purple",
    "gruvbox red",
    "dracula red",
    "red",
    "nord red",
    "solarized red",
    "rose gold",
    "maroon",
    "dracula orange",
    "gruvbox orange",
    "orange",
    "coral",
    "nord orange",
    "solarized orange",
    "gold",
    "gruvbox yellow",
    "dracula yellow",
    "yellow",
    "nord yellow",
    "solarized yellow",
    "white",
    "light gray",
    "silver",
    "neutral gray",
    "solarized base00",
    "solarized base01",
    "charcoal",
    "dark gray",
];

pub fn named_color(hex: &str) -> Option<(usize, &'static str)> {
    NAMED_COLORS
        .iter()
        .position(|color| color.eq_ignore_ascii_case(hex))
        .map(|index| (index + 1, NAMED_COLOR_NAMES[index]))
}

/// Cycles the frozen shared named-colour table. Custom hex values enter at the
/// first entry, matching the other TUI colour controls.
pub fn cycle_named_color(current: &str, direction: i8) -> &'static str {
    NAMED_COLORS[cycle_index(&NAMED_COLORS, current, direction)]
}

pub fn effective_segment_fg(draft: &Config, segment: &str) -> String {
    effective_fg(draft, segment)
}

fn step(direction: i8) -> Option<i8> {
    match direction.cmp(&0) {
        std::cmp::Ordering::Less => Some(-1),
        std::cmp::Ordering::Greater => Some(1),
        std::cmp::Ordering::Equal => None,
    }
}

fn clamp(value: i64, min: i64, max: i64) -> i64 {
    value.clamp(min, max)
}

fn object_field<'a>(object: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let value = object
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()));
    loop {
        if let Value::Object(map) = value {
            return map;
        }
        *value = Value::Object(Map::new());
    }
}

fn array_at_row(draft: &mut Config, row: usize) -> Option<&mut Vec<Value>> {
    let rows = draft.0.get_mut("rows")?.as_array_mut()?;
    rows.get_mut(row)?.get_mut("segments")?.as_array_mut()
}

fn cycle_index(values: &[&str], current: &str, direction: i8) -> usize {
    let current = values
        .iter()
        .position(|value| value.eq_ignore_ascii_case(current));
    match current {
        Some(index) => (index as i32 + direction as i32).rem_euclid(values.len() as i32) as usize,
        None => 0,
    }
}

fn effective_fg(draft: &Config, segment: &str) -> String {
    let configured = draft
        .0
        .get("segments")
        .and_then(Value::as_object)
        .and_then(|segments| segments.get(segment))
        .and_then(Value::as_object)
        .and_then(|config| config.get("fg"))
        .and_then(Value::as_str);
    let effective = match configured.unwrap_or(segment) {
        "model" | "dir" | "pomodoro" | "pomodoro.work" => "#00CF41",
        "effort" => "#008F11",
        "git" | "git.ok" | "ctx" | "ctx.ok" | "pomodoro.break" => "#00CDCD",
        "limit5h" | "limit7d" | "ok" => "#00FF41",
        "node" | "python" => "#00E5FF",
        "version" => "#969696",
        "cost" | "burn" => "#008F11",
        "clock" => "#003232",
        value => value,
    };
    effective.into()
}

pub fn add_row(draft: &Config, segments: Vec<String>) -> Config {
    let mut next = draft.clone();
    let Some(rows) = next.0.get_mut("rows").and_then(Value::as_array_mut) else {
        return next;
    };
    if rows.len() < MAX_ROWS {
        rows.push(serde_json::json!({"layout": "auto", "segments": segments}));
    }
    next
}

pub fn insert_row_segment(draft: &Config, row: usize, index: usize, segment: &str) -> Config {
    let mut next = draft.clone();
    let Some(segments) = array_at_row(&mut next, row) else {
        return next;
    };
    if index <= segments.len() {
        segments.insert(index, Value::String(segment.into()));
    }
    next
}

pub fn delete_row_segment(draft: &Config, row: usize, index: usize) -> Config {
    let mut next = draft.clone();
    let remove_row = {
        let Some(rows) = next.0.get_mut("rows").and_then(Value::as_array_mut) else {
            return next;
        };
        let Some(segments) = rows
            .get_mut(row)
            .and_then(|entry| entry.get_mut("segments"))
            .and_then(Value::as_array_mut)
        else {
            return next;
        };
        if index >= segments.len() {
            return next;
        }
        segments.remove(index);
        segments.is_empty() && row > 0 && row + 1 == rows.len()
    };
    if remove_row {
        if let Some(rows) = next.0.get_mut("rows").and_then(Value::as_array_mut) {
            rows.remove(row);
        }
    }
    next
}

pub fn move_row_segment(draft: &Config, row: usize, index: usize, direction: i8) -> Config {
    let mut next = draft.clone();
    let Some(direction) = step(direction) else {
        return next;
    };
    let Some(segments) = array_at_row(&mut next, row) else {
        return next;
    };
    if index >= segments.len() || segments.len() < 2 {
        return next;
    }
    let destination = (index as i32 + direction as i32).rem_euclid(segments.len() as i32) as usize;
    segments.swap(index, destination);
    next
}

pub fn toggle_row_layout(draft: &Config, row: usize) -> Config {
    let mut next = draft.clone();
    let Some(layout) = next
        .0
        .get_mut("rows")
        .and_then(Value::as_array_mut)
        .and_then(|rows| rows.get_mut(row))
        .and_then(Value::as_object_mut)
    else {
        return next;
    };
    let next_layout = match layout.get("layout").and_then(Value::as_str) {
        Some("fixed") => "auto",
        _ => "fixed",
    };
    layout.insert("layout".into(), Value::String(next_layout.into()));
    next
}

fn adjust_pomodoro(
    draft: &Config,
    key: &str,
    default: i64,
    delta: i64,
    min: i64,
    max: i64,
    direction: i8,
) -> Config {
    let mut next = draft.clone();
    if direction == 0 {
        return next;
    }
    let pomodoro = object_field(&mut next.0, "pomodoro");
    let current = pomodoro.get(key).and_then(Value::as_i64).unwrap_or(default);
    pomodoro.insert(
        key.into(),
        Value::from(clamp(current + delta * i64::from(direction), min, max)),
    );
    next
}

pub fn adjust_pomodoro_work_min(draft: &Config, direction: i8) -> Config {
    adjust_pomodoro(draft, "workMin", 25, 5, 5, 90, direction)
}

pub fn adjust_pomodoro_refresh_sec(draft: &Config, direction: i8) -> Config {
    adjust_pomodoro(draft, "refreshSec", 1, 1, 1, 60, direction)
}

pub fn cycle_color_depth(draft: &Config, direction: i8) -> Config {
    let mut next = draft.clone();
    if direction == 0 {
        return next;
    }
    let current = next
        .0
        .get("colorDepth")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    next.0.insert(
        "colorDepth".into(),
        Value::String(COLOR_DEPTHS[cycle_index(&COLOR_DEPTHS, current, direction)].into()),
    );
    next
}

pub fn cycle_segment_fg(draft: &Config, segment: &str, direction: i8) -> Config {
    let mut next = draft.clone();
    let Some(direction) = step(direction) else {
        return next;
    };
    let current = effective_fg(draft, segment);
    let color = NAMED_COLORS[cycle_index(&NAMED_COLORS, &current, direction)];
    object_field(object_field(&mut next.0, "segments"), segment)
        .insert("fg".into(), Value::String(color.into()));
    next
}

/// The effective matrix-tron palette color for `gauge.<state>Color`, matching
/// the frozen TUI when no draft override has been selected yet.
pub fn effective_gauge_color(draft: &Config, state: &str) -> String {
    let default = match state {
        "ok" => "#00FF41",
        "warn" => "#FF7F50",
        "hot" => "#FF3737",
        _ => return String::new(),
    };
    draft
        .0
        .get("gauge")
        .and_then(Value::as_object)
        .and_then(|gauge| gauge.get(&format!("{state}Color")))
        .and_then(Value::as_str)
        .unwrap_or(default)
        .into()
}

/// Cycles the named-color catalogue into `gauge.<state>Color`, just as the
/// TS ColorsThemeScreen cycles segment colors into `segments.<id>.fg`.
pub fn cycle_gauge_color(draft: &Config, state: &str, direction: i8) -> Config {
    let mut next = draft.clone();
    let Some(direction) = step(direction) else {
        return next;
    };
    let key = match state {
        "ok" => "okColor",
        "warn" => "warnColor",
        "hot" => "hotColor",
        _ => return next,
    };
    let current = effective_gauge_color(draft, state);
    let color = NAMED_COLORS[cycle_index(&NAMED_COLORS, &current, direction)];
    object_field(&mut next.0, "gauge").insert(key.into(), Value::String(color.into()));
    next
}

fn adjust_gauge(
    draft: &Config,
    key: &str,
    default: i64,
    delta: i64,
    min: i64,
    max: i64,
    direction: i8,
) -> Config {
    let mut next = draft.clone();
    if direction == 0 {
        return next;
    }
    let gauge = object_field(&mut next.0, "gauge");
    let current = gauge.get(key).and_then(Value::as_i64).unwrap_or(default);
    gauge.insert(
        key.into(),
        Value::from(clamp(current + delta * i64::from(direction), min, max)),
    );
    next
}

pub fn adjust_gauge_bar_width(draft: &Config, direction: i8) -> Config {
    adjust_gauge(draft, "barWidth", 20, 1, 1, 80, direction)
}

pub fn adjust_gauge_warn_pct(draft: &Config, direction: i8) -> Config {
    let hot = draft
        .0
        .get("gauge")
        .and_then(|gauge| gauge.get("hotPct"))
        .and_then(Value::as_i64)
        .unwrap_or(85);
    adjust_gauge(draft, "warnPct", 65, 5, 0, hot, direction)
}

pub fn adjust_gauge_hot_pct(draft: &Config, direction: i8) -> Config {
    let warn = draft
        .0
        .get("gauge")
        .and_then(|gauge| gauge.get("warnPct"))
        .and_then(Value::as_i64)
        .unwrap_or(65);
    adjust_gauge(draft, "hotPct", 85, 5, warn, 100, direction)
}

pub fn adjust_dir_path_depth(draft: &Config, direction: i8) -> Config {
    let mut next = draft.clone();
    if direction == 0 {
        return next;
    }
    let dir = object_field(object_field(&mut next.0, "segments"), "dir");
    let current = dir.get("pathDepth").and_then(Value::as_i64).unwrap_or(99);
    dir.insert(
        "pathDepth".into(),
        Value::from(clamp(current + i64::from(direction), 1, 99)),
    );
    next
}

fn subagent_segments(draft: &mut Config) -> Option<&mut Vec<Value>> {
    draft
        .0
        .get_mut("subagent")?
        .get_mut("segments")?
        .as_array_mut()
}

pub fn add_subagent_segment(draft: &Config, segment: &str) -> Config {
    let mut next = draft.clone();
    if let Some(segments) = subagent_segments(&mut next) {
        segments.push(Value::String(segment.into()));
    }
    next
}

pub fn insert_subagent_segment(draft: &Config, index: usize, segment: &str) -> Config {
    let mut next = draft.clone();
    let Some(segments) = subagent_segments(&mut next) else {
        return next;
    };
    if index <= segments.len() {
        segments.insert(index, Value::String(segment.into()));
    }
    next
}

pub fn delete_subagent_segment(draft: &Config, index: usize) -> Config {
    let mut next = draft.clone();
    let Some(segments) = subagent_segments(&mut next) else {
        return next;
    };
    if index < segments.len() {
        segments.remove(index);
    }
    next
}

pub fn move_subagent_segment(draft: &Config, index: usize, direction: i8) -> Config {
    let mut next = draft.clone();
    let Some(direction) = step(direction) else {
        return next;
    };
    let Some(segments) = subagent_segments(&mut next) else {
        return next;
    };
    if index >= segments.len() || segments.len() < 2 {
        return next;
    }
    let destination = (index as i32 + direction as i32).rem_euclid(segments.len() as i32) as usize;
    segments.swap(index, destination);
    next
}
