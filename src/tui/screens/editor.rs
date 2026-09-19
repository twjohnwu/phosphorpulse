use crate::tui::{
    app::{AppState, ScreenId, SegmentPickerTarget},
    bin_install,
    codex::config_io,
    draft_ops,
    i18n::{Key, t},
    screens::{
        Action, Screen,
        common::{
            ACCENT, ERROR, TEXT, areas, footer, footer_error, header, move_focus, preview,
            scrolled_list, too_small,
        },
    },
    settings_writer, templates_io,
};
use crossterm::event::{KeyCode, KeyEvent};
use phosphorpulse::{
    config::{self, model::Config},
    jsx::width::display_width,
    protocol,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

pub struct RowsSegmentsScreen {
    move_mode: bool,
}
pub struct SubagentLineScreen {
    move_mode: bool,
}
pub struct ColorsThemeScreen;
pub struct TemplatesScreen;
pub struct SettingsInstallScreen;
pub struct SaveExitScreen;

/// The first 14 IDs keep the frozen TS `listMainSegments()` order; the last 5
/// were added by the stdin-extra-segments change.
const fn builtin_main_segment_ids() -> [&'static str; 19] {
    [
        "model", "effort", "git", "dir", "ctx", "limit5h", "limit7d", "node", "python",
        "version", "cost", "burn", "pomodoro", "flex", "session", "fastMode", "outputStyle",
        "thinking", "limitModel",
    ]
}

fn configured_commands(draft: &Config) -> BTreeMap<String, config::CommandSpec> {
    config::commands(&Value::Object(draft.0.clone()))
}

pub fn main_segment_ids(draft: &Config) -> Vec<String> {
    builtin_main_segment_ids()
        .into_iter()
        .map(str::to_owned)
        .chain(
            configured_commands(draft)
                .into_keys()
                .map(|name| format!("cmd:{name}")),
        )
        .collect()
}
/// Frozen TS `listSubagentSegments()` inventory; picker choices must never be
/// inferred from the main-line list.
const SUBAGENT_SEGMENT_IDS: [&str; 7] = [
    "name",
    "desc",
    "model",
    "ctx",
    "elapsed",
    "effort",
    "tokenCount",
];
const BUILTIN_TEMPLATE_NAMES: [&str; 3] = ["matrix-tron", "solarized-dark", "solarized-light"];

fn selected_segment_style(move_mode: bool) -> Style {
    let style = Style::default()
        .fg(TEXT)
        .add_modifier(Modifier::BOLD)
        .add_modifier(Modifier::REVERSED);
    if move_mode {
        style.fg(Color::Yellow)
    } else {
        style
    }
}

fn horizontal_segments(segments: &[String], selected: usize, move_mode: bool) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let style = if index == selected {
            selected_segment_style(move_mode)
        } else {
            Style::default().fg(TEXT)
        };
        spans.push(Span::styled(segment.clone(), style));
    }
    Line::from(spans)
}

fn pomodoro_work_min(s: &AppState) -> i64 {
    draft_i64(s, "pomodoro", "workMin").unwrap_or(25)
}

fn usage_refresh_sec(s: &AppState) -> i64 {
    draft_i64(s, "usage", "refreshSec")
        .filter(|value| (60..=600).contains(value))
        .unwrap_or(300)
}

fn draft_i64(s: &AppState, section: &str, key: &str) -> Option<i64> {
    s.draft
        .0
        .get(section)
        .and_then(|value| value.get(key))
        .and_then(Value::as_i64)
}

fn integer_setting_line(s: &AppState, marker: &str, key: &Key, value: i64) -> Line<'static> {
    Line::from(format!(
        "{marker} {}",
        t(s.lang, key, &[("value", &value.to_string())])
    ))
}

fn truncate_with_ellipsis(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.into();
    }
    if max_width == 0 {
        return String::new();
    }
    let content_width = max_width.saturating_sub(display_width("…"));
    let mut truncated = String::new();
    let mut used = 0;
    for grapheme in value.graphemes(true) {
        let width = display_width(grapheme);
        if used + width > content_width {
            break;
        }
        truncated.push_str(grapheme);
        used += width;
    }
    truncated.push('…');
    truncated
}

fn row_segment_line(
    s: &AppState,
    segments: &[String],
    selected: usize,
    move_mode: bool,
) -> Line<'static> {
    let work = pomodoro_work_min(s);
    let mut spans = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let style = if index == selected {
            selected_segment_style(move_mode)
        } else {
            Style::default().fg(TEXT)
        };
        let annotation = if segment == "pomodoro" {
            format!(
                " {}",
                t(
                    s.lang,
                    &Key::RowsSegmentsPomodoroValues,
                    &[("work", &work.to_string())]
                )
            )
        } else {
            String::new()
        };
        spans.push(Span::styled(format!("{segment}{annotation}"), style));
    }
    Line::from(spans)
}

fn hex_color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

fn segment_fg_hint(draft: &Config, segment: &str) -> String {
    let hex = draft_ops::effective_segment_fg(draft, segment);
    draft_ops::named_color(&hex).map_or("(-/58)".into(), |(index, _)| format!("({index}/58)"))
}

