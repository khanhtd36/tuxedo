use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::theme;

/// Theme index the given absolute terminal row lands on within the picker's
/// bordered box at `outer`, or `None` outside the list area. Mirrors
/// `render`'s layout split (see docs/adr/0002). The theme list is unwindowed
/// (every theme always shows), unlike the command palette.
pub fn hit_test(outer: Rect, row: u16) -> Option<usize> {
    let inner = Rect {
        x: outer.x + 1,
        y: outer.y + 1,
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    };
    let [list_area, _footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    if row < list_area.y || row >= list_area.y + list_area.height {
        return None;
    }
    let idx = usize::from(row - list_area.y);
    (idx < theme::all().len()).then_some(idx)
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let all = theme::all();

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
            Span::styled(" · themes ", Style::default().fg(theme.dim)),
        ]))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bg = Style::default().bg(theme.panel).fg(theme.fg);

    let [list_area, footer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let cur = app.prefs.theme_idx();

    let lines: Vec<Line> = all
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_sel = i == cur;
            let row_bg = if is_sel { theme.cursor } else { theme.panel };
            let sigil_fg = if is_sel { theme.accent } else { row_bg };
            Line::from(vec![
                Span::styled(
                    if is_sel { " ▶ " } else { "   " },
                    Style::default()
                        .fg(sigil_fg)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    t.name,
                    Style::default()
                        .fg(theme.fg)
                        .bg(row_bg)
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines).style(bg), list_area);

    let footer = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "j/k/T",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" navigate · ", Style::default().fg(theme.dim)),
        Span::styled(
            "Enter",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select · ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", Style::default().fg(theme.dim)),
    ])
    .style(bg);
    frame.render_widget(Paragraph::new(footer).style(bg), footer_area);
}
