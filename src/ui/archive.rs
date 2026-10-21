use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, GroupKey, Mode, View};
use crate::ui::{header, keep_cursor_row_visible, task_row};

/// Display lines for the archive view plus a parallel line -> position-in-
/// `visible_indices()` map. Shared by `render` and [`hit_test`] — see
/// docs/adr/0002.
pub struct Built<'a> {
    pub lines: Vec<Line<'a>>,
    pub cursor_lines: Option<std::ops::Range<usize>>,
    pub line_items: Vec<Option<usize>>,
}

fn build(app: &App, width: u16) -> Built<'_> {
    let theme = app.theme();
    let visible = app.visible_indices();
    let groups = app.visible_groups();
    let blank = super::density_blank_lines(app.prefs.density);
    let cursor_active = app.mode != Mode::Help && app.mode != Mode::Settings;

    let mut lines: Vec<Line> = Vec::new();
    let mut line_items: Vec<Option<usize>> = Vec::new();
    let mut cursor_lines: Option<std::ops::Range<usize>> = None;

    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "   no completed tasks yet".to_string(),
            Style::default().fg(theme.dim),
        )));
        line_items.push(None);
        return Built {
            lines,
            cursor_lines,
            line_items,
        };
    }

    // Pre-count rows per date so each header can show "{date}  N completed".
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for g in groups {
        if let GroupKey::ArchiveDate(d) = g {
            *counts.entry(d.as_str()).or_insert(0) += 1;
        }
    }

    let mut last_date: Option<&str> = None;
    let wrap = task_row::RowWrap {
        width: usize::from(width),
        max_lines: app.prefs.row_max_lines(),
    };

    for (i, (&abs, gk)) in visible.iter().zip(groups.iter()).enumerate() {
        let date = match gk {
            GroupKey::ArchiveDate(d) => d.as_str(),
            _ => continue,
        };
        if last_date != Some(date) {
            if !lines.is_empty() {
                for _ in 0..blank {
                    lines.push(Line::raw(" "));
                    line_items.push(None);
                }
            }
            let count = *counts.get(date).unwrap_or(&0);
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    date.to_string(),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{} completed", count),
                    Style::default().fg(theme.dim),
                ),
            ]));
            line_items.push(None);
            last_date = Some(date);
        }

        let task = &app.archive().tasks()[abs];
        let opts = task_row::RowOpts {
            idx_label: i,
            cursor: i == app.cursor && cursor_active,
            multi_mode: false,
            multi_checked: false,
            selected: false,
            show_line_num: app.prefs.layout.line_num,
            match_term: None,
            today: app.today(),
            hidden_keys: &app.prefs.hidden_keys,
        };
        let row = task_row::build_lines(task, opts, theme, wrap);
        if i == app.cursor {
            cursor_lines = Some(lines.len()..lines.len() + row.len());
        }
        line_items.extend(std::iter::repeat_n(Some(i), row.len()));
        lines.extend(row);
    }

    Built {
        lines,
        cursor_lines,
        line_items,
    }
}

/// Position in `visible_indices()` the given body-relative display line lands
/// on, or `None` for a blank separator / date header line.
pub fn hit_test(app: &App, width: u16, line: usize) -> Option<usize> {
    build(app, width).line_items.get(line).copied().flatten()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));

    let (header_area, body_area) = super::header_body_split(area);

    header::render(
        frame,
        header_area,
        theme,
        header::HeaderProps {
            title: Some("done.txt"),
            // file: "completed",
            count: app.archive().len(),
            sort: "completion-date",
            filter: None,
        },
    );

    let built = build(app, body_area.width);
    let scroll_cell = &app.view_scroll[View::Archive.idx()];
    let scroll = keep_cursor_row_visible(
        scroll_cell.get(),
        built.cursor_lines,
        body_area.height,
        built.lines.len(),
    );
    scroll_cell.set(scroll);

    let para = Paragraph::new(built.lines)
        .style(Style::default().bg(theme.bg).fg(theme.fg))
        .scroll((scroll, 0));
    frame.render_widget(para, body_area);
}
