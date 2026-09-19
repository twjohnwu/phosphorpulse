use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use phosphorpulse::{
    config::default_config,
    tui::{
        app::{AppState, ScreenId, UiMode},
        bin_install,
        router::{Route, dispatch, initial_route, statusline_is_wired_at},
        wizard::WizardStep,
        screens::{
            CodexSettingsScreen, ColorsThemeScreen, ErrorScreen, MainMenuScreen,
            RowsSegmentsScreen, SaveExitScreen, Screen, SettingsInstallScreen,
            SubagentLineScreen, TemplatesScreen, TmThemeEditorScreen, WizardScreen,
        },
    },
};
use std::sync::Mutex;

#[path = "../src/tui/codex/config_io.rs"]
mod config_io;

#[path = "../src/tui/templates_io.rs"]
mod templates_io;

static HOME_LOCK: Mutex<()> = Mutex::new(());
static TEMP_DIR_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
}

fn with_completely_empty_home(test: impl FnOnce(&std::path::Path)) {
    let root = std::env::temp_dir().join(format!(
        "phosphorpulse-fresh-home-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
        TEMP_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let old_home = std::env::var_os("HOME");
    let old_path = std::env::var_os("PATH");
    std::fs::create_dir_all(&root).expect("create empty HOME root");
    assert!(!root.join(".claude").exists(), "fresh HOME has no .claude");
    assert!(!root.join(".codex").exists(), "fresh HOME has no .codex");
    unsafe { std::env::set_var("HOME", &root) };

    test(&root);

    unsafe {
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn screens_are_constructible_without_a_terminal() {
    let state = AppState::new(default_config(), ScreenId::MainMenu);
    let _menu = MainMenuScreen::new();
    let _wizard = WizardScreen::new();
    assert_eq!(state.screen, ScreenId::MainMenu);
}

#[test]
fn fresh_home_wizard_writes_selected_builtin_rows() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        // A plain file is enough for the TUI's simulated bin discovery.
        let bin_dir = home.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create simulated PATH directory");
        std::fs::write(bin_dir.join("phosphorpulse"), b"simulated binary")
            .expect("create simulated binary");
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(bin_dir).chain(std::env::split_paths(&old_path)),
        )
        .expect("construct simulated PATH");
        unsafe { std::env::set_var("PATH", path) };

        let mut state = AppState::new(default_config(), ScreenId::Wizard);
        let mut wizard = WizardScreen::new();
        wizard.on_key(press(KeyCode::Enter), &mut state); // language
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // nerd font; simulated bin found
        wizard.on_key(press(KeyCode::Enter), &mut state); // matrix-tron template
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // both writes

        let path = home.join(".claude/phosphorpulse/settings.json");
        let selected_template = templates_io::load_template(
            "matrix-tron",
            &home.join(".claude/phosphorpulse/templates"),
        )
        .expect("load selected built-in template");
        assert_eq!(state.screen, ScreenId::MainMenu, "wizard completed");
        assert!(path.exists(), "wizard wrote its own settings.json");
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).expect("read fresh-home own config"),
        )
        .expect("fresh-home own config is JSON");
        assert_eq!(saved["activeTemplate"], "matrix-tron");
        assert_eq!(saved["rows"], selected_template["rows"]);
        assert!(!saved["rows"].as_array().expect("rows array").is_empty());
    });
}

#[test]
fn fresh_route_wizard_reaches_template_immediately_after_builtin_bin_install() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let controlled_path = home.join("controlled-path");
        std::fs::create_dir_all(&controlled_path).expect("create controlled PATH directory");
        unsafe { std::env::set_var("PATH", &controlled_path) };

        assert_eq!(
            initial_route(),
            Route::Wizard {
                preserve_existing_config: false
            },
            "an empty HOME starts at the real fresh-install route"
        );
        let mut state = AppState::new(default_config(), ScreenId::Wizard);
        let mut wizard = WizardScreen::new();
        wizard.on_key(press(KeyCode::Enter), &mut state); // language
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // nerd font
        assert_eq!(state.wizard_step, WizardStep::BinInstall);
        wizard.on_key(press(KeyCode::Char('b')), &mut state); // install action
        assert_eq!(state.mode, UiMode::WizardBinInstallConfirm);
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // confirm install

        assert_eq!(
            state.wizard_step,
            WizardStep::Template,
            "the successful install settles detection without a further keypress"
        );
    });
}

