use crate::tui::{
    app::{AppState, ScreenId, UiMode},
    codex::{
        config_io::{self, KNOWN_STATUS_LINE_IDS},
        preview::render_preview_line,
    },
    i18n::{Key, t},
    screens::{
        Action, Screen,
        common::{DIM, TEXT, areas, footer, footer_error, header, move_focus, too_small},
    },
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{fs, path::PathBuf};
pub struct CodexSettingsScreen {
    move_mode: bool,
}
impl CodexSettingsScreen {
    pub fn new() -> Self {
        Self { move_mode: false }
    }
}
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
fn config_path() -> PathBuf {
    home().join(".codex/config.toml")
}
pub fn load_config(state: &mut AppState) {
    let path = config_path();
    match config_io::read_config(&path) {
        Ok(snapshot) => {
            state.codex_edit = Some(snapshot.config.clone());
            state.codex_config = Some(snapshot);
            state.error = None;
        }
        Err(error) => state.error = Some(format!("{}: {error:?}", path.display())),
    }
}
fn themes_dir() -> PathBuf {
    home().join(".codex/themes")
}
fn theme_path(name: &str) -> PathBuf {
    themes_dir().join(format!("{name}.tmTheme"))
}
fn themes() -> Vec<String> {
    let mut names = fs::read_dir(themes_dir())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
fn write_confirmed(state: &mut AppState) {
    if let (Some(snapshot), Some(edit)) = (&state.codex_config, &state.codex_edit) {
        match config_io::write_config(snapshot, edit) {
            Ok(()) => {
                state.status = Some(t(state.lang, &Key::CodexSettingsSaved, &[]));
                if let Ok(snapshot) = config_io::read_config(&config_path()) {
                    state.codex_edit = Some(snapshot.config.clone());
                    state.codex_config = Some(snapshot)
                }
            }
            Err(err) => {
                state.error = Some(t(
                    state.lang,
                    &Key::CodexSettingsReloadedForDiff,
                    &[("error", &format!("{err:?}"))],
                ));
                if let Ok(snapshot) = config_io::read_config(&config_path()) {
                    state.codex_edit = Some(snapshot.config.clone());
                    state.codex_config = Some(snapshot)
                }
            }
        }
    }
}
impl Screen for CodexSettingsScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::CodexSettingsTitle, &[]));
        let Some(edit) = &s.codex_edit else {
            let missing_config = !config_path().is_file();
            let unavailable = t(s.lang, &Key::CodexSettingsNotLoaded, &[]);
            let message = if missing_config {
                t(s.lang, &Key::CodexSettingsNotDetected, &[])
            } else {
                t(
                    s.lang,
                    &Key::CodexSettingsUnavailable,
                    &[("error", s.error.as_deref().unwrap_or(&unavailable))],
                )
            };
            f.render_widget(
                Paragraph::new(message).style(Style::default().fg(DIM)),
                a[1],
            );
            footer(
                f,
                a[2],
                &t(
                    s.lang,
                    &if missing_config {
                        Key::CodexSettingsHintBack
                    } else {
                        Key::CodexSettingsHintRetry
                    },
                    &[],
                ),
            );
            return;
        };
        let line = render_preview_line(
            &edit.status_line,
            edit.status_line_use_colors,
            &theme_path(&edit.theme),
            "truecolor",
        )
        .unwrap_or_else(|e| {
            t(
                s.lang,
                &Key::CodexSettingsPreviewUnavailable,
                &[("error", &format!("{e:?}"))],
            )
        });
        let body = match s.mode {
            UiMode::CodexThemePicker => format!(
                "{}\n{}",
                t(s.lang, &Key::CodexSettingsSelectTheme, &[]),
                themes()
                    .iter()
                    .enumerate()
                    .map(|(i, n)| if i == s.selected() {
                        format!("▸ {n}")
                    } else {
                        format!("  {n}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            UiMode::CodexConfirmWrite => format!(
                "{}\n\nstatus_line: {:?} → {:?}\nuse_colors: {} → {}\ntheme: {} → {}",
                t(s.lang, &Key::CodexSettingsConfirmWrite, &[]),
                s.codex_config.as_ref().map(|v| &v.config.status_line),
                edit.status_line,
                s.codex_config
                    .as_ref()
                    .map(|v| v.config.status_line_use_colors)
                    .unwrap_or(false),
                edit.status_line_use_colors,
                s.codex_config
                    .as_ref()
                    .map(|v| v.config.theme.as_str())
                    .unwrap_or(""),
                edit.theme
            ),
            _ => String::new(),
        };
        if s.mode == UiMode::Normal {
            // Keep the editable list on its own line.  In particular, do not
            // append it to the preview note: long lists then overflow before
            // the selected item's cursor can be seen.
            let mut spans = vec![Span::raw("status_line: ")];
            for (index, item) in edit.status_line.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("  "));
                }
                let style = if index == s.selected() {
                    let base = Style::default()
                        .fg(TEXT)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
                    if self.move_mode {
                        base.fg(ratatui::style::Color::Yellow)
                    } else {
                        base
                    }
                } else {
                    Style::default().fg(TEXT)
                };
                spans.push(Span::styled(item.clone(), style));
            }
            let mut lines = vec![
                Line::from(spans),
                Line::from(t(s.lang, &Key::CodexSettingsSimulatedPreview, &[])),
            ];
            lines.extend(crate::tui::preview::ansi_lines(&line));
            lines.push(Line::from(format!(
                "use colors: {}",
                edit.status_line_use_colors
            )));
            lines.push(Line::from(format!("theme: {}", edit.theme)));
            f.render_widget(Paragraph::new(lines).style(Style::default().fg(TEXT)), a[1]);
        } else {
            f.render_widget(Paragraph::new(body).style(Style::default().fg(TEXT)), a[1]);
        }
        if let Some(e) = &s.error {
            footer_error(f, a[2], e);
        } else {
            footer(f, a[2], &t(s.lang, &Key::CodexSettingsHintActions, &[]));
        }
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if e.code == KeyCode::Esc {
            if s.mode != UiMode::Normal {
                s.mode = UiMode::Normal;
                return Action::Redraw;
            }
            s.screen = ScreenId::MainMenu;
            return Action::Back;
        }
        if s.codex_edit.is_none() {
            load_config(s);
            return Action::Redraw;
        }
        if s.mode == UiMode::CodexThemePicker {
            let options = themes();
            match e.code {
                KeyCode::Up => move_focus(s, options.len().max(1), -1),
                KeyCode::Down => move_focus(s, options.len().max(1), 1),
                KeyCode::Enter => {
                    let selected = s.selected();
                    if let (Some(edit), Some(name)) = (s.codex_edit.as_mut(), options.get(selected))
                    {
                        edit.theme = name.clone();
                        s.mode = UiMode::Normal
                    }
                }
                _ => {}
            }
            return Action::Redraw;
        }
        if s.mode == UiMode::CodexConfirmWrite {
            match e.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    write_confirmed(s);
                    s.mode = UiMode::Normal
                }
                KeyCode::Char('n') => s.mode = UiMode::Normal,
                _ => {}
            }
            return Action::Redraw;
        }
        let selected = s.selected();
        let item_count = s.codex_edit.as_ref().map_or(0, |v| v.status_line.len());
        if item_count == 0 {
            s.set_selected(0);
        } else if selected >= item_count {
            s.set_selected(item_count - 1);
        }
        let selected = s.selected();
        match e.code {
            KeyCode::Left if self.move_mode && item_count > 1 => {
                let to = (selected + item_count - 1) % item_count;
                if let Some(edit) = &mut s.codex_edit {
                    edit.status_line.swap(selected, to);
                }
                s.set_selected(to);
            }
            KeyCode::Right if self.move_mode && item_count > 1 => {
                let to = (selected + 1) % item_count;
                if let Some(edit) = &mut s.codex_edit {
                    edit.status_line.swap(selected, to);
                }
                s.set_selected(to);
            }
            KeyCode::Left => {
                let n = s
                    .codex_edit
                    .as_ref()
                    .map(|v| v.status_line.len())
                    .unwrap_or(1);
                move_focus(s, n.max(1), -1)
            }
            KeyCode::Right => {
                let n = s
                    .codex_edit
                    .as_ref()
                    .map(|v| v.status_line.len())
                    .unwrap_or(1);
                move_focus(s, n.max(1), 1)
            }
            KeyCode::Enter => self.move_mode = !self.move_mode,
            KeyCode::Char('a') => {
                if let Some(edit) = &mut s.codex_edit {
                    if let Some(id) = KNOWN_STATUS_LINE_IDS
                        .iter()
                        .find(|id| !edit.status_line.iter().any(|v| v == **id))
                    {
                        edit.status_line.push((*id).into())
                    }
                }
            }
            KeyCode::Char('d') => {
                if let Some(edit) = &mut s.codex_edit {
                    if selected < edit.status_line.len()
                        && KNOWN_STATUS_LINE_IDS.contains(&edit.status_line[selected].as_str())
                    {
                        edit.status_line.remove(selected);
                        s.set_selected(selected.saturating_sub(1));
                    }
                }
            }
            KeyCode::Char('c') => {
                if let Some(edit) = &mut s.codex_edit {
                    edit.status_line_use_colors = !edit.status_line_use_colors
                }
            }
            KeyCode::Char('t') => {
                s.mode = UiMode::CodexThemePicker;
                s.set_selected(0)
            }
            KeyCode::Char('w') => s.mode = UiMode::CodexConfirmWrite,
            KeyCode::Char('e') => {
                s.set_selected(0);
                s.input.clear();
                s.mode = UiMode::Normal;
                s.tmtheme = None;
                s.tmtheme_original = None;
                s.tmtheme_split = false;
                s.screen = ScreenId::TmThemeEditor;
            }
            _ => {}
        }
        Action::Redraw
    }
}
