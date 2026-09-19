use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use phosphorpulse::{
    config::{self, CommandSpec, model::Config},
    tui::{
        app::{AppState, ScreenId, SegmentPickerTarget, UiMode},
        router::{Route, dispatch},
        screens::{
            ColorsThemeScreen, MainMenuScreen, RowsSegmentsScreen, Screen, TemplatesScreen,
        },
    },
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};
use serde_json::{Value, json};
use std::sync::Mutex;

static HOME_LOCK: Mutex<()> = Mutex::new(());
static TEMP_DIR_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

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

fn draw(screen: &impl Screen, state: &AppState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| screen.draw(frame, state))
        .expect("draw screen");
    terminal.backend().buffer().clone()
}

fn rendered(buffer: &Buffer) -> String {
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn type_text(screen: &mut impl Screen, state: &mut AppState, input: &str) {
    for character in input.chars() {
        screen.on_key(press(KeyCode::Char(character)), state);
    }
}

fn command_specs(draft: &Config) -> std::collections::BTreeMap<String, CommandSpec> {
    config::commands(&Value::Object(draft.0.clone()))
}

fn command_focus_index(draft: &Config, name: &str, field_offset: usize) -> usize {
    const BASE_COLORS_FOCUS_LEN: usize = 27;
    let command_index = command_specs(draft)
        .keys()
        .position(|candidate| candidate == name)
        .expect("command focus exists");
    BASE_COLORS_FOCUS_LEN + command_index * 6 + field_offset
}

fn focus_command(state: &mut AppState, name: &str, field_offset: usize) {
    state.set_selected(command_focus_index(&state.draft, name, field_offset));
}

fn reversed_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
    let width = buffer.area().width;
    buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.modifier.contains(Modifier::REVERSED))
        .map(|(index, _)| ((index as u16) % width, (index as u16) / width))
        .collect()
}

fn cell_symbol(buffer: &Buffer, x: u16, y: u16) -> &str {
    buffer
        .cell((x, y))
        .expect("cell coordinate is inside the terminal")
        .symbol()
}

