//! Update dialog: offers the newer release `U` found. Painted over the list;
//! keys are handled by `handle_update` in main.rs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, UpdateOffer};
use crate::update::release_url;

/// Columns of padding inside the border, each side.
const INNER_PAD: u16 = 2;
const MIN_INNER_W: u16 = 40;
const MAX_INNER_W: u16 = 72;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let Some(offer) = app.update_offer() else {
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border).bg(theme.panel))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "tuxedo",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · update ", Style::default().fg(theme.dim)),
        ]))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let inner = Rect {
        x: inner.x + INNER_PAD,
        width: inner.width.saturating_sub(INNER_PAD * 2),
        ..inner
    };

    let dim = Style::default().fg(theme.dim);
    let mut lines = Vec::new();
    if let Some(failure) = &offer.failure {
        lines.push(Line::from(Span::styled(
            failure.clone(),
            Style::default().fg(theme.overdue),
        )));
        lines.push(Line::default());
    }
    lines.push(Line::from(vec![
        Span::styled(
            concat!("tuxedo ", env!("CARGO_PKG_VERSION"), " → "),
            Style::default().fg(theme.fg),
        ),
        Span::styled(
            offer.version.clone(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(release_url(&offer.version), dim)));
    lines.push(Line::default());
    match &offer.command {
        None => lines.push(Line::from(Span::styled("y install · n cancel", dim))),
        Some(command) => {
            lines.push(Line::from(Span::styled(
                "This copy doesn't update itself. Instead:",
                Style::default().fg(theme.fg),
            )));
            lines.push(Line::from(Span::styled(
                command.clone(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Esc closes", dim)));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.panel).fg(theme.fg))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

/// Natural (width, height) for the dialog, wrapping long lines (a failure
/// message, a command) at [`MAX_INNER_W`].
pub fn size_for(app: &App) -> (u16, u16) {
    let Some(offer) = app.update_offer() else {
        return (0, 0);
    };
    let texts = texts(offer);
    let widest = texts.iter().map(|t| t.chars().count()).max().unwrap_or(0) as u16;
    let inner_w = widest.clamp(MIN_INNER_W, MAX_INNER_W);
    let rows: u16 = texts
        .iter()
        .map(|t| (t.chars().count() as u16).div_ceil(inner_w).max(1))
        .sum();
    // Border (2) on each axis, plus padding on each side horizontally.
    (inner_w + 2 + INNER_PAD * 2, rows + 2)
}

/// The dialog's lines as plain text, for sizing. Mirrors [`render`].
fn texts(offer: &UpdateOffer) -> Vec<String> {
    let mut t = Vec::new();
    if let Some(failure) = &offer.failure {
        t.push(failure.clone());
        t.push(String::new());
    }
    t.push(format!(
        "tuxedo {} → {}",
        env!("CARGO_PKG_VERSION"),
        offer.version
    ));
    t.push(release_url(&offer.version));
    t.push(String::new());
    match &offer.command {
        None => t.push("y install · n cancel".into()),
        Some(command) => {
            t.push("This copy doesn't update itself. Instead:".into());
            t.push(command.clone());
            t.push(String::new());
            t.push("Esc closes".into());
        }
    }
    t
}
