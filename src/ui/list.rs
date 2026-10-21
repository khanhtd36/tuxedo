use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, GroupKey, ListDueBucket, Mode, View};
use crate::core::filter;
use crate::theme::Theme;
use crate::ui::{header, keep_cursor_row_visible, task_row};

/// Display lines for the list view plus a parallel line -> position-in-
/// `visible_indices()` map (`None` for blank separators and group headers).
/// Shared by `render` and [`hit_test`] — a click can never land against a
/// layout `render` didn't actually draw (see docs/adr/0002).
pub struct Built<'a> {
    pub lines: Vec<Line<'a>>,
    pub cursor_lines: Option<std::ops::Range<usize>>,
    pub line_items: Vec<Option<usize>>,
}

/// `match_term` is taken by reference rather than resolved inside `build`
/// itself: it borrows a caller-local (`resolved_needle`, below), and tying
/// its lifetime to the same `'a` as `app` would force `Built` to outlive that
/// local — which only `render` needs (it uses `Built` immediately). `hit_test`
/// passes `None`: highlighting never changes a row's line count, only its
/// color, so the hit-test doesn't need to resolve the needle at all.
fn build<'a>(app: &'a App, width: u16, match_term: Option<&'a str>) -> Built<'a> {
    let theme = app.theme();
    let visible = app.visible_indices();
    let groups = app.visible_groups();
    let mut lines: Vec<Line> = Vec::new();
    let mut line_items: Vec<Option<usize>> = Vec::new();
    let mut cursor_lines: Option<std::ops::Range<usize>> = None;

    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "   no tasks match".to_string(),
            Style::default().fg(theme.dim),
        )));
        line_items.push(None);
    } else {
        let blank = super::density_blank_lines(app.prefs.density);
        let wrap = task_row::RowWrap {
            width: usize::from(width),
            max_lines: app.prefs.row_max_lines(),
        };
        let counts = group_counts(groups);
        let last = visible.len().saturating_sub(1);
        let mut last_group: Option<&GroupKey> = None;

        for (i, (&abs, gk)) in visible.iter().zip(groups.iter()).enumerate() {
            // Emit a section header on group transitions. `GroupKey::None`
            // means the active sort is `Sort::File`; we never render a header
            // for it, so the layout is identical to the pre-grouping version.
            if !matches!(gk, GroupKey::None) && last_group != Some(gk) {
                if !lines.is_empty() {
                    push_blanks(&mut lines, &mut line_items, blank);
                }
                lines.push(group_header(theme, gk, counts.lookup(gk)));
                line_items.push(None);
                last_group = Some(gk);
            }

            let task = &app.tasks()[abs];
            let opts = task_row::RowOpts {
                idx_label: i,
                cursor: i == app.cursor && app.mode != Mode::Help && app.mode != Mode::Settings,
                multi_mode: app.effective_mode() == Mode::Visual,
                multi_checked: app.selection.is_selected(abs),
                selected: app.selection.is_selected(abs),
                show_line_num: app.prefs.layout.line_num,
                match_term,
                today: app.today(),
                hidden_keys: &app.prefs.hidden_keys,
            };
            let row = task_row::build_lines(task, opts, theme, wrap);
            if i == app.cursor {
                cursor_lines = Some(lines.len()..lines.len() + row.len());
            }
            line_items.extend(std::iter::repeat_n(Some(i), row.len()));
            lines.extend(row);
            if matches!(gk, GroupKey::None) && i != last {
                for _ in 0..blank {
                    lines.push(Line::raw(""));
                    line_items.push(None);
                }
            }
        }
    }

    Built {
        lines,
        cursor_lines,
        line_items,
    }
}

