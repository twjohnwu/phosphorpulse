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
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

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
const MAIN_SEGMENT_IDS: [&str; 19] = [
    "model", "effort", "git", "dir", "ctx", "limit5h", "limit7d", "node", "python", "version",
    "cost", "burn", "pomodoro", "flex", "session", "fastMode", "outputStyle", "thinking",
    "limitModel",
];
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
#[derive(Clone, Copy)]
enum ColorsFocus {
    Depth,
    SegmentFg(&'static str),
    GaugeWidth,
    GaugeWarnPct,
    GaugeHotPct,
    GaugeColor(&'static str),
    PomodoroWorkMin,
    UsageRefreshSec,
    DirPathDepth,
    NerdFont,
}
const COLORS_FOCUS: [ColorsFocus; 29] = [
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
    ColorsFocus::DirPathDepth,
    ColorsFocus::NerdFont,
];
const COLORS_SEPARATOR_BEFORE: [usize; 3] = [19, 25, 27];

fn colors_line_index(focus: usize) -> usize {
    focus
        + COLORS_SEPARATOR_BEFORE
            .iter()
            .filter(|&&separator| separator <= focus)
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
            let options: &[&str] = match target {
                SegmentPickerTarget::Main => &MAIN_SEGMENT_IDS,
                SegmentPickerTarget::Subagent => &SUBAGENT_SEGMENT_IDS,
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
            let options: &[&str] = match target {
                SegmentPickerTarget::Main => &MAIN_SEGMENT_IDS,
                SegmentPickerTarget::Subagent => &SUBAGENT_SEGMENT_IDS,
            };
            match e.code {
                KeyCode::Esc => s.mode = crate::tui::app::UiMode::Normal,
                KeyCode::Up => move_focus(s, options.len(), -1),
                KeyCode::Down => move_focus(s, options.len(), 1),
                KeyCode::Enter => {
                    let id = options[s.selected().min(options.len() - 1)];
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
                                draft_ops::add_row(&s.draft, vec![id.into()])
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
impl Screen for ColorsThemeScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::MenuColorsTheme, &[]));
        let gauge = s.draft.0.get("gauge");
        let selected = s.selected();
        let mut rows = Vec::new();
        for (index, focus) in COLORS_FOCUS.iter().enumerate() {
            if COLORS_SEPARATOR_BEFORE.contains(&index) {
                rows.push(Line::from(Span::styled(
                    "────────────────",
                    Style::default().fg(crate::tui::screens::common::DIM),
                )));
            }
            let marker = if index == selected { "▸" } else { " " };
            rows.push(match focus {
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
        f.render_widget(
            scrolled_list(
                rows,
                colors_line_index(selected),
                COLORS_FOCUS.len() + COLORS_SEPARATOR_BEFORE.len(),
                a[1],
            )
            .style(Style::default().fg(TEXT)),
            a[1],
        );
        footer(
            f,
            a[2],
            &[
                match COLORS_FOCUS[selected] {
                    ColorsFocus::SegmentFg(segment) => {
                        let hex = draft_ops::effective_segment_fg(&s.draft, segment);
                        draft_ops::named_color(&hex)
                            .map_or("(-/58)".into(), |(index, _)| format!("({index}/58)"))
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
            ]
            .join(" "),
        );
        preview(f, a[3], s)
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if back(e, s) {
            return Action::Back;
        }
        match e.code {
            KeyCode::Up => move_focus(s, COLORS_FOCUS.len(), -1),
            KeyCode::Down => move_focus(s, COLORS_FOCUS.len(), 1),
            KeyCode::Left | KeyCode::Right => {
                let direction = if e.code == KeyCode::Left { -1 } else { 1 };
                s.draft = match COLORS_FOCUS[s.selected()] {
                    ColorsFocus::Depth => draft_ops::cycle_color_depth(&s.draft, direction),
                    ColorsFocus::SegmentFg(segment) => {
                        draft_ops::cycle_segment_fg(&s.draft, segment, direction)
                    }
                    ColorsFocus::GaugeWidth => {
                        draft_ops::adjust_gauge_bar_width(&s.draft, direction)
                    }
                    ColorsFocus::GaugeWarnPct => {
                        draft_ops::adjust_gauge_warn_pct(&s.draft, direction)
                    }
                    ColorsFocus::GaugeHotPct => {
                        draft_ops::adjust_gauge_hot_pct(&s.draft, direction)
                    }
                    ColorsFocus::GaugeColor(state) => {
                        draft_ops::cycle_gauge_color(&s.draft, state, direction)
                    }
                    ColorsFocus::PomodoroWorkMin => {
                        draft_ops::adjust_pomodoro_work_min(&s.draft, direction)
                    }
                    ColorsFocus::UsageRefreshSec => {
                        draft_ops::adjust_usage_refresh_sec(&s.draft, direction)
                    }
                    ColorsFocus::DirPathDepth => {
                        draft_ops::adjust_dir_path_depth(&s.draft, direction)
                    }
                    ColorsFocus::NerdFont => {
                        let mut draft = s.draft.clone();
                        let enabled = draft
                            .0
                            .get("nerdFont")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        draft.0.insert("nerdFont".into(), Value::Bool(!enabled));
                        draft
                    }
                };
            }
            _ => {}
        }
        Action::Redraw
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
        let body = if matches!(
            s.mode,
            crate::tui::app::UiMode::Normal | crate::tui::app::UiMode::TemplateDeleteConfirm
        ) {
            prompt
        } else {
            format!("{prompt}\n\n{}", s.input)
        };
        f.render_widget(Paragraph::new(body).style(Style::default().fg(TEXT)), a[1]);
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
        assert_eq!(MAIN_SEGMENT_IDS.len(), 19);
        assert_eq!(
            &MAIN_SEGMENT_IDS[..14],
            &[
                "model", "effort", "git", "dir", "ctx", "limit5h", "limit7d", "node",
                "python", "version", "cost", "burn", "pomodoro", "flex",
            ]
        );
        assert_eq!(
            MAIN_SEGMENT_IDS.get(14..),
            Some(
                ["session", "fastMode", "outputStyle", "thinking", "limitModel"].as_slice()
            )
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
        assert_eq!(COLORS_FOCUS.len(), 29);

        let pomodoro = COLORS_FOCUS
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::SegmentFg("pomodoro")))
            .expect("pomodoro foreground focus");
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 1),
            Some(ColorsFocus::SegmentFg("session"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 2),
            Some(ColorsFocus::SegmentFg("fastMode"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 3),
            Some(ColorsFocus::SegmentFg("outputStyle"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 4),
            Some(ColorsFocus::SegmentFg("thinking"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 5),
            Some(ColorsFocus::SegmentFg("limitModel"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro + 6),
            Some(ColorsFocus::GaugeWidth)
        ));
        assert_eq!(COLORS_SEPARATOR_BEFORE, [19, 25, 27]);

        let draft = crate::config::model::Config::defaults();
        let theme = builtin("matrix-tron");
        for (id, palette_key) in [
            ("session", "dir"),
            ("fastMode", "warn"),
            ("outputStyle", "version"),
            ("thinking", "warn"),
        ] {
            let actual = crate::tui::draft_ops::effective_segment_fg(&draft, id);
            assert_eq!(actual.as_str(), theme.palette[palette_key].as_str());
        }
    }

    /// REQ-08 / S-09: list scrolling keeps every selected item visible.
    #[test]
    fn test_s09_list_scroll_offsets() {
        use crate::tui::screens::common::list_scroll_offset;

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
            (28, 31),
        ] {
            assert_eq!(colors_line_index(focus), expected);
        }
        assert_eq!(COLORS_FOCUS.len() + COLORS_SEPARATOR_BEFORE.len(), 32);
    }

    /// REQ-05 / REQ-07 / S-05: limitModel and usage refresh controls occupy fixed TUI slots.
    #[test]
    fn test_s05_limit_model_tui_constants() {
        assert_eq!(MAIN_SEGMENT_IDS.len(), 19);
        let main_segment_ids: &[&str] = &MAIN_SEGMENT_IDS;
        assert_eq!(main_segment_ids[18], "limitModel");
        assert_eq!(COLORS_FOCUS.len(), 29);

        let thinking = COLORS_FOCUS
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::SegmentFg("thinking")))
            .expect("thinking foreground focus");
        assert!(matches!(
            COLORS_FOCUS.get(thinking + 1),
            Some(ColorsFocus::SegmentFg("limitModel"))
        ));
        assert!(matches!(
            COLORS_FOCUS.get(thinking + 2),
            Some(ColorsFocus::GaugeWidth)
        ));

        let pomodoro_work = COLORS_FOCUS
            .iter()
            .position(|focus| matches!(focus, ColorsFocus::PomodoroWorkMin))
            .expect("pomodoro work minutes focus");
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro_work + 1),
            Some(ColorsFocus::UsageRefreshSec)
        ));
        assert!(matches!(
            COLORS_FOCUS.get(pomodoro_work + 2),
            Some(ColorsFocus::DirPathDepth)
        ));

        assert_eq!(COLORS_SEPARATOR_BEFORE, [19, 25, 27]);
        assert_eq!(COLORS_FOCUS.len() + 3, 32);

        let draft = crate::config::model::Config::defaults();
        assert_eq!(
            crate::tui::draft_ops::effective_segment_fg(&draft, "limitModel"),
            crate::tui::draft_ops::effective_segment_fg(&draft, "limit7d")
        );
    }
}
