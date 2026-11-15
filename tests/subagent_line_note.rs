//! The Subagent Line screen tells users the feature needs Claude Code before 2.1.323.

use phosphorpulse::{
    config::default_config,
    tui::{
        app::{AppState, ScreenId},
        screens::{Screen, SubagentLineScreen},
    },
};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn render(language: &str) -> String {
    let mut config = default_config();
    config.0.insert("language".to_owned(), json!(language));
    let state = AppState::new(config, ScreenId::SubagentLine);
    let screen = SubagentLineScreen::new();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("test terminal");
    terminal
        .draw(|frame| screen.draw(frame, &state))
        .expect("Subagent Line draws");
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
        // Wide CJK glyphs leave padding cells in the buffer; compare without whitespace.
        .split_whitespace()
        .collect::<String>()
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect()
}

#[test]
fn subagent_line_screen_shows_version_note_in_english() {
    let rendered = render("en");
    assert!(
        rendered.contains(&squash(
            "Only works on Claude Code before 2.1.323 — later versions removed subagentStatusLine."
        )),
        "note missing from English screen"
    );
}

#[test]
fn subagent_line_screen_shows_version_note_in_zh_tw() {
    let rendered = render("zh-TW");
    assert!(
        rendered.contains(&squash(
            "僅適用 Claude Code 2.1.323 以前的版本；之後的版本已移除 subagentStatusLine。"
        )),
        "note missing from zh-TW screen"
    );
}
