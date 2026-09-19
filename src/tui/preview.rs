//! Preview rendering shell for the TUI.

use std::path::PathBuf;

use phosphorpulse::config::model::Config;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

/// Renders the fixed PreviewPane samples against a TUI draft.
pub fn render_preview(draft: &Config, main_sample: Value, subagent_sample: Value) -> String {
    render_preview_at_columns(draft, main_sample, subagent_sample, 80)
}

/// Left/right padding (in cells) applied to every preview line.
const PREVIEW_GUTTER: usize = 2;

/// Renders the fixed PreviewPane samples at the preview widget's real width.
pub fn render_preview_at_columns(
    draft: &Config,
    main_sample: Value,
    subagent_sample: Value,
    columns: usize,
) -> String {
    let config_dir = preview_config_dir();
    // Claude Code draws the status line inside a 2-cell gutter on both sides;
    // mirror it so the preview does not hug the pane border.
    let main_rendered = phosphorpulse::render::render_value_preview_at_columns(
        main_sample,
        Value::Object(draft.0.clone()),
        config_dir,
        columns.saturating_sub(2 * PREVIEW_GUTTER).max(1),
    );
    let gutter = " ".repeat(PREVIEW_GUTTER);
    let mut rendered = String::new();
    for line in main_rendered.lines() {
        rendered.push_str(&gutter);
        rendered.push_str(line);
        rendered.push('\n');
    }
    // Match PreviewPane.tsx: render the configured subagent segments through
    // the real subagent renderer, then prefix its first JSONL record.
    let subagent_rendered = phosphorpulse::render::render_subagent_value(
        subagent_sample,
        Value::Object(draft.0.clone()),
    );
    if let Some(content) = subagent_rendered
        .lines()
        .next()
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|line| {
            line.get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    {
        rendered.push('\n');
        rendered.push_str(&gutter);
        rendered.push_str("\x1b[1m● main\x1b[0m\n");
        rendered.push_str(&gutter);
        rendered.push_str("○ ");
        rendered.push_str(content.trim());
        rendered.push('\n');
    }
    rendered
}

fn preview_config_dir() -> PathBuf {
    std::env::temp_dir().join(format!("phosphorpulse-preview-{}", std::process::id()))
}

/// The fixed PreviewPane fixture mirrored from the frozen TypeScript source.
/// It supplies input for every registered main-row segment, so a configured
/// segment never looks empty merely because the preview fixture lacks data.
pub fn main_sample() -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    serde_json::json!({
        "session_id": "preview-sample",
        "session_name": "phosphorpulse",
        "fast_mode": true,
        "thinking": { "enabled": false },
        "output_style": { "name": "Concise" },
        "model": { "display_name": "Fable 5" },
        "effort": { "level": "medium" },
        "cwd": "~/example/project",
        "version": "2.1.220",
        "context_window": {
            "used_percentage": 10,
            "total_input_tokens": 20_000,
            "total_output_tokens": 1_200,
            "current_usage": {
                "cache_read_input_tokens": 18_500,
                "cache_creation_input_tokens": 900
            }
        },
        "rate_limits": {
            "five_hour": { "used_percentage": 0, "resets_at": now + 2 * 3600 + 30 * 60 },
            "seven_day": { "used_percentage": 85, "resets_at": now + 5 * 86400 + 19 * 3600 }
        },
        "cost": { "total_cost_usd": 0.42, "total_duration_ms": 2_700_000 }
    })
}

pub fn subagent_sample() -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    serde_json::json!({"tasks": [{
        "name": "subagent-name", "description": "build feature X",
        "model": "claude-sonnet-5", "contextWindowSize": 200_000,
        "tokenCount": 50_000, "startTime": now - 130_000, "effort": "medium"
    }]})
}

/// Converts terminal SGR output into ratatui spans.  Style state deliberately
/// survives newlines: row-builder may wrap between a color and its reset.
pub fn ansi_lines(input: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Vec::<Span<'static>>::new()];
    let mut text = String::new();
    let mut style = Style::default();
    let flush = |text: &mut String, style: Style, line: &mut Vec<Span<'static>>| {
        if !text.is_empty() {
            line.push(Span::styled(std::mem::take(text), style));
        }
    };
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            flush(&mut text, style, lines.last_mut().expect("preview line"));
            lines.push(Vec::new());
            i += 1;
        } else if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
                end += 1;
            }
            if end == bytes.len() {
                // A truncated escape is control data, never visible content.
                break;
            }
            if bytes[end] == b'm' {
                flush(&mut text, style, lines.last_mut().expect("preview line"));
                apply_sgr(&mut style, &input[start..end]);
            }
            i = end + 1;
        } else {
            let ch = input[i..].chars().next().expect("valid UTF-8");
            text.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut text, style, lines.last_mut().expect("preview line"));
    if lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    lines.into_iter().map(Line::from).collect()
}

fn apply_sgr(style: &mut Style, params: &str) {
    let values = if params.is_empty() {
        vec![0]
    } else {
        params
            .split(';')
            .map(|v| v.parse::<u16>().unwrap_or(0))
            .collect()
    };
    let mut i = 0;
    while i < values.len() {
        match values[i] {
            0 => *style = Style::default(),
            1 => *style = style.add_modifier(Modifier::BOLD),
            22 => *style = style.remove_modifier(Modifier::BOLD),
            30..=37 => *style = style.fg(Color::Indexed((values[i] - 30) as u8)),
            90..=97 => *style = style.fg(Color::Indexed((values[i] - 90 + 8) as u8)),
            39 => *style = style.fg(Color::Reset),
            38 if values.get(i + 1) == Some(&5) && values.get(i + 2).is_some() => {
                *style = style.fg(Color::Indexed(values[i + 2] as u8));
                i += 2;
            }
            38 if values.get(i + 1) == Some(&2) && i + 4 < values.len() => {
                *style = style.fg(Color::Rgb(
                    values[i + 2] as u8,
                    values[i + 3] as u8,
                    values[i + 4] as u8,
                ));
                i += 4;
            }
            40..=47 => *style = style.bg(Color::Indexed((values[i] - 40) as u8)),
            100..=107 => *style = style.bg(Color::Indexed((values[i] - 100 + 8) as u8)),
            49 => *style = style.bg(Color::Reset),
            48 if values.get(i + 1) == Some(&5) && values.get(i + 2).is_some() => {
                *style = style.bg(Color::Indexed(values[i + 2] as u8));
                i += 2;
            }
            48 if values.get(i + 1) == Some(&2) && i + 4 < values.len() => {
                *style = style.bg(Color::Rgb(
                    values[i + 2] as u8,
                    values[i + 3] as u8,
                    values[i + 4] as u8,
                ));
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
}