#[test]
fn wizard_recovers_from_still_missing_after_builtin_bin_install() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let controlled_path = home.join("controlled-path");
        std::fs::create_dir_all(&controlled_path).expect("create controlled PATH directory");
        unsafe { std::env::set_var("PATH", &controlled_path) };

        let mut state = AppState::new(default_config(), ScreenId::Wizard);
        let mut wizard = WizardScreen::new();
        wizard.on_key(press(KeyCode::Enter), &mut state); // language
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // nerd font; bin absent
        assert_eq!(state.wizard_step, WizardStep::BinInstall);
        wizard.on_key(press(KeyCode::Char('r')), &mut state); // re-scan; bin still absent
        assert_eq!(state.wizard_step, WizardStep::BinStillMissing);

        wizard.on_key(press(KeyCode::Char('b')), &mut state); // install action
        assert_eq!(state.mode, UiMode::WizardBinInstallConfirm);
        wizard.on_key(press(KeyCode::Char('y')), &mut state); // confirm successful install

        assert_eq!(
            state.wizard_step,
            WizardStep::Template,
            "a successful re-detect escapes BinStillMissing immediately"
        );
    });
}

#[test]
fn main_menu_enter_paths_tolerate_missing_codex_config() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let own_config = home.join(".claude/phosphorpulse/settings.json");
        std::fs::create_dir_all(own_config.parent().expect("own config parent"))
            .expect("create own config directory");
        std::fs::write(&own_config, serde_json::to_vec(&default_config().0).expect("serialize config"))
            .expect("write own config");
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"statusLine":{"command":"phosphorpulse"}}"#,
        )
        .expect("write Claude settings");
        assert_eq!(initial_route(), Route::MainMenu);
        assert!(!home.join(".codex").exists(), "Codex is not installed");

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        let mut settings_state = AppState::new(default_config(), ScreenId::MainMenu);
        let mut menu = MainMenuScreen::new();
        settings_state.set_selected(4);
        menu.on_key(press(KeyCode::Enter), &mut settings_state);
        assert_eq!(settings_state.screen, ScreenId::SettingsInstall);
        let settings = SettingsInstallScreen::new();
        terminal
            .draw(|frame| settings.draw(frame, &settings_state))
            .expect("Settings & Install draws without ~/.codex");
        let settings_rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(settings_rendered.contains("Codex: not configured"));

        let mut codex_state = AppState::new(default_config(), ScreenId::MainMenu);
        let mut menu = MainMenuScreen::new();
        codex_state.set_selected(5);
        menu.on_key(press(KeyCode::Enter), &mut codex_state);
        assert_eq!(codex_state.screen, ScreenId::CodexSettings);
        assert!(codex_state.codex_edit.is_none(), "missing config is not editable");
        let codex = CodexSettingsScreen::new();
        terminal
            .draw(|frame| codex.draw(frame, &codex_state))
            .expect("Codex Settings draws without ~/.codex");
        let codex_rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(codex_rendered.contains(
            "Codex not detected — install the Codex CLI, or create ~/.codex/config.toml"
        ));
    });
}

#[test]
fn local_bin_target_is_detected_when_not_on_path() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let controlled_path = home.join("controlled-path");
        std::fs::create_dir_all(&controlled_path).expect("create controlled PATH directory");
        unsafe { std::env::set_var("PATH", &controlled_path) };
        let local_bin = home.join(".local/bin/phosphorpulse");
        std::fs::create_dir_all(local_bin.parent().expect("local bin parent"))
            .expect("create local bin directory");
        std::fs::write(&local_bin, b"simulated binary").expect("create local binary");

        assert_eq!(bin_install::bin_on_path(), None, "controlled PATH excludes local bin");
        assert_eq!(bin_install::detected_bin(), Some(local_bin));
    });
}

