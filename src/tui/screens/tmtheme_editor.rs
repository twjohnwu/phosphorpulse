use crate::tui::{
    app::{AppState, Focus, ScreenId, UiMode},
    codex::tmtheme,
    draft_ops,
    i18n::{Key, t},
    screens::{
        Action, Screen,
        common::{DIM, TEXT, areas, footer, footer_error, header, move_focus, too_small},
    },
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::path::PathBuf;
pub struct TmThemeEditorScreen;
impl TmThemeEditorScreen {
    pub fn new() -> Self {
        Self
    }
}
const FIELDS: [Key; 6] = [
    Key::TmThemeFieldGlobalForeground,
    Key::TmThemeFieldGlobalBackground,
    Key::TmThemeFieldConstantNumeric,
    Key::TmThemeFieldConstant,
    Key::TmThemeFieldConstantLanguage,
    Key::TmThemeFieldStorageType,
];
const SCOPES: [Option<&str>; 6] = [
    None,
    None,
    Some("constant.numeric"),
    Some("constant"),
    Some("constant.language"),
    Some("storage.type"),
];
fn path(state: &AppState) -> Option<PathBuf> {
    state.codex_edit.as_ref().map(|e| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codex/themes")
            .join(format!("{}.tmTheme", e.theme))
    })
}
fn valid_hex(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn hex_colour(value: &str) -> Option<Color> {
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ))
}
fn value(state: &AppState, index: usize) -> String {
    state
        .tmtheme
        .as_ref()
        .and_then(|snapshot| match index {
            0 => snapshot.theme.global_foreground(),
            1 => snapshot.theme.global_background(),
            i => SCOPES[i].and_then(|scope| snapshot.theme.resolved_foreground(scope)),
        })
        .map(str::to_owned)
        .unwrap_or_else(|| t(state.lang, &Key::TmThemeNone, &[]))
}
fn original_value(state: &AppState, index: usize) -> String {
    state
        .tmtheme_original
        .as_ref()
        .and_then(|snapshot| match index {
            0 => snapshot.theme.global_foreground(),
            1 => snapshot.theme.global_background(),
            i => SCOPES[i].and_then(|scope| snapshot.theme.resolved_foreground(scope)),
        })
        .map(str::to_owned)
        .unwrap_or_else(|| t(state.lang, &Key::TmThemeNone, &[]))
}
fn semantic_diff(state: &AppState) -> String {
    let mut lines = FIELDS
        .iter()
        .enumerate()
        .filter_map(|(index, key)| {
            let old = original_value(state, index);
            let new = value(state, index);
            (old != new).then(|| format!("{}: {old} → {new}", t(state.lang, key, &[])))
        })
        .collect::<Vec<_>>();
    if state.tmtheme_split {
        for index in 2..FIELDS.len() {
            let colour = value(state, index);
            lines.push(t(
                state.lang,
                &Key::TmThemeFusedSelector,
                &[
                    ("field", &t(state.lang, &FIELDS[index], &[])),
                    ("colour", &colour),
                ],
            ));
        }
    }
    if lines.is_empty() {
        t(state.lang, &Key::TmThemeNoColourChanges, &[])
    } else {
        lines.join("\n")
    }
}
fn named_colour_suffix(colour: &str) -> String {
    draft_ops::named_color(colour)
        .map(|(_, name)| format!(" ({name})"))
        .unwrap_or_default()
}
fn colour_hint(state: &AppState) -> String {
    let colour = value(state, state.selected());
    match draft_ops::named_color(&colour) {
        Some((index, name)) => t(
            state.lang,
            &Key::TmThemeNamedColour,
            &[("name", name), ("index", &index.to_string())],
        ),
        None => t(state.lang, &Key::TmThemeCustomColour, &[]),
    }
}
fn apply_field(state: &mut AppState, index: usize, colour: &str) {
    let Some(snapshot) = &mut state.tmtheme else {
        return;
    };
    let result = match index {
        0 => {
            snapshot.theme.set_global_foreground(colour);
            Ok(())
        }
        1 => {
            snapshot.theme.set_global_background(colour);
            Ok(())
        }
        i => snapshot
            .theme
            .set_scope_foreground(SCOPES[i].expect("scope field"), colour),
    };
    if let Err(err) = result {
        state.error = Some(t(
            state.lang,
            &Key::TmThemeSplitScopesFirst,
            &[("error", &format!("{err:?}"))],
        ))
    }
}
fn write_confirmed(state: &mut AppState) {
    if let Some(snapshot) = &state.tmtheme {
        match tmtheme::write_tmtheme(snapshot) {
            Ok(()) => {
                state.status = Some(t(state.lang, &Key::TmThemeSaved, &[]));
                if let Ok(fresh) = tmtheme::read_tmtheme(&snapshot.path) {
                    state.tmtheme_original = Some(fresh.clone());
                    state.tmtheme = Some(fresh);
                    state.tmtheme_split = false;
                }
            }
            Err(err) => {
                state.error = Some(t(
                    state.lang,
                    &Key::TmThemeReloadedForDiff,
                    &[("error", &format!("{err:?}"))],
                ));
                if let Ok(fresh) = tmtheme::read_tmtheme(&snapshot.path) {
                    state.tmtheme_original = Some(fresh.clone());
                    state.tmtheme = Some(fresh);
                    state.tmtheme_split = false;
                }
            }
        }
    }
}
impl Screen for TmThemeEditorScreen {
    fn draw(&self, f: &mut Frame, s: &AppState) {
        let Some(a) = areas(f, s) else {
            too_small(f, s);
            return;
        };
        header(f, a[0], &t(s.lang, &Key::TmThemeTitle, &[]));
        match &s.tmtheme {
            Some(_) if s.mode != UiMode::TmThemeConfirmWrite => {
                let body = FIELDS
                    .iter()
                    .enumerate()
                    .map(|(i, key)| {
                        let colour = value(s, i);
                        let row_colour = hex_colour(&colour).unwrap_or(TEXT);
                        let text_style = Style::default().fg(row_colour);
                        let swatch_style = Style::default().bg(row_colour);
                        Line::from(vec![
                            Span::raw(format!(
                                "{} {}: ",
                                if i == s.selected() { "▸" } else { " " },
                                t(s.lang, key, &[])
                            )),
                            Span::styled(
                                format!("{}{}  ", colour, named_colour_suffix(&colour)),
                                text_style,
                            ),
                            Span::styled("  ", swatch_style),
                        ])
                    })
                    .collect::<Vec<_>>();
                f.render_widget(Paragraph::new(body).style(Style::default().fg(TEXT)), a[1]);
            }
            Some(_) => f.render_widget(
                Paragraph::new(format!(
                    "{}\n\n{}",
                    t(s.lang, &Key::TmThemeConfirmWrite, &[]),
                    semantic_diff(s)
                ))
                .style(Style::default().fg(TEXT)),
                a[1],
            ),
            None => f.render_widget(
                Paragraph::new(t(s.lang, &Key::TmThemeNoEditableTheme, &[]))
                    .style(Style::default().fg(TEXT)),
                a[1],
            ),
        }
        if let Some(e) = &s.error {
            footer_error(f, a[2], e);
        } else {
            footer(f, a[2], &t(s.lang, &Key::TmThemeHintActions, &[]));
        }
        let bottom = if matches!(s.focus, Focus::Editing) {
            t(s.lang, &Key::TmThemeHexColour, &[("value", &s.input)])
        } else {
            format!(
                "{}  {}",
                t(s.lang, &Key::TmThemeColourSwatch, &[]),
                colour_hint(s)
            )
        };
        f.render_widget(Paragraph::new(bottom).style(Style::default().fg(DIM)), a[3]);
    }
    fn on_key(&mut self, e: KeyEvent, s: &mut AppState) -> Action {
        if e.code == KeyCode::Esc {
            if s.mode != UiMode::Normal {
                s.mode = UiMode::Normal;
                return Action::Redraw;
            }
            if matches!(s.focus, Focus::Editing) {
                s.focus = Focus::List(s.selected());
                s.input.clear();
                return Action::Redraw;
            }
            s.screen = ScreenId::CodexSettings;
            return Action::Back;
        }
        if s.tmtheme.is_none() {
            match path(s).and_then(|p| tmtheme::read_tmtheme(&p).ok()) {
                Some(snapshot) => {
                    s.tmtheme_original = Some(snapshot.clone());
                    s.tmtheme = Some(snapshot)
                }
                None => s.error = Some(t(s.lang, &Key::TmThemeUnavailable, &[])),
            };
            return Action::Redraw;
        }
        if s.mode == UiMode::TmThemeConfirmWrite {
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
        if matches!(s.focus, Focus::Editing) {
            match e.code {
                KeyCode::Enter => {
                    if valid_hex(&s.input) {
                        let input = s.input.clone();
                        let selected = s.selected();
                        apply_field(s, selected, &input);
                        s.focus = Focus::List(selected);
                        s.input.clear()
                    } else {
                        s.error = Some(t(s.lang, &Key::TmThemeInvalidHex, &[]))
                    }
                }
                KeyCode::Backspace => s.pop_input_grapheme(),
                KeyCode::Char(c) => s.push_input(c),
                _ => {}
            }
            return Action::Redraw;
        }
        match e.code {
            KeyCode::Up => move_focus(s, FIELDS.len(), -1),
            KeyCode::Down => move_focus(s, FIELDS.len(), 1),
            KeyCode::Left | KeyCode::Right => {
                let direction = if e.code == KeyCode::Left { -1 } else { 1 };
                let selected = s.selected();
                let next = draft_ops::cycle_named_color(&value(s, selected), direction);
                apply_field(s, selected, next);
            }
            KeyCode::Char('s') => {
                if let Some(theme) = &mut s.tmtheme {
                    if let Err(err) = tmtheme::split_codex_scopes(&mut theme.theme) {
                        s.error = Some(format!("{err:?}"))
                    } else {
                        s.tmtheme_split = true;
                        s.status = Some(t(s.lang, &Key::TmThemeScopesSplit, &[]))
                    }
                }
            }
            KeyCode::Char('e') => {
                s.input = value(s, s.selected());
                s.focus = Focus::Editing
            }
            KeyCode::Char('w') => s.mode = UiMode::TmThemeConfirmWrite,
            _ => {}
        }
        Action::Redraw
    }
}
