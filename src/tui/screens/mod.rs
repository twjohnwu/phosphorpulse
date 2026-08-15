//! Screen collection shell for the TUI.
//! Thin drawing and key-forwarding screens.

mod codex_settings;
mod common;
mod editor;
mod error;
mod main_menu;
mod tmtheme_editor;
mod wizard;

pub use codex_settings::CodexSettingsScreen;
pub use editor::{
    ColorsThemeScreen, RowsSegmentsScreen, SaveExitScreen, SettingsInstallScreen,
    SubagentLineScreen, TemplatesScreen,
};
pub use error::ErrorScreen;
pub use main_menu::MainMenuScreen;
pub use tmtheme_editor::TmThemeEditorScreen;
pub use wizard::WizardScreen;

use crate::tui::app::AppState;
use crossterm::event::KeyEvent;
use ratatui::Frame;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    None,
    Redraw,
    Back,
    Quit,
}

/// The only UI pattern: every screen draws from shared state and forwards keys.
pub trait Screen {
    fn draw(&self, frame: &mut Frame, state: &AppState);
    fn on_key(&mut self, event: KeyEvent, state: &mut AppState) -> Action;
}
