use crate::tui::{
    app::{AppState, ScreenId},
    i18n::{Key, Lang, t},
    screens::{
        Action, Screen,
        codex_settings::load_config,
        common::{ACCENT, TEXT, areas, footer, header, move_focus, preview, too_small},
    },
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    style::{Modifier, Style},
    widgets::{List, ListItem},
};

pub struct MainMenuScreen;
const ITEMS: [Key; 8] = [
    Key::MenuRowsSegments,
    Key::MenuSubagentLine,
    Key::MenuColorsTheme,
    Key::MenuTemplates,
    Key::MenuSettingsInstall,
    Key::MenuCodexSettings,
    Key::MenuSaveExit,
    Key::MainMenuLanguage,
];
impl MainMenuScreen {
    pub fn new() -> Self {
        Self
    }
}
fn toggle_language(state: &mut AppState) {
    state.lang = if state.lang == Lang::En {
        Lang::ZhTw
    } else {
        Lang::En
    };
    state.draft.0.insert(
        "language".into(),
        serde_json::Value::String(
            match state.lang {
                Lang::En => "en",
                Lang::ZhTw => "zh-TW",
            }
            .into(),
        ),
    );
}
impl Screen for MainMenuScreen {
    fn draw(&self, frame: &mut Frame, state: &AppState) {
        let Some(a) = areas(frame, state) else {
            too_small(frame, state);
            return;
        };
        header(frame, a[0], &t(state.lang, &Key::MainMenuTitle, &[]));
        let items = ITEMS.iter().enumerate().map(|(i, key)| {
            let language = if state.lang == Lang::En {
                t(state.lang, &Key::WizardLanguageOptionEnglish, &[])
            } else {
                t(state.lang, &Key::WizardLanguageOptionZhTw, &[])
            };
            let label = t(state.lang, key, &[("value", &language)]);
            ListItem::new(if i == state.selected() {
                format!("▸ {label}")
            } else {
                format!("  {label}")
            })
            .style(
                Style::default()
                    .fg(if i == state.selected() { ACCENT } else { TEXT })
                    .add_modifier(if i == state.selected() {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            )
        });
        frame.render_widget(List::new(items), a[1]);
        footer(frame, a[2], &t(state.lang, &Key::MainMenuHintMove, &[]));
        preview(frame, a[3], state);
    }
    fn on_key(&mut self, event: KeyEvent, state: &mut AppState) -> Action {
        match event.code {
            KeyCode::Up => move_focus(state, 8, -1),
            KeyCode::Down => move_focus(state, 8, 1),
            KeyCode::Left | KeyCode::Right if state.selected() == 7 => toggle_language(state),
            KeyCode::Enter if state.selected() == 7 => toggle_language(state),
            KeyCode::Enter => {
                let selected = state.selected();
                state.screen = match selected {
                    0 => ScreenId::RowsSegments,
                    1 => ScreenId::SubagentLine,
                    2 => ScreenId::ColorsTheme,
                    3 => ScreenId::Templates,
                    4 => ScreenId::SettingsInstall,
                    5 => ScreenId::CodexSettings,
                    6 => ScreenId::SaveExit,
                    _ => return Action::Redraw,
                };
                state.set_selected(0);
                if selected == 5 {
                    load_config(state);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
            _ => {}
        }
        Action::Redraw
    }
}
