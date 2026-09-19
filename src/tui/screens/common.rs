use crate::tui::{
    app::AppState,
    chrome,
    i18n::{Key, t},
    preview,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;

use phosphorpulse::jsx::width::display_width;

pub const ACCENT: Color = Color::Rgb(0, 255, 65);
pub const TEXT: Color = Color::Rgb(0, 207, 65);
pub const DIM: Color = Color::Rgb(0, 143, 17);
pub const ERROR: Color = Color::Rgb(255, 55, 55);

pub fn input_line(
    prompt: &str,
    input: &str,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    let graphemes = input.graphemes(true).collect::<Vec<_>>();
    let available = usize::from(width).saturating_sub(1);
    let mut start = graphemes.len();
    let mut used = 0;
    while start > 0 {
        let grapheme_width = display_width(graphemes[start - 1]);
        if used + grapheme_width > available {
            break;
        }
        used += grapheme_width;
        start -= 1;
    }
    let visible = graphemes[start..].concat();
    vec![
        Line::from(Span::styled(
            prompt.to_owned(),
            Style::default().fg(DIM),
        )),
        Line::from(vec![
            Span::styled(visible, Style::default().fg(TEXT)),
            Span::styled(
                " ",
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::REVERSED),
            ),
        ]),
    ]
}

pub fn areas(frame: &Frame, state: &AppState) -> Option<[Rect; 4]> {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        return None;
    }
    // `PreviewPane` has one rendered line per main row plus its subagent
    // sample. Reserve its actual content height (and the top border) rather
    // than clipping the appended subagent line in a fixed three-row pane.
    let lines = preview::render_preview_at_columns(
        &state.draft,
        preview::main_sample(),
        preview::subagent_sample(),
        area.width as usize,
    )
    .lines()
    .count()
    .max(1);
    let preview_height = (lines + 1).min(u16::MAX as usize) as u16;
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(preview_height),
    ])
    .split(area);
    Some([chunks[0], chunks[1], chunks[2], chunks[3]])
}

pub fn too_small(frame: &mut Frame, state: &AppState) {
    frame.render_widget(
        Paragraph::new(t(state.lang, &Key::CommonTerminalTooSmall, &[]))
            .style(Style::default().fg(DIM)),
        frame.area(),
    );
}

pub fn header(frame: &mut Frame, area: Rect, name: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            name.to_string(),
            Style::default()
                .fg(ACCENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )]))
        .style(Style::default().fg(DIM)),
        area,
    );
}

pub fn footer(frame: &mut Frame, area: Rect, hint: &str) {
    let title = chrome::main_menu_title();
    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(title.len() as u16)])
        .split(area);
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(DIM)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Right)
            .style(Style::default().fg(DIM)),
        chunks[1],
    );
}

/// Keeps the version chrome visible while surfacing a footer-area error.
pub fn footer_error(frame: &mut Frame, area: Rect, error: &str) {
    let title = chrome::main_menu_title();
    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(title.len() as u16)])
        .split(area);
    frame.render_widget(
        Paragraph::new(error).style(Style::default().fg(ERROR)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Right)
            .style(Style::default().fg(DIM)),
        chunks[1],
    );
}

pub fn preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let main = preview::main_sample();
    let sub = preview::subagent_sample();
    let rendered = preview::render_preview_at_columns(&state.draft, main, sub, area.width as usize);
    frame.render_widget(
        Paragraph::new(preview::ansi_lines(&rendered))
            .block(Block::default().borders(Borders::TOP).title(t(
                state.lang,
                &Key::CommonPreview,
                &[],
            )))
            .style(Style::default().fg(TEXT)),
        area,
    );
}

pub(crate) fn list_scroll_offset(
    selected_line: usize,
    total_lines: usize,
    visible: usize,
) -> usize {
    if visible == 0 || total_lines <= visible {
        0
    } else {
        selected_line
            .saturating_sub(visible - 1)
            .min(total_lines - visible)
    }
}

pub(crate) fn scrolled_list<'a>(
    body: impl Into<Text<'a>>,
    selected_line: usize,
    total_lines: usize,
    area: Rect,
) -> Paragraph<'a> {
    let offset = list_scroll_offset(selected_line, total_lines, area.height as usize);
    Paragraph::new(body).scroll((offset as u16, 0))
}

pub fn move_focus(state: &mut AppState, len: usize, delta: i8) {
    if len == 0 {
        return;
    }
    let next = (state.selected() as i32 + delta as i32).rem_euclid(len as i32) as usize;
    state.set_selected(next);
}
