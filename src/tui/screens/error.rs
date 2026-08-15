use crate::tui::{
    app::{AppState, ScreenId},
    i18n::{Key, t},
    screens::{
        Action, Screen,
        common::{ERROR, areas, footer, header, too_small},
    },
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, style::Style, widgets::Paragraph};
pub struct ErrorScreen;
impl ErrorScreen {
    pub fn new() -> Self {
        Self
    }
}
impl Screen for ErrorScreen {
    fn draw(&self, frame: &mut Frame, state: &AppState) {
        let Some(a) = areas(frame, state) else {
            too_small(frame, state);
            return;
        };
        header(frame, a[0], &t(state.lang, &Key::ErrorTitle, &[]));
        frame.render_widget(
            Paragraph::new(
                state
                    .error
                    .clone()
                    .unwrap_or_else(|| t(state.lang, &Key::CommonConfigurationError, &[])),
            )
            .style(Style::default().fg(ERROR)),
            a[1],
        );
        footer(
            frame,
            a[2],
            &format!(
                "{} {}",
                t(state.lang, &Key::ErrorHintRerunWizard, &[]),
                t(state.lang, &Key::ErrorHintFixManually, &[])
            ),
        );
    }
    fn on_key(&mut self, event: KeyEvent, state: &mut AppState) -> Action {
        match event.code {
            KeyCode::Char('1') => {
                state.error = None;
                state.wizard_step = crate::tui::wizard::WizardStep::Language;
                state.screen = ScreenId::Wizard;
                Action::Redraw
            }
            KeyCode::Char('2') => {
                state.should_quit = true;
                Action::Quit
            }
            _ => Action::None,
        }
    }
}