#[test]
fn wizard_bin_install_reports_success_and_advances_when_local_bin_is_not_on_path() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let controlled_path = home.join("controlled-path");
        std::fs::create_dir_all(&controlled_path).expect("create controlled PATH directory");
        unsafe { std::env::set_var("PATH", &controlled_path) };

        let mut state = AppState::new(default_config(), ScreenId::Wizard);
        let mut wizard = WizardScreen::new();
        wizard.on_key(press(KeyCode::Enter), &mut state);
        wizard.on_key(press(KeyCode::Char('y')), &mut state);
        assert_eq!(state.wizard_step, WizardStep::BinInstall);

        wizard.on_key(press(KeyCode::Char('b')), &mut state);
        assert_eq!(state.mode, UiMode::WizardBinInstallConfirm);
        wizard.on_key(press(KeyCode::Char('y')), &mut state);

        assert_eq!(state.wizard_step, WizardStep::Template, "successful re-detect advances wizard");
        let status = state.status.as_ref().expect("install success status");
        assert!(status.contains("installed to"));
        assert!(status.contains(&home.join(".local/bin/phosphorpulse").display().to_string()));
        assert!(status.contains("PATH"));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        terminal
            .draw(|frame| wizard.draw(frame, &state))
            .expect("draw wizard success feedback");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("installed to"), "success feedback is rendered");
    });
}

#[test]
fn every_screen_draws_and_handles_a_benign_key_on_a_fresh_home() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|_| {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        let screens: Vec<(ScreenId, Box<dyn Screen>)> = vec![
            (ScreenId::MainMenu, Box::new(MainMenuScreen::new())),
            (ScreenId::RowsSegments, Box::new(RowsSegmentsScreen::new())),
            (ScreenId::SubagentLine, Box::new(SubagentLineScreen::new())),
            (ScreenId::ColorsTheme, Box::new(ColorsThemeScreen::new())),
            (ScreenId::Templates, Box::new(TemplatesScreen::new())),
            (ScreenId::SettingsInstall, Box::new(SettingsInstallScreen::new())),
            (ScreenId::CodexSettings, Box::new(CodexSettingsScreen::new())),
            (ScreenId::TmThemeEditor, Box::new(TmThemeEditorScreen::new())),
            (ScreenId::SaveExit, Box::new(SaveExitScreen::new())),
            (ScreenId::Wizard, Box::new(WizardScreen::new())),
            (ScreenId::Error, Box::new(ErrorScreen::new())),
        ];

        for (id, mut screen) in screens {
            let mut state = AppState::new(default_config(), id);
            terminal
                .draw(|frame| screen.draw(frame, &state))
                .expect("screen draws on a fresh HOME");
            screen.on_key(press(KeyCode::Null), &mut state);
        }
    });
}

#[test]
fn every_screen_tolerates_degenerate_drafts_on_a_fresh_home() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|_| {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        let drafts = [
            phosphorpulse::config::model::Config(serde_json::Map::new()),
            phosphorpulse::config::model::Config(
                serde_json::json!({"style": "lean", "language": "en"})
                    .as_object()
                    .expect("object-shaped degenerate draft")
                    .clone(),
            ),
        ];

        for draft in drafts {
            let screens: Vec<(ScreenId, Box<dyn Screen>)> = vec![
                (ScreenId::MainMenu, Box::new(MainMenuScreen::new())),
                (ScreenId::RowsSegments, Box::new(RowsSegmentsScreen::new())),
                (ScreenId::SubagentLine, Box::new(SubagentLineScreen::new())),
                (ScreenId::ColorsTheme, Box::new(ColorsThemeScreen::new())),
                (ScreenId::Templates, Box::new(TemplatesScreen::new())),
                (ScreenId::SettingsInstall, Box::new(SettingsInstallScreen::new())),
                (ScreenId::CodexSettings, Box::new(CodexSettingsScreen::new())),
                (ScreenId::TmThemeEditor, Box::new(TmThemeEditorScreen::new())),
                (ScreenId::SaveExit, Box::new(SaveExitScreen::new())),
                (ScreenId::Wizard, Box::new(WizardScreen::new())),
                (ScreenId::Error, Box::new(ErrorScreen::new())),
            ];

            for (id, mut screen) in screens {
                let mut state = AppState::new(draft.clone(), id);
                terminal
                    .draw(|frame| screen.draw(frame, &state))
                    .expect("screen draws with a degenerate draft");
                screen.on_key(press(KeyCode::Null), &mut state);
            }
        }
    });
}

#[test]
fn router_dispatches_all_three_start_states() {
    assert_eq!(
        dispatch(false, false, None),
        Route::Wizard {
            preserve_existing_config: false
        }
    );
    assert_eq!(
        dispatch(true, false, None),
        Route::Wizard {
            preserve_existing_config: true
        }
    );
    assert_eq!(dispatch(true, true, None), Route::MainMenu);
    assert_eq!(
        dispatch(true, true, Some(("/tmp/settings.json", "invalid JSON"))),
        Route::ErrorScreen {
            path: "/tmp/settings.json".into(),
            reason: "invalid JSON".into()
        }
    );
}