/// Position in `visible_indices()` the given body-relative display line (0 =
/// top of the body area, already adjusted for scroll) lands on, or `None` for
/// a blank separator / group header line.
pub fn hit_test(app: &App, width: u16, line: usize) -> Option<usize> {
    build(app, width, None)
        .line_items
        .get(line)
        .copied()
        .flatten()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.bg));

    let (header_area, body_area) = super::header_body_split(area);

    let filter_label = header::filter_label(&app.filter);
    header::render(
        frame,
        header_area,
        theme,
        header::HeaderProps {
            title: Some(&display_path(&app.file_path)),
            // title: None,
            // file: &display_path(&app.file_path),
            count: app.visible_indices().len(),
            sort: app.sort_label(),
            filter: filter_label.as_deref(),
        },
    );

    if app.tasks().is_empty() {
        // The welcome overlay covers the same ground on first run; drawing the
        // empty-state card underneath stacks two boxes on top of each other.
        if app.mode != Mode::Welcome {
            crate::ui::empty::render(frame, body_area, app);
        }
        return;
    }

    // Highlight only the free-text part — a `due:` term filters by date and
    // never appears verbatim in the body.
    let resolved_needle = (!app.filter.search.is_empty())
        .then(|| filter::resolve_needle(&app.filter.search, app.today()));
    let match_term: Option<&str> = resolved_needle
        .as_ref()
        .and_then(|n| (!n.text.is_empty()).then_some(n.text.as_str()));

    let built = build(app, body_area.width, match_term);
    let scroll_cell = &app.view_scroll[View::List.idx()];
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

fn display_path(p: &std::path::Path) -> String {
    if let Some(home) = crate::xdg::home_dir()
        && let Ok(rel) = p.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    p.display().to_string()
}

/// Tally rows per `GroupKey` so each header can show its count without an
/// extra rescan during the render loop. Keyed by a stable string form so
/// `GroupKey::ListPriority(None)` and `ListPriority(Some('A'))` don't collide.
struct GroupCounts {
    inner: std::collections::HashMap<String, usize>,
}

impl GroupCounts {
    fn lookup(&self, gk: &GroupKey) -> usize {
        self.inner.get(&group_count_key(gk)).copied().unwrap_or(0)
    }
}

fn group_counts(groups: &[GroupKey]) -> GroupCounts {
    let mut inner: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for g in groups {
        if matches!(g, GroupKey::None) {
            continue;
        }
        *inner.entry(group_count_key(g)).or_insert(0) += 1;
    }
    GroupCounts { inner }
}

fn group_count_key(gk: &GroupKey) -> String {
    match gk {
        GroupKey::ListPriority(Some(c)) => format!("p:{c}"),
        GroupKey::ListPriority(None) => "p:_".to_string(),
        GroupKey::ListDue(b) => format!("d:{}", b.label()),
        // Not produced for List view; encode defensively.
        GroupKey::ArchiveDate(d) => format!("a:{d}"),
        GroupKey::None => String::new(),
    }
}

fn group_header<'a>(theme: &Theme, gk: &GroupKey, count: usize) -> Line<'a> {
    let (label, color) = match gk {
        GroupKey::ListPriority(Some(c)) => (format!("PRIORITY {c}"), theme.priority_color(*c)),
        GroupKey::ListPriority(None) => ("NO PRIORITY".to_string(), theme.dim),
        GroupKey::ListDue(b) => (b.label().to_string(), due_bucket_color(theme, *b)),
        // Defensive fallthrough — not produced under List view.
        GroupKey::ArchiveDate(d) => (d.clone(), theme.accent),
        GroupKey::None => (String::new(), theme.fg),
    };

    let char_count = count.to_string().len() + label.len();
    let divider_length = 80 - char_count;

    Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("({})", count), Style::default().fg(theme.dim)),
        Span::raw("  "),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "─".repeat(divider_length),
            Style::default().fg(theme.border),
        ),
    ])
}

fn due_bucket_color(theme: &Theme, b: ListDueBucket) -> Color {
    match b {
        ListDueBucket::Overdue => theme.overdue,
        ListDueBucket::Today => theme.today,
        ListDueBucket::ThisWeek => theme.accent,
        ListDueBucket::NextWeek => theme.accent,
        ListDueBucket::Later => theme.accent,
        ListDueBucket::NoDue => theme.dim,
    }
}

fn push_blanks(lines: &mut Vec<Line>, line_items: &mut Vec<Option<usize>>, n: usize) {
    for _ in 0..n {
        lines.push(Line::raw(" "));
        line_items.push(None);
    }
}
