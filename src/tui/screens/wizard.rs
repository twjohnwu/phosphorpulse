use crate::tui::{
    app::{AppState, ScreenId, UiMode},
    bin_install,
    i18n::{Key, t},
    screens::{
        Action, Screen,
        common::{ACCENT, areas, footer, footer_error, header, preview, too_small},
    },
    settings_writer,
    templates_io,
    wizard::{WizardAction, WizardInput, WizardStep, transition},
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, style::Style, widgets::Paragraph};
use serde_json::Value;
pub struct WizardScreen;
impl WizardScreen {
    pub fn new() -> Self {
        Self
    }
}
fn bin_found() -> bool {
    bin_install::detected_bin().is_some()
}
impl Screen for WizardScreen {
    fn draw(&self, frame: &mut Frame, state: &AppState) {
        let Some(a) = areas(frame, state) else {
            too_small(frame, state);
            return;
        };
        header(frame, a[0], &t(state.lang, &Key::WizardTitle, &[]));
        let text = match state.wizard_step {
            WizardStep::Language => format!(
                "{}\n\n{}",
                t(state.lang, &Key::WizardLanguageTitle, &[]),
                t(state.lang, &Key::WizardLanguagePrompt, &[])
            ),
            WizardStep::NerdFont => t(state.lang, &Key::WizardNerdFontQuestion, &[("yn", "y/n")]),
            WizardStep::BinInstall if state.mode == UiMode::WizardBinInstallConfirm => {
                bin_install_confirm_text(state)
            }
            WizardStep::BinStillMissing if state.mode == UiMode::WizardBinInstallConfirm => {
                bin_install_confirm_text(state)
            }
            WizardStep::BinInstall => format!(
                "{}\n{}",
                t(state.lang, &Key::BinInstallNotFound, &[]),
                t(state.lang, &Key::BinInstallRePrompt, &[])
            ),
            WizardStep::BinStillMissing => format!(
                "{}\n{}",
                t(state.lang, &Key::BinInstallStillNotFound, &[]),
                t(state.lang, &Key::BinInstallManualInstall, &[])
            ),
            WizardStep::Template => t(state.lang, &Key::WizardTemplateConfirmPrompt, &[]),
            WizardStep::Confirm => {
                let prompt = t(state.lang, &Key::SettingsWriteConfirmPrompt, &[]);
                match settings_writer::statusline_blocks_diff(None) {
                    Ok(diff) => format!("{diff}\n\n{prompt}"),
                    Err(_) => prompt,
                }
            }
        };
        let text = match &state.status {
            Some(status) => format!("{text}\n\n{status}"),
            None => text,
        };
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(ACCENT)),
            a[1],
        );
        if let Some(e) = &state.error {
            footer_error(frame, a[2], e);
        } else {
            let hint = if matches!(state.wizard_step, WizardStep::BinInstall | WizardStep::BinStillMissing) {
                [
                    t(state.lang, &Key::BinInstallHintInstallBin, &[]),
                    t(state.lang, &Key::BinInstallHintReDetect, &[]),
                    t(state.lang, &Key::WizardHintQuit, &[]),
                ]
                .join(" ")
            } else {
                t(state.lang, &Key::WizardHintQuit, &[])
            };
            footer(frame, a[2], &hint);
        }
        preview(frame, a[3], state);
    }
    fn on_key(&mut self, event: KeyEvent, state: &mut AppState) -> Action {
        if state.mode == UiMode::WizardBinInstallConfirm {
            return self.confirm_bin_install(event, state);
        }
        if event.code == KeyCode::Esc {
            if state.wizard_preserves_existing_config {
                state.screen = ScreenId::MainMenu;
                state.set_selected(0);
                return Action::Redraw;
            }
            return Action::Quit;
        }
        if state.wizard_step == WizardStep::Language
            && matches!(event.code, KeyCode::Up | KeyCode::Down)
        {
            state.lang = match state.lang {
                crate::tui::i18n::Lang::En => crate::tui::i18n::Lang::ZhTw,
                crate::tui::i18n::Lang::ZhTw => crate::tui::i18n::Lang::En,
            };
            return Action::Redraw;
        }
        let input = match state.wizard_step {
            WizardStep::Language if event.code == KeyCode::Enter => {
                state.draft.0.insert(
                    "language".into(),
                    Value::String(
                        match state.lang {
                            crate::tui::i18n::Lang::En => "en",
                            crate::tui::i18n::Lang::ZhTw => "zh-TW",
                        }
                        .into(),
                    ),
                );
                Some(WizardInput::ConfirmLanguage)
            }
            WizardStep::NerdFont
                if matches!(event.code, KeyCode::Char('y') | KeyCode::Char('n')) =>
            {
                state.draft.0.insert(
                    "nerdFont".into(),
                    Value::Bool(event.code == KeyCode::Char('y')),
                );
                Some(WizardInput::ConfirmNerdFont {
                    bin_found: bin_found(),
                })
            }
            WizardStep::BinInstall | WizardStep::BinStillMissing if event.code == KeyCode::Char('b') => {
                if let Some(path) = bin_install::local_bin_path() {
                    state.error = None;
                    state.pending_path = path;
                    state.mode = UiMode::WizardBinInstallConfirm;
                } else {
                    state.error = Some(t(state.lang, &Key::SettingsInstallBinHomeMissing, &[]));
                }
                None
            }
            WizardStep::BinInstall | WizardStep::BinStillMissing => {
                Some(WizardInput::RedetectBin {
                    bin_found: bin_found(),
                })
            }
            WizardStep::Template if event.code == KeyCode::Enter => {
                if let Err(error) = apply_selected_template(state) {
                    state.error = Some(error.to_string());
                    return Action::Redraw;
                }
                Some(WizardInput::ConfirmTemplate)
            }
            WizardStep::Confirm if event.code == KeyCode::Char('n') => {
                Some(WizardInput::DeclineSettingsWrite)
            }
            WizardStep::Confirm if matches!(event.code, KeyCode::Char('y') | KeyCode::Enter) => {
                Some(WizardInput::ConfirmSettingsWrite)
            }
            _ => None,
        };
        if let Some(input) = input {
            let (step, actions) = transition(state.wizard_step, input);
            state.wizard_step = step;
            for action in actions {
                match action {
                    WizardAction::WriteSettings => {
                        if let Err(e) = settings_writer::write_statusline_blocks(None) {
                            state.error = Some(e.to_string());
                            return Action::Redraw;
                        }
                    }
                    WizardAction::WriteOwnConfig => {
                        if let Err(e) =
                            crate::tui::app::write_own_config(&state.draft, &state.config_dir)
                        {
                            state.error = Some(e.to_string());
                            return Action::Redraw;
                        };
                        state.screen = ScreenId::MainMenu;
                    }
                }
            }
        }
        Action::Redraw
    }
}