fn with_temp_home(settings: Option<serde_json::Value>, test: impl FnOnce(&std::path::Path)) {
    let root = std::env::temp_dir().join(format!(
        "phosphorpulse-router-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
        TEMP_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(root.join(".claude/phosphorpulse")).expect("create config dir");
    std::fs::write(
        root.join(".claude/phosphorpulse/settings.json"),
        serde_json::to_vec(&default_config()).expect("serialize valid own config"),
    )
    .expect("write own config");
    if let Some(settings) = settings {
        std::fs::write(
            root.join(".claude/settings.json"),
            serde_json::to_vec(&settings).expect("serialize Claude settings"),
        )
        .expect("write Claude settings");
    }
    test(&root);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn existing_own_config_with_unwired_statusline_routes_to_wizard() {
    with_temp_home(
        Some(serde_json::json!({"statusLine": {"command": "phosphorflux render"}})),
        |home| {
            assert_eq!(
                dispatch(
                    true,
                    statusline_is_wired_at(&home.join(".claude/settings.json")),
                    None,
                ),
                Route::Wizard {
                    preserve_existing_config: true
                }
            );
        },
    );
}

#[test]
fn existing_own_config_with_phosphorpulse_statusline_routes_to_main_menu() {
    with_temp_home(
        Some(serde_json::json!({"statusLine": {"command": "exec phosphorpulse render"}})),
        |home| {
            assert_eq!(
                dispatch(
                    true,
                    statusline_is_wired_at(&home.join(".claude/settings.json")),
                    None,
                ),
                Route::MainMenu
            );
        },
    );
}

#[test]
fn absent_own_config_routes_to_wizard() {
    with_temp_home(None, |home| {
        std::fs::remove_file(home.join(".claude/phosphorpulse/settings.json"))
            .expect("remove own config");
        assert_eq!(
            dispatch(
                false,
                statusline_is_wired_at(&home.join(".claude/settings.json")),
                None,
            ),
            Route::Wizard {
                preserve_existing_config: false
            }
        );
    });
}

#[test]
fn rows_tab_cycles_each_of_three_draft_rows() {
    let mut draft = default_config();
    draft.0.insert(
        "rows".into(),
        serde_json::json!([
            {"layout": "auto", "segments": ["model"]},
            {"layout": "auto", "segments": ["git"]},
            {"layout": "auto", "segments": ["dir"]}
        ]),
    );
    let mut state = AppState::new(draft, ScreenId::RowsSegments);
    let mut screen = RowsSegmentsScreen::new();
    let tab = KeyEvent::new_with_kind(KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Press);

    screen.on_key(tab, &mut state);
    assert_eq!(state.row_index, 1);
    screen.on_key(tab, &mut state);
    assert_eq!(state.row_index, 2);
    screen.on_key(tab, &mut state);
    assert_eq!(state.row_index, 0);
}

#[test]
fn codex_config_reader_accepts_quoted_project_table_before_tui() {
    let path =
        std::env::temp_dir().join(format!("phosphorpulse-codex-{}.toml", std::process::id()));
    std::fs::write(&path, "model = \"gpt-5\"\n[projects.\"/quoted.path\"]\ntrust_level = \"trusted\"\n\n[tui]\nstatus_line = [\"model-with-reasoning\", \"current-dir\", \"git-branch\", \"run-state\", \"codex-version\", \"context-used\", \"five-hour-limit\", \"weekly-limit\"]\nstatus_line_use_colors = true\ntheme = \"Matrix-Tron\"\n").unwrap();
    let snapshot = config_io::read_config(&path).expect("real-shaped config is readable");
    assert_eq!(snapshot.config.status_line.len(), 8);
    assert!(snapshot.config.status_line_use_colors);
    assert_eq!(snapshot.config.theme, "Matrix-Tron");
    let _ = std::fs::remove_file(path);
}

#[test]
fn codex_config_reader_preserves_all_real_status_line_ids() {
    let path = std::env::temp_dir().join(format!(
        "phosphorpulse-codex-ids-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, "[projects.\"quoted.path\"]\ntrust_level = \"trusted\"\n[tui]\nstatus_line = [\"model-with-reasoning\", \"current-dir\", \"git-branch\", \"run-state\", \"codex-version\", \"context-used\", \"five-hour-limit\", \"weekly-limit\"]\nstatus_line_use_colors = true\ntheme = \"Matrix-Tron\"\n").unwrap();
    let snapshot = config_io::read_config(&path).unwrap();
    assert_eq!(snapshot.config.status_line[0], "model-with-reasoning");
    assert_eq!(snapshot.config.status_line[7], "weekly-limit");
    let _ = std::fs::remove_file(path);
}

#[test]
fn codex_config_reader_accepts_top_level_keys_before_tui() {
    let path = std::env::temp_dir().join(format!(
        "phosphorpulse-codex-top-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, "model = \"gpt-5\"\napproval_policy = \"never\"\n[tui]\nstatus_line = [\"current-dir\"]\nstatus_line_use_colors = false\ntheme = \"Matrix-Tron\"\n").unwrap();
    let snapshot = config_io::read_config(&path).unwrap();
    assert_eq!(snapshot.config.status_line, ["current-dir"]);
    assert!(!snapshot.config.status_line_use_colors);
    let _ = std::fs::remove_file(path);
}

#[test]
fn codex_config_reader_keeps_theme_case() {
    let path = std::env::temp_dir().join(format!(
        "phosphorpulse-codex-theme-{}.toml",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "[tui]\nstatus_line = []\nstatus_line_use_colors = true\ntheme = \"Matrix-Tron\"\n",
    )
    .unwrap();
    assert_eq!(
        config_io::read_config(&path).unwrap().config.theme,
        "Matrix-Tron"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn present_codex_config_without_tui_defaults_draws_and_writes() {
    partial_codex_config_defaults_draws_and_writes("model = \"gpt-5\"\n# keep this comment\n", true);
}

#[test]
fn present_tui_without_status_line_defaults_and_writes() {
    partial_codex_config_defaults_draws_and_writes(
        "model = \"gpt-5\"\n[tui]\nstatus_line_use_colors = true\ntheme = \"\"\n",
        false,
    );
}

#[test]
fn present_tui_without_theme_defaults_and_writes() {
    partial_codex_config_defaults_draws_and_writes(
        "model = \"gpt-5\"\n[tui]\nstatus_line = [\"model-with-reasoning\", \"current-dir\", \"git-branch\", \"run-state\", \"codex-version\", \"context-used\", \"five-hour-limit\", \"weekly-limit\"]\nstatus_line_use_colors = true\n",
        false,
    );
}

fn partial_codex_config_defaults_draws_and_writes(contents: &str, draw_screens: bool) {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|home| {
        let own_config = home.join(".claude/phosphorpulse/settings.json");
        let codex_config = home.join(".codex/config.toml");
        std::fs::create_dir_all(own_config.parent().expect("own config parent"))
            .expect("create own config directory");
        std::fs::create_dir_all(codex_config.parent().expect("Codex config parent"))
            .expect("create Codex config directory");
        std::fs::write(
            &own_config,
            serde_json::to_vec(&default_config().0).expect("serialize own config"),
        )
        .expect("write own config");

        std::fs::write(&codex_config, contents).expect("write partial Codex config");
        let snapshot = config_io::read_config(&codex_config).expect("partial config is readable");
        assert_eq!(
            snapshot.config.status_line,
            config_io::KNOWN_STATUS_LINE_IDS.map(str::to_owned),
            "missing status line defaults",
        );
        assert!(snapshot.config.status_line_use_colors, "colors default");
        assert_eq!(snapshot.config.theme, "", "theme default");

        config_io::write_config(&snapshot, &snapshot.config).expect("partial config writes");
        let written = std::fs::read_to_string(&codex_config).expect("read completed Codex config");
        assert!(written.contains("model = \"gpt-5\""), "preserves unrelated content");
        if contents.contains("# keep this comment") {
            assert!(
                written.contains("# keep this comment"),
                "preserves unrelated formatting comments"
            );
        }
        assert!(written.contains("[tui]"), "writes tui table");
        assert!(written.contains("status_line = ["), "writes status line");
        assert!(written.contains("status_line_use_colors = true"), "writes colors");
        assert!(written.contains("theme = \"\""), "writes theme");

        if !draw_screens {
            return;
        }
        std::fs::write(&codex_config, contents).expect("restore partial Codex config");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        let mut menu = MainMenuScreen::new();
        let mut settings_state = AppState::new(default_config(), ScreenId::MainMenu);
        settings_state.set_selected(4);
        menu.on_key(press(KeyCode::Enter), &mut settings_state);
        assert_eq!(settings_state.screen, ScreenId::SettingsInstall);
        terminal
            .draw(|frame| SettingsInstallScreen::new().draw(frame, &settings_state))
            .expect("Settings & Install draws with no-tui Codex config");

        let mut codex_state = AppState::new(default_config(), ScreenId::MainMenu);
        codex_state.set_selected(5);
        menu.on_key(press(KeyCode::Enter), &mut codex_state);
        assert_eq!(codex_state.screen, ScreenId::CodexSettings);
        assert!(codex_state.codex_edit.is_some(), "partial config is editable");
        terminal
            .draw(|frame| CodexSettingsScreen::new().draw(frame, &codex_state))
            .expect("Codex Settings draws with no-tui config");
    });
}

/// S-11/R6: the export writer must expand the literal TUI `~/Desktop` input
/// before deriving the atomic-write temporary-file directory.
#[test]
fn template_export_expands_literal_home_desktop_path() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    let root = std::env::temp_dir().join(format!(
        "phosphorpulse-template-export-{}",
        std::process::id()
    ));
    let home = root.join("home");
    let templates = home.join(".claude/phosphorpulse/templates");
    std::fs::create_dir_all(home.join("Desktop")).expect("create temporary Desktop");
    let old_home = std::env::var_os("HOME");
    unsafe { std::env::set_var("HOME", &home) };

    let snapshot = serde_json::json!({"rows": [], "subagent": {"segments": []}, "gauge": {}, "segments": {}, "palette": {}});
    templates_io::save_template("saved", &snapshot, &templates).expect("save template");
    templates_io::export_template(
        "saved",
        std::path::Path::new("~/Desktop/x.json"),
        &templates,
    )
    .expect("literal ~/Desktop export succeeds");

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(home.join("Desktop/x.json")).expect("read exported template")
        )
        .expect("exported JSON"),
        snapshot,
    );
    unsafe {
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

/// S-11/R8: built-ins are code snapshots, not files under `templates/`.
#[test]
fn builtin_matrix_tron_exports_and_loads_without_template_file() {
    let root = std::env::temp_dir().join(format!(
        "phosphorpulse-builtin-template-{}",
        std::process::id()
    ));
    let templates = root.join("templates");
    let exported = root.join("matrix-tron.json");
    std::fs::create_dir_all(&root).expect("create temporary root");

    templates_io::export_template("matrix-tron", &exported, &templates)
        .expect("export built-in template");
    let imported = templates_io::import_template(&exported, "matrix-copy", &templates)
        .expect("exported built-in passes import validation");
    assert_eq!(
        imported["rows"][1]["segments"],
        serde_json::json!(["limit5h", "pomodoro", "version", "node", "python", "flex", "ctx", "limit7d"])
    );
    assert_eq!(
        templates_io::load_template("matrix-tron", &templates)
            .expect("load built-in template")["rows"][1]["segments"],
        serde_json::json!(["limit5h", "pomodoro", "version", "node", "python", "flex", "ctx", "limit7d"])
    );

    let _ = std::fs::remove_dir_all(root);
}

/// S-11/R9: saved templates are deletable, while built-ins remain code-owned.
#[test]
fn template_delete_removes_saved_file_and_rejects_builtin_name() {
    let root = std::env::temp_dir().join(format!(
        "phosphorpulse-template-delete-{}",
        std::process::id()
    ));
    let templates = root.join("templates");
    let saved = templates.join("saved.json");
    let builtin_file = templates.join("matrix-tron.json");
    let snapshot = serde_json::json!({"rows": []});

    templates_io::save_template("saved", &snapshot, &templates).expect("save template");
    std::fs::write(&builtin_file, b"must remain untouched").expect("plant builtin sentinel");
    templates_io::delete_template("saved", &templates).expect("delete saved template");

    assert!(!saved.exists(), "saved template file is removed");
    assert!(templates_io::delete_template("matrix-tron", &templates).is_err());
    assert_eq!(
        std::fs::read(&builtin_file).expect("read builtin sentinel"),
        b"must remain untouched"
    );

    let _ = std::fs::remove_dir_all(root);
}
