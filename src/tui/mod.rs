//! Terminal user-interface entry point and supporting modules.

pub mod app;
pub mod bin_install;
pub mod chrome;
pub mod codex;
pub mod draft_ops;
pub mod i18n;
pub mod preview;
pub mod router;
pub mod screens;
pub mod settings_writer;
pub mod templates_io;
pub mod wizard;

use app::{AppState, ScreenId};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use router::Route;
use screens::{
    Action, CodexSettingsScreen, ColorsThemeScreen, ErrorScreen, MainMenuScreen,
    RowsSegmentsScreen, SaveExitScreen, Screen, SettingsInstallScreen, SubagentLineScreen,
    TemplatesScreen, TmThemeEditorScreen, WizardScreen,
};
use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::{self, Write},
    panic,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, atomic::{AtomicBool, Ordering}},
    time::{SystemTime, UNIX_EPOCH},
};

static PANICKED: AtomicBool = AtomicBool::new(false);
static PANIC_LOG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn panic_log_path() -> Option<PathBuf> {
    PANIC_LOG_PATH
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|path| path.clone())
}

fn install_panic_hook(config_dir: &Path) {
    let config_dir = config_dir.to_path_buf();
    let path = config_dir.join("panic.log");
    if let Ok(mut stored_path) = PANIC_LOG_PATH.get_or_init(|| Mutex::new(None)).lock() {
        *stored_path = Some(path.clone());
    }
    PANICKED.store(false, Ordering::Release);
    panic::set_hook(Box::new(move |info| {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let entry = format!(
            "timestamp: {timestamp}\ncrate version: {}\npanic: {info}\nbacktrace:\n{}\n\n",
            env!("CARGO_PKG_VERSION"),
            Backtrace::force_capture(),
        );
        if fs::create_dir_all(&config_dir).is_ok() {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = file.write_all(entry.as_bytes());
            }
        }
        PANICKED.store(true, Ordering::Release);
    }));
}

struct TerminalGuard {
    alternate_screen: bool,
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        if self.alternate_screen {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
        if PANICKED.swap(false, Ordering::AcqRel) {
            if let Some(path) = panic_log_path() {
                eprintln!("phosphorpulse: panic details written to {}", path.display());
            }
        }
    }
}

fn draw_screen(
    screen: &mut dyn Screen,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &AppState,
) -> io::Result<()> {
    terminal.draw(|frame| screen.draw(frame, state)).map(|_| ())
}

fn screen_for(id: ScreenId) -> Box<dyn Screen> {
    match id {
        ScreenId::MainMenu => Box::new(MainMenuScreen::new()),
        ScreenId::RowsSegments => Box::new(RowsSegmentsScreen::new()),
        ScreenId::SubagentLine => Box::new(SubagentLineScreen::new()),
        ScreenId::ColorsTheme => Box::new(ColorsThemeScreen::new()),
        ScreenId::Templates => Box::new(TemplatesScreen::new()),
        ScreenId::SettingsInstall => Box::new(SettingsInstallScreen::new()),
        ScreenId::CodexSettings => Box::new(CodexSettingsScreen::new()),
        ScreenId::TmThemeEditor => Box::new(TmThemeEditorScreen::new()),
        ScreenId::SaveExit => Box::new(SaveExitScreen::new()),
        ScreenId::Wizard => Box::new(WizardScreen::new()),
        ScreenId::Error => Box::new(ErrorScreen::new()),
    }
}

/// Runs the terminal user interface.
pub fn run_tui() -> io::Result<()> {
    let config_dir = phosphorpulse::config::settings_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    install_panic_hook(&config_dir);
    let route = router::initial_route();
    let wizard_preserves_existing_config = matches!(
        &route,
        Route::Wizard {
            preserve_existing_config: true
        }
    );
    let (draft, screen, error) = match route {
        Route::MainMenu => (
            phosphorpulse::config::load()
                .map(|l| l.config)
                .unwrap_or_else(|_| phosphorpulse::config::default_config()),
            ScreenId::MainMenu,
            None,
        ),
        Route::Wizard {
            preserve_existing_config,
        } => (
            if preserve_existing_config {
                phosphorpulse::config::load()
                    .map(|loaded| loaded.config)
                    .unwrap_or_else(|_| phosphorpulse::config::default_config())
            } else {
                phosphorpulse::config::default_config()
            },
            ScreenId::Wizard,
            None,
        ),
        Route::ErrorScreen { path, reason } => (
            phosphorpulse::config::default_config(),
            ScreenId::Error,
            Some(format!("{path}: {reason}")),
        ),
    };
    enable_raw_mode()?;
    let mut guard = TerminalGuard {
        alternate_screen: false,
    };
    execute!(io::stdout(), EnterAlternateScreen)?;
    guard.alternate_screen = true;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = AppState::new(draft, screen);
    state.wizard_preserves_existing_config = wizard_preserves_existing_config;
    state.error = error;
    let mut current_id = state.screen;
    let mut current = screen_for(current_id);
    loop {
        draw_screen(current.as_mut(), &mut terminal, &state)?;
        if let Event::Key(key) = event::read()? {
            let action = current.on_key(key, &mut state);
            if matches!(action, Action::Quit) || state.should_quit {
                break;
            }
            if state.screen != current_id {
                current_id = state.screen;
                current = screen_for(current_id);
            }
        }
    }
    Ok(())
}
