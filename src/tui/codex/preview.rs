//! Codex preview shell.

use phosphorpulse::render;
use phosphorpulse::tui::codex::tmtheme::{TmTheme, read_tmtheme};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum PreviewError {
    Io(String),
    Parse(String),
}

/// Renders the Codex status-line sample using colours resolved from a tmTheme file.
pub fn render_preview_line(
    status_line: &[String],
    use_colors: bool,
    theme_path: &Path,
    color_depth: &str,
) -> Result<String, PreviewError> {
    let theme = read_tmtheme(theme_path).map_err(|error| PreviewError::Parse(format!("{error:?}")))?;
    let rendered = status_line
        .iter()
        .map(|id| render_item(id, &theme.theme, use_colors, color_depth))
        .collect::<Vec<_>>()
        .join(" | ");
    Ok(if use_colors {
        rendered
    } else {
        format!("\x1b[2m{rendered}\x1b[0m")
    })
}

fn render_item(id: &str, theme: &TmTheme, use_colors: bool, color_depth: &str) -> String {
    let (text, scopes, fallback) = match id {
        "model-with-reasoning" => ("gpt-5.2", &["entity.name.type", "support.type", "variable"][..], "#00FFFF"),
        "current-dir" => ("phosphorpulse", &["string", "markup.underline.link"][..], "#00FF00"),
        "git-branch" => ("main", &["entity.name.function", "entity.name.tag"][..], "#FF00FF"),
        "run-state" => ("idle", &["keyword.control", "keyword"][..], "#00FFFF"),
        "context-used" => ("62%", &["constant.numeric", "constant"][..], "#00FF00"),
        "five-hour-limit" => ("5h 61%", &["constant.language", "storage.type"][..], "#FF00FF"),
        "weekly-limit" => ("78%", &["constant.language", "storage.type"][..], "#FF00FF"),
        "codex-version" => ("codex", &["comment", "constant.other"][..], "#00FFFF"),
        unknown => (unknown, &[][..], "#00FFFF"),
    };
    let foreground = scopes
        .iter()
        .find_map(|scope| theme.resolved_scope_foreground(scope).filter(|colour| is_six_digit_hex(colour)))
        .or_else(|| theme.global_foreground().filter(|colour| is_six_digit_hex(colour)))
        .unwrap_or(fallback);
    match use_colors {
        true => format!(
            "{}{}\x1b[0m",
            render::color(foreground, color_depth, false),
            text
        ),
        false => text.to_owned(),
    }
}

fn is_six_digit_hex(color: &str) -> bool {
    matches!(color.strip_prefix('#'), Some(hex) if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