/// REQ-04 / S-07
#[test]
fn test_s07_add_edit_delete() {
    let _home_lock = HOME_LOCK.lock().expect("lock HOME for this process");
    with_completely_empty_home(|_| {
        let mut draft = Config::defaults();
        draft.0.insert(
            "rows".into(),
            json!([
                {"layout": "auto", "segments": ["model", "ctx", "cmd:k8s"]},
                {"layout": "auto", "segments": ["git", "cmd:k8s"]}
            ]),
        );
        draft.0.insert(
            "commands".into(),
            json!({"k8s": {"command": "kubectl x"}}),
        );
        draft.0.insert(
            "segments".into(),
            json!({"cmd:k8s": {"fg": "#00FF41"}}),
        );

        assert_eq!(dispatch(true, true, None), Route::MainMenu);
        let mut state = AppState::new(draft, ScreenId::MainMenu);
        let mut menu = MainMenuScreen::new();
        menu.on_key(press(KeyCode::Down), &mut state);
        menu.on_key(press(KeyCode::Down), &mut state);
        menu.on_key(press(KeyCode::Enter), &mut state);
        assert_eq!(state.screen, ScreenId::ColorsTheme);
        let mut colors = ColorsThemeScreen::new();

        // (1) Invalid names stay in CommandName and do not change commands.
        let commands_before = command_specs(&state.draft);
        colors.on_key(press(KeyCode::Char('a')), &mut state);
        type_text(&mut colors, &mut state, "bad name");
        colors.on_key(press(KeyCode::Enter), &mut state);
        let buffer = draw(&colors, &state);
        assert!(
            rendered(&buffer).contains("name must match"),
            "case (1): invalid-name warning must be rendered"
        );
        assert_eq!(state.mode, UiMode::CommandName);
        assert_eq!(command_specs(&state.draft), commands_before);

        // (2) A valid name and command create a fully defaulted command.
        for _ in 0.."bad name".chars().count() {
            colors.on_key(press(KeyCode::Backspace), &mut state);
        }
        type_text(&mut colors, &mut state, "battery");
        colors.on_key(press(KeyCode::Enter), &mut state);
        type_text(&mut colors, &mut state, "pmset -g batt");
        colors.on_key(press(KeyCode::Enter), &mut state);
        assert_eq!(
            command_specs(&state.draft)["battery"],
            CommandSpec {
                command: "pmset -g batt".into(),
                timeout_ms: 1_000,
                ttl_sec: 5,
                max_width: 24,
                preserve_colors: false,
            }
        );
        assert_eq!(
            state.selected(),
            command_focus_index(&state.draft, "battery", 0),
            "focus is on CmdFg(\"battery\")"
        );
        let mut picker_state = state.clone();
        picker_state.mode = UiMode::SegmentPicker {
            target: SegmentPickerTarget::Main,
            insert_after: false,
            position: 0,
        };
        picker_state.set_selected(19);
        let picker_buffer = draw(&RowsSegmentsScreen::new(), &picker_state);
        assert!(
            rendered(&picker_buffer).contains("> cmd:battery"),
            "main_segment_ids includes cmd:battery in the picker"
        );

        // (3) Enter on CmdCommand edits the existing string.
        focus_command(&mut state, "battery", 1);
        colors.on_key(press(KeyCode::Enter), &mut state);
        for _ in 0..4 {
            colors.on_key(press(KeyCode::Backspace), &mut state);
        }
        type_text(&mut colors, &mut state, "ps");
        colors.on_key(press(KeyCode::Enter), &mut state);
        assert_eq!(command_specs(&state.draft)["battery"].command, "pmset -g ps");

        // (4) Empty edits warn and Esc preserves the prior command.
        colors.on_key(press(KeyCode::Enter), &mut state);
        while !state.input.is_empty() {
            colors.on_key(press(KeyCode::Backspace), &mut state);
        }
        colors.on_key(press(KeyCode::Enter), &mut state);
        let buffer = draw(&colors, &state);
        assert!(rendered(&buffer).contains("command must not be empty"));
        colors.on_key(press(KeyCode::Esc), &mut state);
        assert_eq!(command_specs(&state.draft)["battery"].command, "pmset -g ps");

        // (5) Delete confirmation consumes non-y keys, then removes all references.
        focus_command(&mut state, "k8s", 0);
        colors.on_key(press(KeyCode::Char('d')), &mut state);
        colors.on_key(press(KeyCode::Char('a')), &mut state);
        assert_ne!(state.mode, UiMode::CommandName);
        assert!(command_specs(&state.draft).contains_key("k8s"));
        colors.on_key(press(KeyCode::Char('d')), &mut state);
        colors.on_key(press(KeyCode::Char('y')), &mut state);
        assert!(!command_specs(&state.draft).contains_key("k8s"));
        assert!(
            state.draft.0["rows"]
                .as_array()
                .expect("rows array")
                .iter()
                .all(|row| row["segments"]
                    .as_array()
                    .expect("segments array")
                    .iter()
                    .all(|segment| segment != "cmd:k8s"))
        );
        assert!(
            !state.draft.0["segments"]
                .as_object()
                .expect("segments object")
                .contains_key("cmd:k8s")
        );
        let buffer = draw(&colors, &state);
        assert!(rendered(&buffer).contains("removed from 2 rows"));

        // (6) Numeric values clamp and the boolean toggles.
        focus_command(&mut state, "battery", 3);
        for _ in 0..5 {
            colors.on_key(press(KeyCode::Left), &mut state);
        }
        assert_eq!(command_specs(&state.draft)["battery"].ttl_sec, 1);
        focus_command(&mut state, "battery", 5);
        colors.on_key(press(KeyCode::Right), &mut state);
        assert!(command_specs(&state.draft)["battery"].preserve_colors);

        // (7) A long command scrolls horizontally but retains one visible cursor cell.
        focus_command(&mut state, "battery", 1);
        colors.on_key(press(KeyCode::Enter), &mut state);
        type_text(&mut colors, &mut state, &"x".repeat(200));
        let buffer = draw(&colors, &state);
        let cursors = reversed_cells(&buffer);
        assert_eq!(cursors.len(), 1, "case (7): exactly one reversed cursor cell");
        assert!(cursors[0].0 < WIDTH && cursors[0].1 < HEIGHT);

        // (8) Templates uses the same cursor renderer, immediately after the input.
        colors.on_key(press(KeyCode::Esc), &mut state);
        colors.on_key(press(KeyCode::Esc), &mut state);
        assert_eq!(state.screen, ScreenId::MainMenu);
        let mut menu = MainMenuScreen::new();
        for _ in 0..3 {
            menu.on_key(press(KeyCode::Down), &mut state);
        }
        menu.on_key(press(KeyCode::Enter), &mut state);
        assert_eq!(state.screen, ScreenId::Templates);
        let mut templates = TemplatesScreen::new();
        templates.on_key(press(KeyCode::Char('s')), &mut state);
        type_text(&mut templates, &mut state, "abc");
        let buffer = draw(&templates, &state);
        let cursors = reversed_cells(&buffer);
        assert_eq!(cursors.len(), 1, "case (8): exactly one reversed cursor cell");
        let (cursor_x, cursor_y) = cursors[0];
        assert!(cursor_x < WIDTH && cursor_y < HEIGHT);
        assert!(cursor_x > 0, "cursor follows the typed text");
        assert_eq!(cell_symbol(&buffer, cursor_x - 1, cursor_y), "c");
    });
}
