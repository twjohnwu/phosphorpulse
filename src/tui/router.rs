//! Routing shell for TUI navigation.

use std::{fs, io::ErrorKind, path::PathBuf};

use phosphorpulse::{config, tui::settings_writer};
use serde_json::Value;

/// The three top-level destinations selected before the event loop starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Route {
    /// `preserve_existing_config` is true only when Claude's statusLine needs
    /// rewiring while this installation's own valid config already exists.
    Wizard { preserve_existing_config: bool },
    MainMenu,
    ErrorScreen { path: String, reason: String },
}

/// Decides the initial route from the existing configuration loader.
pub fn initial_route() -> Route {
    match config::load() {
        Ok(_) if statusline_is_wired() => Route::MainMenu,
        Ok(_) => Route::Wizard {
            preserve_existing_config: true,
        },
        Err(reason) => {
            let path = config::settings_path();
            if matches!(fs::metadata(&path), Err(error) if error.kind() == ErrorKind::NotFound) {
                Route::Wizard {
                    preserve_existing_config: false,
                }
            } else {
                Route::ErrorScreen {
                    path: path.display().to_string(),
                    reason,
                }
            }
        }
    }
}

/// Pure route dispatch, kept separate so assembly tests need no filesystem.
pub fn dispatch(
    config_exists: bool,
    statusline_is_wired: bool,
    load_error: Option<(&str, &str)>,
) -> Route {
    match (config_exists, load_error, statusline_is_wired) {
        (false, _, _) => Route::Wizard {
            preserve_existing_config: false,
        },
        (true, None, true) => Route::MainMenu,
        (true, None, false) => Route::Wizard {
            preserve_existing_config: true,
        },
        (true, Some((path, reason)), _) => Route::ErrorScreen {
            path: path.into(),
            reason: reason.into(),
        },
    }
}

fn statusline_is_wired() -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    statusline_is_wired_at(&PathBuf::from(home).join(".claude/settings.json"))
}

/// Checks whether a Claude settings file points its status line at this tool.
/// Kept public for filesystem-isolated assembly coverage of startup routing.
pub fn statusline_is_wired_at(path: &std::path::Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|settings| settings.as_object().cloned())
        .and_then(|settings| settings_writer::configured_command(&settings, "statusLine"))
        .is_some_and(|command| command.contains("phosphorpulse"))
}
