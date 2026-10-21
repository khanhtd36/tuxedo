use std::io;

use super::types::{Density, Sort};
use crate::app::WeekStart;
use crate::config::Config;
use crate::theme::{self, Theme};

#[derive(Debug, Clone)]
pub struct Layout {
    pub left: bool,
    pub right: bool,
    pub line_num: bool,
    pub status_bar: bool,
    pub wrap_rows: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            left: false,
            right: false,
            line_num: true,
            status_bar: true,
            wrap_rows: true,
        }
    }
}

/// Default for `Prefs::wrap_max_lines`.
pub const DEFAULT_WRAP_MAX_LINES: usize = 2;

/// User-tunable preferences persisted to `Config`. Cycle/toggle methods return
/// the flash message for the caller to display, sidestepping any `&mut prefs`
/// + `&mut flash_state` borrow tangle on `App`.
#[derive(Debug, Clone)]
pub struct Prefs {
    theme_idx: usize,
    pub density: Density,
    pub sort: Sort,
    pub layout: Layout,
    pub show_done: bool,
    pub show_future: bool,
    /// Metadata keys whose `key:value` tokens are hidden from task rows.
    /// Config-only (no in-app toggle); see `Config::hidden_keys`.
    pub hidden_keys: Vec<String>,
    pub week_start: WeekStart,
    /// Whether the create/edit dialog opens the recurrence builder overlay
    /// for `rec:` / `/rec`. Config-only (no in-app toggle); see
    /// `Config::recurrence_builder`.
    pub recurrence_builder: bool,
    /// Most display lines one task row may take while `layout.wrap_rows` is
    /// on; `0` means no limit. Config-only (no in-app toggle); see
    /// `Config::wrap_max_lines`.
    pub wrap_max_lines: usize,
    /// Whether the terminal reports mouse events. See `Config::mouse`.
    pub mouse: bool,
}

impl Prefs {
    pub fn from_config(cfg: Config) -> Self {
        let theme_idx = cfg
            .theme
            .as_deref()
            .and_then(|name| theme::all().iter().position(|t| t.name == name))
            .unwrap_or(0);
        Self {
            theme_idx,
            density: cfg.density.unwrap_or(Density::Comfortable),
            sort: cfg.sort.unwrap_or(Sort::Priority),
            layout: Layout {
                left: cfg.show_left.unwrap_or(false),
                right: cfg.show_right.unwrap_or(false),
                line_num: cfg.show_line_num.unwrap_or(true),
                status_bar: cfg.show_status_bar.unwrap_or(true),
                wrap_rows: cfg.wrap_rows.unwrap_or(true),
            },
            show_done: cfg.show_done.unwrap_or(false),
            show_future: cfg.show_future.unwrap_or(false),
            hidden_keys: cfg.hidden_keys,
            week_start: cfg.week_start.unwrap_or(WeekStart::Sunday),
            recurrence_builder: cfg.recurrence_builder.unwrap_or(true),
            wrap_max_lines: cfg.wrap_max_lines.unwrap_or(DEFAULT_WRAP_MAX_LINES),
            mouse: cfg.mouse.unwrap_or(true),
        }
    }

    /// Display-line budget for one task row: the configured limit while
    /// wrapping, a single line otherwise.
    pub fn row_max_lines(&self) -> usize {
        if self.layout.wrap_rows {
            self.wrap_max_lines
        } else {
            1
        }
    }

    pub fn theme(&self) -> &'static Theme {
        let all = theme::all();
        all[self.theme_idx % all.len()]
    }

    pub fn theme_idx(&self) -> usize {
        self.theme_idx
    }

    /// Jump directly to a specific theme by index. Used by the screenshot
    /// example to render every theme; production code should call
    /// `cycle_theme` instead so the change persists with a flash message.
    pub fn set_theme_idx(&mut self, idx: usize) {
        self.theme_idx = idx % theme::all().len();
    }

    pub fn sort_label(&self) -> &'static str {
        self.sort.as_str()
    }

    pub fn cycle_theme(&mut self) -> String {
        self.theme_idx = (self.theme_idx + 1) % theme::all().len();
        format!("theme: {}", self.theme().name)
    }

    pub fn cycle_density(&mut self) -> String {
        self.density = match self.density {
            Density::Compact => Density::Comfortable,
            Density::Comfortable => Density::Cozy,
            Density::Cozy => Density::Compact,
        };
        format!("density: {}", self.density)
    }

    pub fn cycle_sort(&mut self) -> String {
        self.sort = match self.sort {
            Sort::Priority => Sort::Due,
            Sort::Due => Sort::File,
            Sort::File => Sort::Priority,
        };
        format!("sort: {}", self.sort)
    }

    pub fn toggle_left(&mut self) {
        self.layout.left = !self.layout.left;
    }

    pub fn toggle_right(&mut self) {
        self.layout.right = !self.layout.right;
    }

    pub fn toggle_line_num(&mut self) {
        self.layout.line_num = !self.layout.line_num;
    }

    pub fn toggle_wrap_rows(&mut self) {
        self.layout.wrap_rows = !self.layout.wrap_rows;
    }

    pub fn toggle_show_done(&mut self) {
        self.show_done = !self.show_done;
    }

    pub fn toggle_show_future(&mut self) {
        self.show_future = !self.show_future;
    }

    pub fn toggle_mouse(&mut self) {
        self.mouse = !self.mouse;
    }

    pub fn cycle_week_start(&mut self) -> String {
        self.week_start = match self.week_start {
            WeekStart::Sunday => WeekStart::Monday,
            WeekStart::Monday => WeekStart::Sunday,
        };
        format!("week_start: {}", self.week_start)
    }

    /// Persist to the XDG config path. Returns the IO error so the caller
    /// can flash it (writing to stderr from inside the alt-screen would
    /// corrupt the TUI). Saving is best-effort — callers that don't care
    /// about reporting can `let _ = prefs.save();`.
    ///
    /// Loads the on-disk config first so non-pref fields (like
    /// `share_token` / `share_port`, owned by the capture server) are
    /// preserved across pref toggles.
    pub fn save(&self) -> io::Result<()> {
        let mut cfg = Config::load();
        cfg.theme = Some(self.theme().name.to_string());
        cfg.density = Some(self.density);
        cfg.sort = Some(self.sort);
        cfg.show_left = Some(self.layout.left);
        cfg.show_right = Some(self.layout.right);
        cfg.show_line_num = Some(self.layout.line_num);
        cfg.show_status_bar = Some(self.layout.status_bar);
        cfg.show_done = Some(self.show_done);
        cfg.show_future = Some(self.show_future);
        cfg.wrap_rows = Some(self.layout.wrap_rows);
        cfg.hidden_keys = self.hidden_keys.clone();
        cfg.mouse = Some(self.mouse);
        cfg.save()
    }
}