/// Mirrors TemplatesScreen.tsx's `updateDraft` call on template load: merge
/// the four displayed configuration fields and set the active template. The
/// snapshot palette intentionally remains on disk for renderer lookup rather
/// than being copied into the draft.
fn apply_loaded_template(s: &mut AppState, template: Value, name: &str) {
    let template = template.as_object();
    s.draft
        .0
        .insert("activeTemplate".into(), Value::String(name.into()));
    for field in ["rows", "subagent", "gauge", "segments"] {
        s.draft.0.insert(
            field.into(),
            template
                .and_then(|snapshot| snapshot.get(field))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
}
#[derive(Clone, PartialEq, Debug)]
pub enum ColorsFocus {
    Depth,
    SegmentFg(&'static str),
    GaugeWidth,
    GaugeWarnPct,
    GaugeHotPct,
    GaugeColor(&'static str),
    PomodoroWorkMin,
    UsageRefreshSec,
    CmdFg(String),
    CmdCommand(String),
    CmdTimeoutMs(String),
    CmdTtlSec(String),
    CmdMaxWidth(String),
    CmdPreserveColors(String),
    CmdAdd,
    DirPathDepth,
    NerdFont,
}

const BASE_COLORS_FOCUS: [ColorsFocus; 27] = [
    ColorsFocus::Depth,
    ColorsFocus::SegmentFg("model"),
    ColorsFocus::SegmentFg("effort"),
    ColorsFocus::SegmentFg("git"),
    ColorsFocus::SegmentFg("dir"),
    ColorsFocus::SegmentFg("ctx"),
    ColorsFocus::SegmentFg("limit5h"),
    ColorsFocus::SegmentFg("limit7d"),
    ColorsFocus::SegmentFg("node"),
    ColorsFocus::SegmentFg("python"),
    ColorsFocus::SegmentFg("version"),
    ColorsFocus::SegmentFg("cost"),
    ColorsFocus::SegmentFg("burn"),
    ColorsFocus::SegmentFg("pomodoro"),
    ColorsFocus::SegmentFg("session"),
    ColorsFocus::SegmentFg("fastMode"),
    ColorsFocus::SegmentFg("outputStyle"),
    ColorsFocus::SegmentFg("thinking"),
    ColorsFocus::SegmentFg("limitModel"),
    ColorsFocus::GaugeWidth,
    ColorsFocus::GaugeWarnPct,
    ColorsFocus::GaugeHotPct,
    ColorsFocus::GaugeColor("ok"),
    ColorsFocus::GaugeColor("warn"),
    ColorsFocus::GaugeColor("hot"),
    ColorsFocus::PomodoroWorkMin,
    ColorsFocus::UsageRefreshSec,
];

pub fn colors_focus(draft: &Config) -> Vec<ColorsFocus> {
    let commands = configured_commands(draft);
    let mut focus = BASE_COLORS_FOCUS.to_vec();
    for name in commands.keys() {
        focus.extend([
            ColorsFocus::CmdFg(name.clone()),
            ColorsFocus::CmdCommand(name.clone()),
            ColorsFocus::CmdTimeoutMs(name.clone()),
            ColorsFocus::CmdTtlSec(name.clone()),
            ColorsFocus::CmdMaxWidth(name.clone()),
            ColorsFocus::CmdPreserveColors(name.clone()),
        ]);
    }
    focus.extend([
        ColorsFocus::CmdAdd,
        ColorsFocus::DirPathDepth,
        ColorsFocus::NerdFont,
    ]);
    focus
}

pub fn colors_separators(draft: &Config) -> Vec<usize> {
    let command_count = configured_commands(draft).len();
    let mut separators = vec![19, 25, 27];
    separators.extend((1..command_count).map(|index| 27 + 6 * index));
    separators.push(28 + 6 * command_count);
    separators
}

pub fn colors_line_index(draft: &Config, focus: usize) -> usize {
    focus
        + colors_separators(draft)
            .into_iter()
            .filter(|&separator| separator <= focus)
            .count()
}

macro_rules! simple_new {($($t:ident),*)=>{$(impl $t{pub fn new()->Self{Self}})*}}
simple_new!(
    ColorsThemeScreen,
    TemplatesScreen,
    SettingsInstallScreen,
    SaveExitScreen
);
impl RowsSegmentsScreen {
    pub fn new() -> Self {
        Self { move_mode: false }
    }
}
impl SubagentLineScreen {
    pub fn new() -> Self {
        Self { move_mode: false }
    }
}
fn back(event: KeyEvent, state: &mut AppState) -> bool {
    if event.code == KeyCode::Esc {
        state.screen = ScreenId::MainMenu;
        state.set_selected(0);
        true
    } else {
        false
    }
}
fn row_count(state: &AppState) -> usize {
    state
        .draft
        .0
        .get("rows")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}
fn row_segments(state: &AppState, row: usize) -> Vec<String> {
    state
        .draft
        .0
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.get(row))
        .and_then(|r| r.get("segments"))
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn row_layout(state: &AppState, row: usize) -> &str {
    state
        .draft
        .0
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.get(row))
        .and_then(|entry| entry.get("layout"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
}
fn subagent_segment_count(state: &AppState) -> usize {
    state
        .draft
        .0
        .get("subagent")
        .and_then(|value| value.get("segments"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}
fn clear_template_pending(state: &mut AppState) {
    state.input.clear();
    state.pending_name.clear();
    state.pending_path.clear();
}

/// Expands only the portable, explicitly supported leading `~/` shorthand.
/// Keeping this at the UI boundary means every filesystem operation receives a
/// real path while the user remains free to edit the friendly default.
fn expand_home_path(value: &str) -> PathBuf {
    templates_io::expand_home_path(Path::new(value))
}

fn template_names(templates_dir: &Path) -> Vec<String> {
    let mut names = BUILTIN_TEMPLATE_NAMES.map(str::to_owned).to_vec();
    if let Ok(entries) = fs::read_dir(templates_dir) {
        let mut saved = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .filter_map(|entry| {
                entry
                    .path()
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .filter(|name| !BUILTIN_TEMPLATE_NAMES.contains(&name.as_str()))
            .collect::<Vec<_>>();
        saved.sort();
        names.extend(saved);
    }
    names
}

fn settings_install_status() -> (Option<PathBuf>, Option<String>, Option<String>) {
    let commands = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".claude/settings.json"))
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.as_object().cloned());
    let status_line = commands
        .as_ref()
        .and_then(|settings| settings_writer::configured_command(settings, "statusLine"));
    let subagent_line = commands
        .as_ref()
        .and_then(|settings| settings_writer::configured_command(settings, "subagentStatusLine"));
    (bin_install::detected_bin(), status_line, subagent_line)
}

/// Uses the same strict reader as the Codex Settings editor, so a partial or
/// unreadable config is accurately represented as not configured here.
fn codex_settings_status() -> Option<config_io::CodexConfig> {
    let path =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex/config.toml"))?;
    config_io::read_config(&path)
        .ok()
        .map(|snapshot| snapshot.config)
}

impl Screen for RowsSegmentsScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        if let crate::tui::app::UiMode::SegmentPicker { target, .. } = s.mode {
            let options: Vec<String> = match target {
                SegmentPickerTarget::Main => main_segment_ids(&s.draft),
                SegmentPickerTarget::Subagent => SUBAGENT_SEGMENT_IDS
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            };
            let body = options
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    format!("{} {id}", if index == s.selected() { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("\n");
            header(f, a[0], &t(s.lang, &Key::CommonSegmentPicker, &[]));
            f.render_widget(
                scrolled_list(body, s.selected(), options.len(), a[1])
                    .style(Style::default().fg(TEXT)),
                a[1],
            );
            footer(
                f,
                a[2],
                &[
                    t(s.lang, &Key::SegmentPickerHintSelect, &[]),
                    t(s.lang, &Key::SegmentPickerHintConfirm, &[]),
                    t(s.lang, &Key::SegmentPickerHintCancel, &[]),
                ]
                .join(" "),
            );
            preview(f, a[3], s);
            return;
        }
        let row_number = (s.row_index + 1).to_string();
        let layout = t(
            s.lang,
            &if row_layout(s, s.row_index) == "fixed" {
                Key::RowsSegmentsLayoutFixed
            } else {
                Key::RowsSegmentsLayoutAuto
            },
            &[],
        );
        let screen_title = format!(
            "{}{}",
            t(
                s.lang,
                &Key::RowsSegmentsHeader,
                &[("n", &row_number), ("max", "3"), ("layout", &layout)],
            ),
            if self.move_mode {
                format!(" {}", t(s.lang, &Key::CommonMoveModeSuffix, &[]))
            } else {
                String::new()
            },
        );
        header(f, a[0], &screen_title);
        let seg = row_segments(s, s.row_index);
        f.render_widget(
            Paragraph::new(row_segment_line(s, &seg, s.selected(), self.move_mode)),
            a[1],
        );
        footer(
            f,
            a[2],
            &[
                t(s.lang, &Key::HintsAddA, &[]),
                t(s.lang, &Key::HintsInsertI, &[]),
                t(s.lang, &Key::HintsDeleteD, &[]),
                t(s.lang, &Key::HintsEnterMoveMode, &[]),
                t(s.lang, &Key::CommonMove, &[]),
                t(s.lang, &Key::HintsSpaceLayout, &[]),
                t(s.lang, &Key::HintsTabSwitchRow, &[]),
            ]
            .join(" "),
        );
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if let crate::tui::app::UiMode::SegmentPicker {
            target,
            insert_after,
            position,
        } = s.mode
        {
            let options: Vec<String> = match target {
                SegmentPickerTarget::Main => main_segment_ids(&s.draft),
                SegmentPickerTarget::Subagent => SUBAGENT_SEGMENT_IDS
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            };
            match e.code {
                KeyCode::Esc => s.mode = crate::tui::app::UiMode::Normal,
                KeyCode::Up => move_focus(s, options.len(), -1),
                KeyCode::Down => move_focus(s, options.len(), 1),
                KeyCode::Enter => {
                    let id = &options[s.selected().min(options.len() - 1)];
                    match target {
                        SegmentPickerTarget::Main => {
                            let row = s.row_index;
                            let count = row_segments(s, row).len();
                            let index = if insert_after {
                                (position + 1).min(count)
                            } else {
                                count
                            };
                            s.draft = if row == row_count(s) {
                                // TS RowsSegmentsScreen's virtual new-row slot becomes real
                                // only when the picker commits its first segment.
                                draft_ops::add_row(&s.draft, vec![id.clone()])
                            } else {
                                draft_ops::insert_row_segment(&s.draft, row, index, id)
                            };
                            s.set_selected(index);
                        }
                        SegmentPickerTarget::Subagent => {
                            let count = subagent_segment_count(s);
                            let index = if insert_after {
                                (position + 1).min(count)
                            } else {
                                count
                            };
                            s.draft = draft_ops::insert_subagent_segment(&s.draft, index, id);
                            s.set_selected(index);
                        }
                    }
                    s.mode = crate::tui::app::UiMode::Normal;
                }
                _ => {}
            }
            return Action::Redraw;
        }
        if back(e, s) {
            return Action::Back;
        }
        let row = s.row_index;
        let n = row_segments(s, row).len();
        if n == 0 {
            s.set_selected(0);
        } else if s.selected() >= n {
            s.set_selected(n - 1);
        }
        match e.code {
            KeyCode::Left if self.move_mode && n > 1 => {
                let to = (s.selected() + n - 1) % n;
                s.draft = draft_ops::move_row_segment(&s.draft, row, s.selected(), -1);
                s.set_selected(to);
            }
            KeyCode::Right if self.move_mode && n > 1 => {
                let to = (s.selected() + 1) % n;
                s.draft = draft_ops::move_row_segment(&s.draft, row, s.selected(), 1);
                s.set_selected(to);
            }
            KeyCode::Left => move_focus(s, n, -1),
            KeyCode::Right => move_focus(s, n, 1),
            KeyCode::Enter => self.move_mode = !self.move_mode,
            KeyCode::Tab => {
                // Mirrors RowsSegmentsScreen.tsx: one uncommitted row slot is
                // navigable below the three-row cap, then picker selection
                // materializes it via add_row above.
                let slots = (row_count(s).min(2)) + 1;
                s.row_index = (row + 1) % slots;
                s.set_selected(0);
            }
            KeyCode::Char('a') => {
                s.mode = crate::tui::app::UiMode::SegmentPicker {
                    target: SegmentPickerTarget::Main,
                    insert_after: false,
                    position: s.selected(),
                };
                s.set_selected(0);
            }
            KeyCode::Char('i') => {
                s.mode = crate::tui::app::UiMode::SegmentPicker {
                    target: SegmentPickerTarget::Main,
                    insert_after: true,
                    position: s.selected(),
                };
                s.set_selected(0);
            }
            KeyCode::Char('d') => {
                s.draft = draft_ops::delete_row_segment(&s.draft, row, s.selected());
                let rows = row_count(s);
                if rows > 0 {
                    s.row_index = s.row_index.min(rows - 1);
                    s.set_selected(0);
                }
            }
            KeyCode::Char(' ') => s.draft = draft_ops::toggle_row_layout(&s.draft, row),
            _ => {}
        }
        Action::Redraw
    }
}
impl Screen for SubagentLineScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        if let crate::tui::app::UiMode::SegmentPicker {
            target: SegmentPickerTarget::Subagent,
            ..
        } = s.mode
        {
            let options: &[&str] = &SUBAGENT_SEGMENT_IDS;
            let body = options
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    format!("{} {id}", if index == s.selected() { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("\n");
            header(f, a[0], &t(s.lang, &Key::CommonSegmentPicker, &[]));
            f.render_widget(
                scrolled_list(body, s.selected(), options.len(), a[1])
                    .style(Style::default().fg(TEXT)),
                a[1],
            );
            footer(
                f,
                a[2],
                &[
                    t(s.lang, &Key::SegmentPickerHintSelect, &[]),
                    t(s.lang, &Key::SegmentPickerHintConfirm, &[]),
                    t(s.lang, &Key::SegmentPickerHintCancel, &[]),
                ]
                .join(" "),
            );
            preview(f, a[3], s);
            return;
        }
        let title = if self.move_mode {
            format!(
                "{} {}",
                t(s.lang, &Key::SubagentLineTitle, &[]),
                t(s.lang, &Key::CommonMoveModeSuffix, &[])
            )
        } else {
            t(s.lang, &Key::SubagentLineTitle, &[])
        };
        header(f, a[0], &title);
        let segments = s
            .draft
            .0
            .get("subagent")
            .and_then(|v| v.get("segments"))
            .and_then(Value::as_array)
            .map(|segments| {
                segments
                    .iter()
                    .enumerate()
                    .filter_map(|(_, value)| value.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        f.render_widget(
            Paragraph::new(horizontal_segments(&segments, s.selected(), self.move_mode)),
            a[1],
        );
        footer(
            f,
            a[2],
            &[
                t(s.lang, &Key::HintsAddA, &[]),
                t(s.lang, &Key::HintsInsertI, &[]),
                t(s.lang, &Key::HintsDeleteD, &[]),
                t(s.lang, &Key::HintsEnterMoveMode, &[]),
                t(s.lang, &Key::CommonMove, &[]),
            ]
            .join(" "),
        );
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if let crate::tui::app::UiMode::SegmentPicker {
            target: SegmentPickerTarget::Subagent,
            ..
        } = s.mode
        {
            // RowsSegmentsScreen owns the shared picker dispatch for both targets.
            return RowsSegmentsScreen::new().on_key(e, s);
        }
        if back(e, s) {
            return Action::Back;
        }
        let count = subagent_segment_count(s);
        if count == 0 {
            s.set_selected(0);
        } else if s.selected() >= count {
            s.set_selected(count - 1);
        }
        match e.code {
            KeyCode::Left if count > 0 => {
                let selected = s.selected().min(count - 1);
                if self.move_mode && count > 1 {
                    let destination = (selected + count - 1) % count;
                    s.draft = draft_ops::move_subagent_segment(&s.draft, selected, -1);
                    s.set_selected(destination);
                } else {
                    move_focus(s, count, -1);
                }
            }
            KeyCode::Right if count > 0 => {
                let selected = s.selected().min(count - 1);
                if self.move_mode && count > 1 {
                    let destination = (selected + 1) % count;
                    s.draft = draft_ops::move_subagent_segment(&s.draft, selected, 1);
                    s.set_selected(destination);
                } else {
                    move_focus(s, count, 1);
                }
            }
            KeyCode::Enter => self.move_mode = !self.move_mode,
            KeyCode::Char('a') => {
                s.mode = crate::tui::app::UiMode::SegmentPicker {
                    target: SegmentPickerTarget::Subagent,
                    insert_after: false,
                    position: s.selected(),
                };
                s.set_selected(0);
            }
            KeyCode::Char('i') => {
                s.mode = crate::tui::app::UiMode::SegmentPicker {
                    target: SegmentPickerTarget::Subagent,
                    insert_after: true,
                    position: s.selected(),
                };
                s.set_selected(0);
            }
            KeyCode::Char('d') => {
                s.draft = draft_ops::delete_subagent_segment(&s.draft, s.selected());
                s.set_selected(s.selected().saturating_sub(1));
            }
            _ => {}
        }
        Action::Redraw
    }
}

fn command_input_prompt(s: &AppState) -> Option<String> {
    let key = match s.mode {
        crate::tui::app::UiMode::CommandName => Key::ColorsHintCmdName,
        crate::tui::app::UiMode::CommandString => Key::ColorsHintCmdCommand,
        crate::tui::app::UiMode::CommandEdit => Key::ColorsHintCmdEdit,
        _ => return None,
    };
    Some(t(s.lang, &key, &[]))
}

fn clear_command_input(s: &mut AppState) {
    s.input.clear();
    s.input_error = None;
    s.pending_command = None;
    s.mode = crate::tui::app::UiMode::Normal;
}

fn submit_command_input(s: &mut AppState) {
    use crate::tui::app::UiMode;

    let mode = s.mode.clone();
    let value = std::mem::take(&mut s.input);
    match mode {
        UiMode::CommandName => {
            let commands = configured_commands(&s.draft);
            if config::model::is_valid_command_name(&value) && !commands.contains_key(&value) {
                s.pending_command = Some(value);
                s.input_error = None;
                s.mode = UiMode::CommandString;
            } else {
                s.input = value;
                s.input_error = Some(Key::ColorsCmdNameInvalid);
            }
        }
        UiMode::CommandString | UiMode::CommandEdit if value.trim().is_empty() => {
            s.input = value;
            s.input_error = Some(Key::ColorsCmdCommandEmpty);
        }
        UiMode::CommandString => {
            let Some(name) = s.pending_command.take() else {
                clear_command_input(s);
                return;
            };
            s.draft = draft_ops::add_command(&s.draft, &name, &value);
            let selected = colors_focus(&s.draft)
                .iter()
                .position(|focus| matches!(focus, ColorsFocus::CmdFg(candidate) if candidate == &name))
                .expect("new command has a foreground focus row");
            s.set_selected(selected);
            s.input_error = None;
            s.mode = UiMode::Normal;
        }
        UiMode::CommandEdit => {
            let Some(name) = s.pending_command.take() else {
                clear_command_input(s);
                return;
            };
            s.draft = draft_ops::set_command_string(&s.draft, &name, &value);
            s.input_error = None;
            s.mode = UiMode::Normal;
        }
        _ => s.input = value,
    }
}

fn on_colors_input(e: KeyEvent, s: &mut AppState) {
    s.input_error = None;
    match e.code {
        KeyCode::Esc => clear_command_input(s),
        KeyCode::Backspace => s.pop_input_grapheme(),
        KeyCode::Char(character) => s.push_input(character),
        KeyCode::Enter => submit_command_input(s),
        _ => {}
    }
}

fn remove_command_cache(config_dir: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(config_dir.join("commands")) else {
        return;
    };
    let prefix = format!("{name}-");
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&prefix) && file_name.ends_with(".json") {
            let _ = fs::remove_file(path);
        }
    }
}

fn on_colors_confirm_delete(e: KeyEvent, s: &mut AppState) {
    use crate::tui::app::UiMode;

    let selected = s.selected();
    let pending = s.pending_command.take();
    if e.code == KeyCode::Char('y')
        && let Some(name) = pending
    {
        let (draft, removed_rows) = draft_ops::remove_command(&s.draft, &name);
        s.draft = draft;
        remove_command_cache(&s.config_dir, &name);
        s.command_notice = Some(t(
            s.lang,
            &Key::ColorsHintCmdRemovedRows,
            &[("count", &removed_rows.to_string())],
        ));
        s.set_selected(selected.saturating_sub(1).min(colors_focus(&s.draft).len() - 1));
    }
    s.input.clear();
    s.input_error = None;
    s.mode = UiMode::Normal;
}

fn colors_adjustment(s: &AppState, current: &ColorsFocus, direction: i8) -> Option<Config> {
    match current {
        ColorsFocus::Depth => Some(draft_ops::cycle_color_depth(&s.draft, direction)),
        ColorsFocus::SegmentFg(segment) => {
            Some(draft_ops::cycle_segment_fg(&s.draft, segment, direction))
        }
        ColorsFocus::GaugeWidth => Some(draft_ops::adjust_gauge_bar_width(&s.draft, direction)),
        ColorsFocus::GaugeWarnPct => Some(draft_ops::adjust_gauge_warn_pct(&s.draft, direction)),
        ColorsFocus::GaugeHotPct => Some(draft_ops::adjust_gauge_hot_pct(&s.draft, direction)),
        ColorsFocus::GaugeColor(state) => {
            Some(draft_ops::cycle_gauge_color(&s.draft, state, direction))
        }
        ColorsFocus::PomodoroWorkMin => {
            Some(draft_ops::adjust_pomodoro_work_min(&s.draft, direction))
        }
        ColorsFocus::UsageRefreshSec => {
            Some(draft_ops::adjust_usage_refresh_sec(&s.draft, direction))
        }
        ColorsFocus::CmdFg(name) => Some(draft_ops::cycle_segment_fg(
            &s.draft,
            &format!("cmd:{name}"),
            direction,
        )),
        ColorsFocus::CmdTimeoutMs(name)
        | ColorsFocus::CmdTtlSec(name)
        | ColorsFocus::CmdMaxWidth(name) => {
            let (key, default, step, min, max) = match current {
                ColorsFocus::CmdTimeoutMs(_) => ("timeoutMs", 1_000, 100, 100, 10_000),
                ColorsFocus::CmdTtlSec(_) => ("ttlSec", 5, 1, 1, 3_600),
                ColorsFocus::CmdMaxWidth(_) => ("maxWidth", 24, 1, 8, 80),
                _ => unreachable!(),
            };
            Some(draft_ops::adjust_command_int(
                &s.draft, name, key, default, step, min, max, direction,
            ))
        }
        ColorsFocus::CmdPreserveColors(name) => Some(draft_ops::toggle_command_bool(
            &s.draft,
            name,
            "preserveColors",
            false,
        )),
        ColorsFocus::DirPathDepth => Some(draft_ops::adjust_dir_path_depth(&s.draft, direction)),
        ColorsFocus::NerdFont => {
            let mut draft = s.draft.clone();
            let enabled = draft
                .0
                .get("nerdFont")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            draft.0.insert("nerdFont".into(), Value::Bool(!enabled));
            Some(draft)
        }
        ColorsFocus::CmdAdd | ColorsFocus::CmdCommand(_) => None,
    }
}

fn on_colors_normal(e: KeyEvent, s: &mut AppState) -> Action {
    use crate::tui::app::UiMode;

    if back(e, s) {
        return Action::Back;
    }
    let focus = colors_focus(&s.draft);
    let current = focus[s.selected().min(focus.len() - 1)].clone();
    match e.code {
        KeyCode::Char('a') => {
            s.input.clear();
            s.input_error = None;
            s.pending_command = None;
            s.mode = UiMode::CommandName;
        }
        KeyCode::Enter => {
            if let ColorsFocus::CmdCommand(name) = current {
                let command = configured_commands(&s.draft)
                    .get(&name)
                    .and_then(|spec| protocol::clean_text(Some(&spec.command)))
                    .unwrap_or_default();
                s.input = command;
                s.input_error = None;
                s.pending_command = Some(name);
                s.mode = UiMode::CommandEdit;
            }
        }
        KeyCode::Char('d') => {
            if let ColorsFocus::CmdFg(name) = current {
                s.input_error = None;
                s.pending_command = Some(name);
                s.mode = UiMode::ConfirmDeleteCommand;
            }
        }
        KeyCode::Up => move_focus(s, focus.len(), -1),
        KeyCode::Down => move_focus(s, focus.len(), 1),
        KeyCode::Left | KeyCode::Right => {
            let direction = if e.code == KeyCode::Left { -1 } else { 1 };
            if let Some(next) = colors_adjustment(s, &current, direction) {
                s.draft = next;
            }
        }
        _ => {}
    }
    Action::Redraw
}

impl Screen for ColorsThemeScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::MenuColorsTheme, &[]));
        let gauge = s.draft.0.get("gauge");
        let focus = colors_focus(&s.draft);
        let separators = colors_separators(&s.draft);
        let command_count = configured_commands(&s.draft).len();
        let commands_empty = command_count == 0;
        let selected = s.selected().min(focus.len() - 1);
        let mut rows = Vec::new();
        for (index, item) in focus.iter().enumerate() {
            if separators.contains(&index) {
                if index > 27
                    && index < 27 + 6 * command_count
                    && (index - 27) % 6 == 0
                {
                    rows.push(Line::default());
                } else {
                    rows.push(Line::from(Span::styled(
                        "────────────────",
                        Style::default().fg(crate::tui::screens::common::DIM),
                    )));
                }
            }
            let marker = if index == selected { "▸" } else { " " };
            rows.push(match item {
                ColorsFocus::Depth => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &Key::ColorsDepth,
                        &[(
                            "depth",
                            s.draft
                                .0
                                .get("colorDepth")
                                .and_then(Value::as_str)
                                .unwrap_or("auto")
                        )]
                    )
                )),
                ColorsFocus::SegmentFg(segment) => {
                    let hex = draft_ops::effective_segment_fg(&s.draft, segment);
                    let label = draft_ops::named_color(&hex)
                        .map_or_else(|| hex.clone(), |(_, name)| name.into());
                    Line::from(vec![
                        Span::raw(format!(
                            "{marker} {}",
                            t(
                                s.lang,
                                &Key::ColorsSegmentForeground,
                                &[("segment", segment)]
                            )
                        )),
                        Span::styled(label, Style::default().fg(hex_color(&hex).unwrap_or(TEXT))),
                    ])
                }
                ColorsFocus::GaugeWidth => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &Key::ColorsGaugeWidth,
                        &[(
                            "value",
                            &gauge
                                .and_then(|gauge| gauge.get("barWidth"))
                                .and_then(Value::as_i64)
                                .unwrap_or(20)
                                .to_string()
                        )]
                    )
                )),
                ColorsFocus::GaugeWarnPct => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &Key::ColorsGaugeWarnPct,
                        &[(
                            "value",
                            &gauge
                                .and_then(|gauge| gauge.get("warnPct"))
                                .and_then(Value::as_i64)
                                .unwrap_or(65)
                                .to_string()
                        )]
                    )
                )),
                ColorsFocus::GaugeHotPct => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &Key::ColorsGaugeHotPct,
                        &[(
                            "value",
                            &gauge
                                .and_then(|gauge| gauge.get("hotPct"))
                                .and_then(Value::as_i64)
                                .unwrap_or(85)
                                .to_string()
                        )]
                    )
                )),
                ColorsFocus::GaugeColor(state) => {
                    let hex = draft_ops::effective_gauge_color(&s.draft, state);
                    let label = draft_ops::named_color(&hex)
                        .map_or_else(|| hex.clone(), |(_, name)| name.into());
                    Line::from(vec![
                        Span::raw(format!(
                            "{marker} {} ",
                            t(s.lang, &Key::ColorsGaugeColorLabel, &[("state", state)])
                        )),
                        Span::styled(label, Style::default().fg(hex_color(&hex).unwrap_or(TEXT))),
                    ])
                }
                ColorsFocus::PomodoroWorkMin => integer_setting_line(
                    s,
                    marker,
                    &Key::ColorsPomodoroWorkMin,
                    pomodoro_work_min(s),
                ),
                ColorsFocus::UsageRefreshSec => integer_setting_line(
                    s,
                    marker,
                    &Key::ColorsUsageRefreshSec,
                    usage_refresh_sec(s),
                ),
                ColorsFocus::CmdFg(name) => {
                    let segment = format!("cmd:{name}");
                    let hex = draft_ops::effective_segment_fg(&s.draft, &segment);
                    let label = draft_ops::named_color(&hex)
                        .map_or_else(|| hex.clone(), |(_, color_name)| color_name.into());
                    Line::from(vec![
                        Span::raw(format!("{marker} ")),
                        Span::styled(
                            segment,
                            Style::default()
                                .fg(ACCENT)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(t(
                            s.lang,
                            &Key::ColorsSegmentForeground,
                            &[("segment", "")],
                        )),
                        Span::styled(label, Style::default().fg(hex_color(&hex).unwrap_or(TEXT))),
                    ])
                }
                ColorsFocus::CmdCommand(name) => {
                    let command = s
                        .draft
                        .0
                        .get("commands")
                        .and_then(Value::as_object)
                        .and_then(|commands| commands.get(name))
                        .and_then(Value::as_object)
                        .and_then(|command| command.get("command"))
                        .and_then(Value::as_str);
                    let cleaned = protocol::clean_text(command).unwrap_or_default();
                    let prefix = t(s.lang, &Key::ColorsCmdCommand, &[("value", "")]);
                    let available = usize::from(a[1].width)
                        .saturating_sub(display_width(marker) + 1 + display_width(&prefix));
                    let displayed = truncate_with_ellipsis(&cleaned, available);
                    Line::from(format!(
                        "{marker} {}",
                        t(s.lang, &Key::ColorsCmdCommand, &[("value", &displayed)])
                    ))
                }
                ColorsFocus::CmdTimeoutMs(name) => integer_setting_line(
                    s,
                    marker,
                    &Key::ColorsCmdTimeoutMs,
                    draft_ops::command_field_i64(&s.draft, name, "timeoutMs", 1_000),
                ),
                ColorsFocus::CmdTtlSec(name) => integer_setting_line(
                    s,
                    marker,
                    &Key::ColorsCmdTtlSec,
                    draft_ops::command_field_i64(&s.draft, name, "ttlSec", 5),
                ),
                ColorsFocus::CmdMaxWidth(name) => integer_setting_line(
                    s,
                    marker,
                    &Key::ColorsCmdMaxWidth,
                    draft_ops::command_field_i64(&s.draft, name, "maxWidth", 24),
                ),
                ColorsFocus::CmdPreserveColors(name) => {
                    let enabled = s
                        .draft
                        .0
                        .get("commands")
                        .and_then(Value::as_object)
                        .and_then(|commands| commands.get(name))
                        .and_then(Value::as_object)
                        .and_then(|command| command.get("preserveColors"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    Line::from(format!(
                        "{marker} {}",
                        t(
                            s.lang,
                            &Key::ColorsCmdPreserveColors,
                            &[("value", if enabled { "on" } else { "off" })]
                        )
                    ))
                }
                ColorsFocus::CmdAdd => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &if commands_empty {
                            Key::ColorsCmdEmpty
                        } else {
                            Key::ColorsCmdAdd
                        },
                        &[]
                    )
                ))
                .style(Style::default().fg(crate::tui::screens::common::DIM)),
                ColorsFocus::DirPathDepth => Line::from(format!(
                    "{marker} {}",
                    t(
                        s.lang,
                        &Key::ColorsDirPathDepth,
                        &[(
                            "value",
                            &s.draft
                                .0
                                .get("segments")
                                .and_then(|segments| segments.get("dir"))
                                .and_then(|dir| dir.get("pathDepth"))
                                .and_then(Value::as_i64)
                                .unwrap_or(99)
                                .to_string()
                        )]
                    )
                )),
                ColorsFocus::NerdFont => {
                    let enabled = s
                        .draft
                        .0
                        .get("nerdFont")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let value = t(
                        s.lang,
                        &if enabled {
                            Key::ColorsNerdFontOn
                        } else {
                            Key::ColorsNerdFontOff
                        },
                        &[],
                    );
                    Line::from(format!(
                        "{marker} {}",
                        t(s.lang, &Key::ColorsNerdFont, &[("value", &value)])
                    ))
                }
            });
        }
        let input_prompt = command_input_prompt(s);
        let (list_area, input_area) = if input_prompt.is_some() {
            let input_height = 2 + u16::from(s.input_error.is_some());
            let chunks = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(input_height),
            ])
            .split(a[1]);
            (chunks[0], Some(chunks[1]))
        } else {
            (a[1], None)
        };
        f.render_widget(
            scrolled_list(
                rows,
                colors_line_index(&s.draft, selected),
                focus.len() + separators.len(),
                list_area,
            )
            .style(Style::default().fg(TEXT)),
            list_area,
        );
        if let (Some(prompt), Some(area)) = (input_prompt, input_area) {
            let mut lines = crate::tui::screens::common::input_line(&prompt, &s.input, area.width);
            if let Some(error) = s.input_error {
                lines.push(Line::from(Span::styled(
                    t(s.lang, &error, &[]),
                    Style::default().fg(ERROR),
                )));
            }
            f.render_widget(Paragraph::new(lines), area);
        }
        let footer_text = if s.mode == crate::tui::app::UiMode::ConfirmDeleteCommand {
            t(
                s.lang,
                &Key::ColorsHintCmdDelete,
                &[(
                    "name",
                    s.pending_command.as_deref().unwrap_or_default(),
                )],
            )
        } else if let Some(notice) = &s.command_notice {
            notice.clone()
        } else {
            let mut footer_parts = vec![
                match &focus[selected] {
                    ColorsFocus::SegmentFg(segment) => segment_fg_hint(&s.draft, segment),
                    ColorsFocus::CmdFg(name) => {
                        segment_fg_hint(&s.draft, &format!("cmd:{name}"))
                    }
                    ColorsFocus::GaugeColor(state) => {
                        let hex = draft_ops::effective_gauge_color(&s.draft, state);
                        draft_ops::named_color(&hex)
                            .map_or("(-/58)".into(), |(index, _)| format!("({index}/58)"))
                    }
                    _ => "(-/58)".into(),
                },
                t(s.lang, &Key::MainMenuHintMove, &[]),
                t(s.lang, &Key::ColorsHintCycleAdjust, &[]),
            ];
            match &focus[selected] {
                ColorsFocus::CmdFg(_) => {
                    footer_parts.push(t(s.lang, &Key::ColorsHintCmdDeleteKey, &[]));
                    footer_parts.push(t(s.lang, &Key::ColorsHintCmdAddKey, &[]));
                }
                ColorsFocus::CmdCommand(_) => {
                    footer_parts.push(t(s.lang, &Key::ColorsHintCmdEditKey, &[]));
                    footer_parts.push(t(s.lang, &Key::ColorsHintCmdAddKey, &[]));
                }
                ColorsFocus::CmdTimeoutMs(_)
                | ColorsFocus::CmdTtlSec(_)
                | ColorsFocus::CmdMaxWidth(_)
                | ColorsFocus::CmdPreserveColors(_)
                | ColorsFocus::CmdAdd => {
                    footer_parts.push(t(s.lang, &Key::ColorsHintCmdAddKey, &[]));
                }
                _ => {}
            }
            footer_parts.join(" ")
        };
        footer(f, a[2], &footer_text);
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        s.command_notice = None;
        match s.mode {
            crate::tui::app::UiMode::CommandName
            | crate::tui::app::UiMode::CommandString
            | crate::tui::app::UiMode::CommandEdit => {
                on_colors_input(e, s);
                Action::Redraw
            }
            crate::tui::app::UiMode::ConfirmDeleteCommand => {
                on_colors_confirm_delete(e, s);
                Action::Redraw
            }
            _ => on_colors_normal(e, s),
        }
    }
}
impl Screen for TemplatesScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::MenuTemplates, &[]));
        let templates_dir = s.config_dir.join("templates");
        let templates = template_names(&templates_dir);
        let prompt = match s.mode {
            crate::tui::app::UiMode::TemplateSaveName => {
                t(s.lang, &Key::TemplatesSaveNameLabel, &[])
            }
            crate::tui::app::UiMode::TemplateLoadName => {
                t(s.lang, &Key::TemplatesLoadNameLabel, &[])
            }
            crate::tui::app::UiMode::TemplateExportPath => {
                t(s.lang, &Key::TemplatesExportPathLabel, &[])
            }
            crate::tui::app::UiMode::TemplateImportPath => {
                t(s.lang, &Key::TemplatesImportPathLabel, &[])
            }
            crate::tui::app::UiMode::TemplateImportName => {
                t(s.lang, &Key::TemplatesImportNameLabel, &[])
            }
            crate::tui::app::UiMode::TemplateSaveOverwrite
            | crate::tui::app::UiMode::TemplateImportOverwrite
            | crate::tui::app::UiMode::TemplateExportOverwrite => t(
                s.lang,
                &Key::TemplatesOverwriteConfirm,
                &[("path", &s.pending_path.display().to_string())],
            ),
            crate::tui::app::UiMode::TemplateDeleteConfirm => t(
                s.lang,
                &Key::TemplatesDeleteConfirm,
                &[("name", &s.pending_name)],
            ),
            _ => templates
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    format!(
                        "{} {name}{}",
                        if index == s.selected() { ">" } else { " " },
                        if index < 3 {
                            t(s.lang, &Key::TemplatesBuiltinSuffix, &[])
                        } else {
                            String::new()
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let text_input = matches!(
            s.mode,
            crate::tui::app::UiMode::TemplateSaveName
                | crate::tui::app::UiMode::TemplateLoadName
                | crate::tui::app::UiMode::TemplateExportPath
                | crate::tui::app::UiMode::TemplateImportPath
                | crate::tui::app::UiMode::TemplateImportName
        );
        if text_input {
            f.render_widget(
                Paragraph::new(crate::tui::screens::common::input_line(
                    &prompt,
                    &s.input,
                    a[1].width,
                )),
                a[1],
            );
        } else {
            f.render_widget(
                Paragraph::new(prompt).style(Style::default().fg(TEXT)),
                a[1],
            );
        }
        if let Some(err) = &s.error {
            footer_error(f, a[2], err);
        } else {
            footer(
                f,
                a[2],
                &[
                    t(s.lang, &Key::TemplatesHintSave, &[]),
                    t(s.lang, &Key::TemplatesHintLoad, &[]),
                    t(s.lang, &Key::TemplatesHintExport, &[]),
                    t(s.lang, &Key::TemplatesHintImport, &[]),
                    t(s.lang, &Key::TemplatesHintDelete, &[]),
                    t(s.lang, &Key::TemplatesHintBackToMenu, &[]),
                ]
                .join(" "),
            );
        }
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        use crate::tui::app::UiMode;
        let templates = s.config_dir.join("templates");
        if e.code == KeyCode::Esc {
            if s.mode != UiMode::Normal {
                s.mode = UiMode::Normal;
                clear_template_pending(s);
                return Action::Redraw;
            }
            if back(e, s) {
                return Action::Back;
            }
        }
        if !matches!(
            s.mode,
            UiMode::Normal
                | UiMode::TemplateSaveOverwrite
                | UiMode::TemplateImportOverwrite
                | UiMode::TemplateExportOverwrite
                | UiMode::TemplateDeleteConfirm
        ) {
            match e.code {
                KeyCode::Backspace => s.pop_input_grapheme(),
                KeyCode::Char(c) => s.push_input(c),
                KeyCode::Enter => {
                    let value = std::mem::take(&mut s.input);
                    match s.mode {
                        UiMode::TemplateSaveName => {
                            s.pending_name = value;
                            if templates.join(format!("{}.json", s.pending_name)).exists() {
                                s.mode = UiMode::TemplateSaveOverwrite
                            } else {
                                match templates_io::save_template(
                                    &s.pending_name,
                                    &templates_io::draft_to_snapshot(&Value::Object(
                                        s.draft.0.clone(),
                                    )),
                                    &templates,
                                ) {
                                    Ok(()) => {
                                        s.status = Some(t(
                                            s.lang,
                                            &Key::TemplatesSavedAs,
                                            &[("name", &s.pending_name)],
                                        ))
                                    }
                                    Err(err) => s.error = Some(err.to_string()),
                                }
                                clear_template_pending(s);
                                s.mode = UiMode::Normal
                            }
                        }
                        UiMode::TemplateLoadName => {
                            match templates_io::load_template(&value, &templates) {
                                Ok(template) => {
                                    apply_loaded_template(s, template, &value);
                                    s.status =
                                        Some(t(s.lang, &Key::TemplatesLoaded, &[("name", &value)]))
                                }
                                Err(err) => s.error = Some(err.to_string()),
                            };
                            s.mode = UiMode::Normal
                        }
                        UiMode::TemplateExportPath => {
                            s.pending_path = expand_home_path(&value);
                            if s.pending_path.exists() {
                                s.mode = UiMode::TemplateExportOverwrite
                            } else {
                                match templates_io::export_template(
                                    &s.pending_name,
                                    &s.pending_path,
                                    &templates,
                                ) {
                                    Ok(()) => {
                                        s.status = Some(t(
                                            s.lang,
                                            &Key::TemplatesExportedTo,
                                            &[("path", &s.pending_path.display().to_string())],
                                        ))
                                    }
                                    Err(err) => s.error = Some(err.to_string()),
                                };
                                clear_template_pending(s);
                                s.mode = UiMode::Normal
                            }
                        }
                        UiMode::TemplateImportPath => {
                            s.pending_path = expand_home_path(&value);
                            s.mode = UiMode::TemplateImportName
                        }
                        UiMode::TemplateImportName => {
                            s.pending_name = value;
                            if templates.join(format!("{}.json", s.pending_name)).exists() {
                                s.mode = UiMode::TemplateImportOverwrite
                            } else {
                                match templates_io::import_template(
                                    &s.pending_path,
                                    &s.pending_name,
                                    &templates,
                                ) {
                                    Ok(_) => {
                                        s.status = Some(t(
                                            s.lang,
                                            &Key::TemplatesImportedAs,
                                            &[("name", &s.pending_name)],
                                        ))
                                    }
                                    Err(err) => s.error = Some(err.to_string()),
                                };
                                clear_template_pending(s);
                                s.mode = UiMode::Normal
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return Action::Redraw;
        }
        match e.code {
            KeyCode::Up => {
                move_focus(s, template_names(&templates).len().max(1), -1);
            }
            KeyCode::Down => {
                move_focus(s, template_names(&templates).len().max(1), 1);
            }
            KeyCode::Char('s') => {
                clear_template_pending(s);
                s.mode = UiMode::TemplateSaveName;
            }
            KeyCode::Char('l') => {
                clear_template_pending(s);
                let names = template_names(&templates);
                if let Some(name) = names.get(s.selected()) {
                    if s.selected() < 3 {
                        s.draft
                            .0
                            .insert("activeTemplate".into(), Value::String(name.clone()));
                        s.status = Some(t(s.lang, &Key::TemplatesLoaded, &[("name", name)]));
                    } else {
                        match templates_io::load_template(name, &templates) {
                            Ok(template) => {
                                apply_loaded_template(s, template, name);
                                s.status =
                                    Some(t(s.lang, &Key::TemplatesLoaded, &[("name", name)]));
                            }
                            Err(error) => {
                                s.error = Some(t(
                                    s.lang,
                                    &Key::TemplatesLoadFailed,
                                    &[("message", &error.to_string())],
                                ))
                            }
                        }
                    }
                }
            }
            KeyCode::Char('e') => {
                clear_template_pending(s);
                if let Some(name) = template_names(&templates).get(s.selected()) {
                    s.pending_name = name.clone();
                    s.input = format!("~/Desktop/{name}.json");
                    s.mode = UiMode::TemplateExportPath;
                }
            }
            KeyCode::Char('i') => {
                clear_template_pending(s);
                s.input = "~/Desktop/".into();
                s.mode = UiMode::TemplateImportPath;
            }
            KeyCode::Char('d') => {
                if let Some(name) = template_names(&templates).get(s.selected()) {
                    if s.selected() >= 3 {
                        clear_template_pending(s);
                        s.pending_name = name.clone();
                        s.mode = UiMode::TemplateDeleteConfirm;
                    }
                }
            }
            KeyCode::Char('y') if s.mode == UiMode::TemplateSaveOverwrite => {
                match templates_io::save_template(
                    &s.pending_name,
                    &templates_io::draft_to_snapshot(&Value::Object(s.draft.0.clone())),
                    &templates,
                ) {
                    Ok(()) => {
                        s.status = Some(t(
                            s.lang,
                            &Key::TemplatesSavedAs,
                            &[("name", &s.pending_name)],
                        ))
                    }
                    Err(err) => s.error = Some(err.to_string()),
                }
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('y') if s.mode == UiMode::TemplateImportOverwrite => {
                match templates_io::import_template(&s.pending_path, &s.pending_name, &templates) {
                    Ok(_) => {
                        s.status = Some(t(
                            s.lang,
                            &Key::TemplatesImportedAs,
                            &[("name", &s.pending_name)],
                        ))
                    }
                    Err(err) => s.error = Some(err.to_string()),
                }
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('y') if s.mode == UiMode::TemplateExportOverwrite => {
                match templates_io::export_template(&s.pending_name, &s.pending_path, &templates) {
                    Ok(()) => {
                        s.status = Some(t(
                            s.lang,
                            &Key::TemplatesExportedTo,
                            &[("path", &s.pending_path.display().to_string())],
                        ))
                    }
                    Err(err) => s.error = Some(err.to_string()),
                }
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('y') if s.mode == UiMode::TemplateDeleteConfirm => {
                match templates_io::delete_template(&s.pending_name, &templates) {
                    Ok(()) => {
                        s.status = Some(t(
                            s.lang,
                            &Key::TemplatesDeleted,
                            &[("name", &s.pending_name)],
                        ));
                        let count = template_names(&templates).len();
                        s.set_selected(s.selected().min(count.saturating_sub(1)));
                    }
                    Err(err) => s.error = Some(err.to_string()),
                }
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('n')
                if matches!(
                    s.mode,
                    UiMode::TemplateSaveOverwrite | UiMode::TemplateImportOverwrite
                ) =>
            {
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('n') if s.mode == UiMode::TemplateExportOverwrite => {
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            KeyCode::Char('n') if s.mode == UiMode::TemplateDeleteConfirm => {
                clear_template_pending(s);
                s.mode = UiMode::Normal
            }
            _ => {}
        }
        Action::Redraw
    }
}
impl Screen for SettingsInstallScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::MenuSettingsInstall, &[]));
        let (body, hint) = if s.mode == crate::tui::app::UiMode::SettingsWriteConfirm {
            (
                s.input.clone(),
                format!(
                    "{} {}",
                    t(s.lang, &Key::SettingsWriteHintConfirmWrite, &[]),
                    t(s.lang, &Key::SettingsWriteHintCancel, &[])
                ),
            )
        } else if s.mode == crate::tui::app::UiMode::SettingsBinInstallConfirm {
            let path = s.pending_path.display().to_string();
            let prompt = if s.pending_path.exists() {
                Key::SettingsInstallBinOverwriteConfirm
            } else {
                Key::SettingsInstallBinInstallConfirm
            };
            (
                t(s.lang, &prompt, &[("path", &path)]),
                format!(
                    "{} {}",
                    t(s.lang, &Key::SettingsWriteHintConfirmWrite, &[]),
                    t(s.lang, &Key::SettingsWriteHintCancel, &[])
                ),
            )
        } else {
            // Re-read settings.json for every render.  This deliberately makes
            // the status below reflect a completed `w` write immediately.
            let (bin, status_line, subagent_line) = settings_install_status();
            let mut lines = vec![match &bin {
                Some(path) => t(
                    s.lang,
                    &Key::SettingsInstallBinInstalled,
                    &[("path", &path.display().to_string())],
                ),
                None => t(
                    s.lang,
                    &Key::SettingsInstallBinNotInstalled,
                    &[("cmd", "phosphorpulse")],
                ),
            }];
            lines.push(match status_line {
                Some(command) => t(
                    s.lang,
                    &Key::SettingsInstallStatusLineSet,
                    &[("cmd", &command)],
                ),
                None => t(s.lang, &Key::SettingsInstallStatusLineUnset, &[]),
            });
            lines.push(match subagent_line {
                Some(command) => t(
                    s.lang,
                    &Key::SettingsInstallSubagentStatusLineSet,
                    &[("cmd", &command)],
                ),
                None => t(s.lang, &Key::SettingsInstallSubagentStatusLineUnset, &[]),
            });
            lines.push(match codex_settings_status() {
                Some(config) => {
                    let item_count = config.status_line.len().to_string();
                    let colors = t(
                        s.lang,
                        &if config.status_line_use_colors {
                            Key::SettingsInstallCodexColorsOn
                        } else {
                            Key::SettingsInstallCodexColorsOff
                        },
                        &[],
                    );
                    t(
                        s.lang,
                        &Key::SettingsInstallCodexConfigured,
                        &[
                            ("items", &item_count),
                            ("colors", &colors),
                            ("theme", &config.theme),
                        ],
                    )
                }
                None => t(s.lang, &Key::SettingsInstallCodexNotConfigured, &[]),
            });
            if bin.is_some() && bin_install::bin_on_path().is_none() {
                lines.push(t(s.lang, &Key::SettingsInstallBinPathHint, &[]));
            }
            if let Some(status) = &s.status {
                lines.push(status.clone());
            }
            if let Some(error) = &s.error {
                lines.push(error.clone());
            }
            (
                lines.join("\n"),
                [
                    t(s.lang, &Key::SettingsInstallHintRewrite, &[]),
                    t(
                        s.lang,
                        &if bin.is_some() {
                            Key::SettingsInstallHintReinstallBin
                        } else {
                            Key::SettingsInstallHintInstallBin
                        },
                        &[],
                    ),
                ]
                .into_iter()
                .filter(|hint| !hint.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
            )
        };
        f.render_widget(Paragraph::new(body).style(Style::default().fg(TEXT)), a[1]);
        footer(f, a[2], &hint);
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        use crate::tui::app::UiMode;

        if s.mode == UiMode::SettingsWriteConfirm {
            match e.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if let Err(err) = settings_writer::write_statusline_blocks(None) {
                        s.error = Some(err.to_string());
                    } else {
                        s.status = Some(t(s.lang, &Key::SettingsWriteDone, &[]));
                    }
                    s.input.clear();
                    s.mode = UiMode::Normal;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    s.input.clear();
                    s.mode = UiMode::Normal;
                }
                _ => {}
            }
            return Action::Redraw;
        }
        if s.mode == UiMode::SettingsBinInstallConfirm {
            match e.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let path = s.pending_path.clone();
                    match bin_install::install_global_bin(&path) {
                        Ok(()) => {
                            s.error = None;
                            s.status = Some(t(
                                s.lang,
                                &Key::BinInstallInstalledTo,
                                &[("path", &path.display().to_string())],
                            ));
                        }
                        Err(error) => {
                            s.error = Some(t(
                                s.lang,
                                &Key::SettingsInstallBinInstallFailed,
                                &[
                                    ("path", &path.display().to_string()),
                                    ("reason", &error.to_string()),
                                ],
                            ))
                        }
                    }
                    s.pending_path.clear();
                    s.mode = UiMode::Normal;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    s.pending_path.clear();
                    s.mode = UiMode::Normal;
                }
                _ => {}
            }
            return Action::Redraw;
        }
        if back(e, s) {
            return Action::Back;
        }
        if e.code == KeyCode::Char('w') {
            match settings_writer::statusline_blocks_diff(None) {
                Ok(diff) => {
                    s.input = diff;
                    s.mode = UiMode::SettingsWriteConfirm;
                }
                Err(err) => s.error = Some(err.to_string()),
            }
        }
        if e.code == KeyCode::Char('b') {
            if let Some(path) = bin_install::detected_bin().or_else(bin_install::local_bin_path) {
                s.error = None;
                s.status = None;
                s.pending_path = path;
                s.mode = UiMode::SettingsBinInstallConfirm;
            } else {
                s.error = Some(t(s.lang, &Key::SettingsInstallBinHomeMissing, &[]));
            }
        }
        Action::Redraw
    }
}
impl Screen for SaveExitScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::MenuSaveExit, &[]));
        f.render_widget(
            Paragraph::new(t(s.lang, &Key::SaveExitConfirmPrompt, &[]))
                .style(Style::default().fg(ACCENT)),
            a[1],
        );
        if let Some(error) = &s.error {
            f.render_widget(
                Paragraph::new(t(s.lang, &Key::SaveExitError, &[("message", error)]))
                    .style(Style::default().fg(ERROR)),
                a[2],
            );
        } else {
            footer(f, a[2], &t(s.lang, &Key::SaveExitHintSaveAndExit, &[]));
        }
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if back(e, s) {
            return Action::Back;
        }
        if matches!(e.code, KeyCode::Char('y') | KeyCode::Enter) {
            if let Err(err) = crate::tui::app::write_own_config(&s.draft, &s.config_dir) {
                s.error = Some(err.to_string());
                return Action::Redraw;
            }
            let _ = settings_writer::maybe_rewrite_claude_settings(&s.draft);
            return Action::Quit;
        }
        Action::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::themes::builtin;

    /// REQ-04 / S-04: picker inventories include the new main IDs without changing built-in rows.
    #[test]
    fn test_s04_picker_ids_and_builtin_rows() {
        let ids = main_segment_ids(&Config::defaults());
        assert_eq!(ids.len(), 19);
        assert_eq!(
            ids[..14].iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "model", "effort", "git", "dir", "ctx", "limit5h", "limit7d", "node",
                "python", "version", "cost", "burn", "pomodoro", "flex",
            ]
        );
        assert_eq!(
            ids[14..].iter().map(String::as_str).collect::<Vec<_>>(),
            ["session", "fastMode", "outputStyle", "thinking", "limitModel"]
        );
        assert_eq!(
            SUBAGENT_SEGMENT_IDS,
            [
                "name",
                "desc",
                "model",
                "ctx",
                "elapsed",
                "effort",
                "tokenCount",
            ]
        );

        let new_ids = ["session", "fastMode", "outputStyle", "thinking"];
        for theme_name in ["matrix-tron", "solarized-dark", "solarized-light"] {
            let theme = builtin(theme_name);
            assert!(theme.rows.iter().all(|row| row
                .segments
                .iter()
                .all(|segment| !new_ids.contains(&segment.as_str()))));
        }
    }

    /// REQ-07 / S-08: Colors & Themes lists the new segment foreground controls.
    #[test]
    fn test_s08_colors_focus_lists_new_segments() {
        let d = Config::defaults();
        assert_eq!(colors_focus(&d).len(), 30);

        let focus = colors_focus(&d);
        let pomodoro = focus
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::SegmentFg("pomodoro")))
            .expect("pomodoro foreground focus");
        assert!(matches!(
            focus.get(pomodoro + 1),
            Some(ColorsFocus::SegmentFg("session"))
        ));
        assert!(matches!(
            focus.get(pomodoro + 2),
            Some(ColorsFocus::SegmentFg("fastMode"))
        ));
        assert!(matches!(
            focus.get(pomodoro + 3),
            Some(ColorsFocus::SegmentFg("outputStyle"))
        ));
        assert!(matches!(
            focus.get(pomodoro + 4),
            Some(ColorsFocus::SegmentFg("thinking"))
        ));
        assert!(matches!(
            focus.get(pomodoro + 5),
            Some(ColorsFocus::SegmentFg("limitModel"))
        ));
        assert!(matches!(
            focus.get(pomodoro + 6),
            Some(ColorsFocus::GaugeWidth)
        ));
        assert_eq!(colors_separators(&d), [19, 25, 27, 28]);

        let active_template = d
            .0
            .get("activeTemplate")
            .and_then(Value::as_str)
            .expect("defaults active template");
        let theme = builtin(active_template);
        for (id, palette_key) in [
            ("session", "dir"),
            ("fastMode", "warn"),
            ("outputStyle", "version"),
            ("thinking", "warn"),
        ] {
            let actual = crate::tui::draft_ops::effective_segment_fg(&d, id);
            assert_eq!(actual.as_str(), theme.palette[palette_key].as_str());
        }
    }

    /// REQ-08 / S-09: list scrolling keeps every selected item visible.
    #[test]
    fn test_s09_list_scroll_offsets() {
        use crate::tui::screens::common::list_scroll_offset;

        let d = Config::defaults();
        assert_eq!(list_scroll_offset(0, 18, 12), 0);
        assert_eq!(list_scroll_offset(5, 18, 12), 0);
        assert_eq!(list_scroll_offset(11, 18, 12), 0);
        assert_eq!(list_scroll_offset(12, 18, 12), 1);
        assert_eq!(list_scroll_offset(17, 18, 12), 6);
        assert_eq!(list_scroll_offset(30, 18, 12), 6);
        assert_eq!(list_scroll_offset(3, 5, 12), 0);
        assert_eq!(list_scroll_offset(17, 18, 0), 0);

        for (focus, expected) in [
            (0, 0),
            (17, 17),
            (18, 18),
            (19, 20),
            (23, 24),
            (24, 25),
            (25, 27),
            (26, 28),
            (27, 30),
            (28, 32),
        ] {
            assert_eq!(colors_line_index(&d, focus), expected);
        }
        assert_eq!(30 + 4, 34);
    }

    /// REQ-05 / REQ-07 / S-05: limitModel and usage refresh controls occupy fixed TUI slots.
    #[test]
    fn test_s05_limit_model_tui_constants() {
        let d = Config::defaults();
        let main_segment_ids = main_segment_ids(&Config::defaults());
        assert_eq!(main_segment_ids.len(), 19);
        assert_eq!(main_segment_ids[18], "limitModel");
        assert_eq!(colors_focus(&d).len(), 30);

        let focuses = colors_focus(&d);

        let thinking = focuses
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::SegmentFg("thinking")))
            .expect("thinking foreground focus");
        assert!(matches!(
            focuses.get(thinking + 1),
            Some(ColorsFocus::SegmentFg("limitModel"))
        ));
        assert!(matches!(
            focuses.get(thinking + 2),
            Some(ColorsFocus::GaugeWidth)
        ));

        let pomodoro_work = focuses
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::PomodoroWorkMin))
            .expect("pomodoro work minutes focus");
        assert!(matches!(
            focuses.get(pomodoro_work + 1),
            Some(ColorsFocus::UsageRefreshSec)
        ));
        assert!(matches!(
            focuses.get(pomodoro_work + 2),
            Some(ColorsFocus::CmdAdd)
        ));
        assert!(matches!(
            focuses.get(pomodoro_work + 3),
            Some(ColorsFocus::DirPathDepth)
        ));

        assert_eq!(colors_separators(&d), [19, 25, 27, 28]);
        assert_eq!(colors_focus(&d).len() + 4, 34);

        assert_eq!(
            crate::tui::draft_ops::effective_segment_fg(&d, "limitModel"),
            crate::tui::draft_ops::effective_segment_fg(&d, "limit7d")
        );
    }

    /// REQ-04 / REQ-05 / S-06
    #[test]
    fn test_s06_dynamic_focus_lists() {
        let d0 = Config::defaults();
        let mut d2 = d0.clone();
        d2.0.insert(
            "commands".into(),
            serde_json::json!({
                "zeta": {"command": "a"},
                "alpha": {"command": "b"}
            }),
        );

        let main0 = main_segment_ids(&d0);
        assert_eq!(main0.len(), 19);
        assert_eq!(main0[18], "limitModel");

        let main2 = main_segment_ids(&d2);
        assert_eq!(main2.len(), 21);
        assert_eq!(main2[19], "cmd:alpha");
        assert_eq!(main2[20], "cmd:zeta");

        let focus0 = colors_focus(&d0);
        assert_eq!(focus0.len(), 30);
        assert_eq!(focus0[27], ColorsFocus::CmdAdd);
        assert_eq!(focus0[28], ColorsFocus::DirPathDepth);
        assert_eq!(focus0[29], ColorsFocus::NerdFont);
        assert_eq!(colors_separators(&d0), [19, 25, 27, 28]);

        let focus2 = colors_focus(&d2);
        assert_eq!(focus2.len(), 42);
        assert_eq!(focus2[27], ColorsFocus::CmdFg("alpha".into()));
        assert_eq!(focus2[33], ColorsFocus::CmdFg("zeta".into()));
        assert_eq!(focus2[39], ColorsFocus::CmdAdd);
        assert_eq!(colors_separators(&d2), [19, 25, 27, 33, 40]);

        let active_template = d2
            .0
            .get("activeTemplate")
            .and_then(Value::as_str)
            .expect("defaults active template");
        let theme = builtin(active_template);
        assert_eq!(
            crate::tui::draft_ops::effective_segment_fg(&d2, "cmd:alpha"),
            theme.palette["text"]
        );
    }
}