/// Applies the wizard's selected template before confirmation writes the
/// draft. First-run defaults select the built-in `matrix-tron` snapshot.
fn apply_selected_template(state: &mut AppState) -> std::io::Result<()> {
    let name = state
        .draft
        .0
        .get("activeTemplate")
        .and_then(Value::as_str)
        .unwrap_or("matrix-tron")
        .to_owned();
    let template = templates_io::load_template(&name, &state.config_dir.join("templates"))?;
    let template = template.as_object();
    for field in ["rows", "subagent", "gauge", "segments"] {
        state.draft.0.insert(
            field.into(),
            template
                .and_then(|snapshot| snapshot.get(field))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    Ok(())
}

impl WizardScreen {
    fn confirm_bin_install(&mut self, event: KeyEvent, state: &mut AppState) -> Action {
        match event.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let path = state.pending_path.clone();
                match bin_install::install_global_bin(&path) {
                    Ok(()) => {
                        state.error = None;
                        let installed = t(
                            state.lang,
                            &Key::BinInstallInstalledTo,
                            &[("path", &path.display().to_string())],
                        );
                        state.status = Some(if bin_install::bin_on_path().is_none() {
                            format!(
                                "{installed}\n{}",
                                t(state.lang, &Key::SettingsInstallBinPathHint, &[])
                            )
                        } else {
                            installed
                        });
                        state.pending_path.clear();
                        state.mode = UiMode::Normal;
                        let (step, _) = transition(
                            state.wizard_step,
                            // A successful copy is the authoritative detection result.
                            // Do not make this first-run transition depend on a later
                            // filesystem/PATH re-scan or another keypress.
                            WizardInput::RedetectBin { bin_found: true },
                        );
                        state.wizard_step = step;
                    }
                    Err(error) => {
                        state.error = Some(t(
                            state.lang,
                            &Key::SettingsInstallBinInstallFailed,
                            &[
                                ("path", &path.display().to_string()),
                                ("reason", &error.to_string()),
                            ],
                        ));
                        state.pending_path.clear();
                        state.mode = UiMode::Normal;
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.pending_path.clear();
                state.mode = UiMode::Normal;
            }
            _ => {}
        }
        Action::Redraw
    }
}

fn bin_install_confirm_text(state: &AppState) -> String {
    let path = state.pending_path.display().to_string();
    let key = if state.pending_path.exists() {
        Key::SettingsInstallBinOverwriteConfirm
    } else {
        Key::SettingsInstallBinInstallConfirm
    };
    t(state.lang, &key, &[("path", &path)])
}
