//! In-app update: `U` checks for a newer fork release, the update dialog
//! offers it, and `y` self-updates on a background thread while the TUI
//! stays usable. The status bar follows along (see [`App::update_note`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use super::{App, Mode};
use crate::update::{self, InstallKind, Latest, Step};

/// Installs a version in place of an executable, reporting progress; see
/// [`update::install::install`].
pub type InstallFn = fn(&str, &Path, &mut dyn FnMut(Step)) -> Result<(), String>;

/// The network and filesystem work behind an in-app update. Tests swap in
/// fakes so they never touch GitHub or the running executable.
#[derive(Clone, Copy)]
pub struct UpdateOps {
    pub check: fn() -> Result<Latest, String>,
    pub install: InstallFn,
    pub exe: fn() -> Option<PathBuf>,
}

impl Default for UpdateOps {
    fn default() -> Self {
        Self {
            check: update::check_now,
            install: update::install::install,
            exe: || std::env::current_exe().ok(),
        }
    }
}

/// What the update dialog offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: String,
    /// What to run instead, for a copy that doesn't self-update. `None`
    /// means `y` installs.
    pub command: Option<String>,
    /// The previous attempt's failure, shown atop the dialog.
    pub failure: Option<String>,
}

/// Where an in-app update has got to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePhase {
    Idle,
    Checking,
    Installing { version: String, step: Option<Step> },
    Installed { version: String },
}

enum UpdateMsg {
    Checked(Result<Latest, String>),
    Progress(Step),
    Done(Result<(), String>),
}

pub(crate) struct SelfUpdate {
    ops: UpdateOps,
    pub(crate) phase: UpdatePhase,
    rx: Option<Receiver<UpdateMsg>>,
    pub(crate) offer: Option<UpdateOffer>,
    /// `(version, error)` of the last failed install; cleared when the next
    /// dialog shows it.
    failure: Option<(String, String)>,
}

impl Default for SelfUpdate {
    fn default() -> Self {
        Self {
            ops: UpdateOps::default(),
            phase: UpdatePhase::Idle,
            rx: None,
            offer: None,
            failure: None,
        }
    }
}

impl App {
    /// Replace the update work with fakes. Tests only.
    pub fn set_update_ops(&mut self, ops: UpdateOps) {
        self.self_update.ops = ops;
    }

    pub fn update_phase(&self) -> &UpdatePhase {
        &self.self_update.phase
    }

    pub fn update_offer(&self) -> Option<&UpdateOffer> {
        self.self_update.offer.as_ref()
    }

    /// `U`: check for a newer release, then offer it in the update dialog.
    /// While a check or install runs, or once one is installed, says so
    /// instead.
    pub fn request_update(&mut self) {
        match &self.self_update.phase {
            UpdatePhase::Checking => self.flash("already checking for updates…"),
            UpdatePhase::Installing { version, step } => {
                let step = step.map(|s| s.to_string()).unwrap_or("starting".into());
                self.flash(format!("already updating to {version}: {step}"));
            }
            UpdatePhase::Installed { version } => {
                self.flash(format!("{version} is installed, restart tuxedo to use it"));
            }
            UpdatePhase::Idle => {
                self.self_update.phase = UpdatePhase::Checking;
                let check = self.self_update.ops.check;
                let (tx, rx) = mpsc::channel();
                self.self_update.rx = Some(rx);
                thread::spawn(move || {
                    let _ = tx.send(UpdateMsg::Checked(check()));
                });
            }
        }
    }

    /// `y` in the update dialog: install the offered version in the
    /// background. Does nothing for a copy that doesn't self-update.
    pub fn accept_update(&mut self) {
        let Some(offer) = &self.self_update.offer else {
            return;
        };
        if offer.command.is_some() {
            return;
        }
        let version = offer.version.clone();
        self.dismiss_update();
        let ops = self.self_update.ops;
        let Some(exe) = (ops.exe)() else {
            self.flash("can't find tuxedo's executable");
            return;
        };
        self.self_update.phase = UpdatePhase::Installing {
            version: version.clone(),
            step: None,
        };
        let (tx, rx) = mpsc::channel();
        self.self_update.rx = Some(rx);
        thread::spawn(move || {
            let progress = tx.clone();
            let result = (ops.install)(&version, &exe, &mut |s| {
                let _ = progress.send(UpdateMsg::Progress(s));
            });
            let _ = tx.send(UpdateMsg::Done(result));
        });
    }

    /// Close the update dialog.
    pub fn dismiss_update(&mut self) {
        self.self_update.offer = None;
        if self.mode == Mode::Update {
            self.mode = Mode::Normal;
        }
    }

    /// Drain the in-app update's messages. Returns `true` when anything
    /// changed, so the caller redraws.
    pub fn poll_self_update(&mut self) -> bool {
        let mut changed = false;
        loop {
            let Some(rx) = &self.self_update.rx else {
                return changed;
            };
            let msg = match rx.try_recv() {
                Ok(msg) => msg,
                Err(TryRecvError::Empty) => return changed,
                Err(TryRecvError::Disconnected) => {
                    self.self_update.rx = None;
                    if !matches!(self.self_update.phase, UpdatePhase::Installed { .. }) {
                        self.self_update.phase = UpdatePhase::Idle;
                    }
                    return true;
                }
            };
            changed = true;
            match msg {
                UpdateMsg::Checked(result) => self.on_update_checked(result),
                UpdateMsg::Progress(step) => {
                    if let UpdatePhase::Installing { step: s, .. } = &mut self.self_update.phase {
                        *s = Some(step);
                    }
                }
                UpdateMsg::Done(result) => self.on_update_done(result),
            }
        }
    }

    fn on_update_checked(&mut self, result: Result<Latest, String>) {
        self.self_update.phase = UpdatePhase::Idle;
        self.self_update.rx = None;
        let latest = match result {
            Ok(l) => l,
            Err(e) => {
                self.flash(e);
                return;
            }
        };
        self.latest_version = Some(latest.version.clone());
        let current = env!("CARGO_PKG_VERSION");
        if !update::is_newer(&latest.version, current) {
            self.flash(format!("tuxedo {current} is the latest"));
            return;
        }
        // Don't yank the user out of an edit or another dialog; the status
        // bar's `↑ <version> (U)` hint still points at it.
        if !matches!(self.mode, Mode::Normal | Mode::Visual) {
            return;
        }
        let exe = (self.self_update.ops.exe)();
        let failure = self
            .self_update
            .failure
            .take()
            .map(|(v, e)| format!("updating to {v} failed: {e}"));
        self.self_update.offer = Some(UpdateOffer {
            version: latest.version,
            command: InstallKind::of(exe.as_deref()).upgrade_command(),
            failure,
        });
        self.mode = Mode::Update;
    }

    fn on_update_done(&mut self, result: Result<(), String>) {
        self.self_update.rx = None;
        let version = match &self.self_update.phase {
            UpdatePhase::Installing { version, .. } => version.clone(),
            _ => return,
        };
        match result {
            Ok(()) => self.self_update.phase = UpdatePhase::Installed { version },
            Err(e) => {
                self.self_update.phase = UpdatePhase::Idle;
                self.flash(format!("update failed: {e}"));
                self.self_update.failure = Some((version, e));
            }
        }
    }

    /// The status bar's word on updates, and whether it reports a failure:
    /// one checking, downloading or installed, one failed, or a newer
    /// release out.
    pub fn update_note(&self) -> Option<(String, bool)> {
        let note = match &self.self_update.phase {
            UpdatePhase::Checking => "checking for updates…".to_string(),
            UpdatePhase::Installing { version, step } => {
                let step = step.map(|s| s.to_string()).unwrap_or("starting".into());
                let step = step.strip_prefix("downloading ").unwrap_or(&step);
                format!("updating {version} {step}")
            }
            UpdatePhase::Installed { version } => format!("{version} installed, restart tuxedo"),
            UpdatePhase::Idle if self.self_update.failure.is_some() => {
                return Some(("update failed (U)".to_string(), true));
            }
            UpdatePhase::Idle => format!("↑ {} (U)", self.update_available()?),
        };
        Some((note, false))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;
    use std::time::{Duration, Instant};

    const NEWER: &str = "9999.1.1";

    fn latest_newer() -> Result<Latest, String> {
        Ok(Latest {
            version: NEWER.into(),
            age_secs: None,
        })
    }

    fn latest_same() -> Result<Latest, String> {
        Ok(Latest {
            version: env!("CARGO_PKG_VERSION").into(),
            age_secs: None,
        })
    }

    fn check_fails() -> Result<Latest, String> {
        Err("couldn't check for updates, try again in 1m40s".into())
    }

    fn install_ok(_: &str, _: &Path, report: &mut dyn FnMut(Step)) -> Result<(), String> {
        report(Step::Downloading {
            done: 1,
            total: Some(2),
        });
        report(Step::Verifying);
        Ok(())
    }

    fn install_fails(_: &str, _: &Path, _: &mut dyn FnMut(Step)) -> Result<(), String> {
        Err("checksum mismatch".into())
    }

    fn binary_exe() -> Option<PathBuf> {
        Some(PathBuf::from("/opt/tuxedo/tuxedo"))
    }

    fn cargo_exe() -> Option<PathBuf> {
        Some(PathBuf::from("/home/m/.cargo/bin/tuxedo"))
    }

    fn app(ops: UpdateOps) -> App {
        let mut app = build_app("a task\n");
        app.set_update_ops(ops);
        app
    }

    fn ops(check: fn() -> Result<Latest, String>) -> UpdateOps {
        UpdateOps {
            check,
            install: install_ok,
            exe: binary_exe,
        }
    }

    /// Poll until `done` holds, for the background thread to report.
    fn settle(app: &mut App, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out");
            app.poll_self_update();
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn newer_release_opens_dialog_that_installs() {
        let mut app = app(ops(latest_newer));
        app.request_update();
        assert_eq!(app.update_phase(), &UpdatePhase::Checking);
        assert_eq!(app.update_note().unwrap().0, "checking for updates…");
        settle(&mut app, |a| a.mode == Mode::Update);
        let offer = app.update_offer().unwrap();
        assert_eq!(offer.version, NEWER);
        assert_eq!(offer.command, None);

        app.accept_update();
        assert_eq!(app.mode, Mode::Normal);
        settle(&mut app, |a| {
            matches!(a.update_phase(), UpdatePhase::Installed { .. })
        });
        assert_eq!(
            app.update_note().unwrap().0,
            format!("{NEWER} installed, restart tuxedo")
        );
        app.request_update();
        assert!(app.flash_active().unwrap().contains("restart tuxedo"));
    }

    #[test]
    fn latest_already_running_only_flashes() {
        let mut app = app(ops(latest_same));
        app.request_update();
        settle(&mut app, |a| a.update_phase() == &UpdatePhase::Idle);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.flash_active().unwrap().ends_with("is the latest"));
    }

    #[test]
    fn failed_check_flashes_its_reason() {
        let mut app = app(ops(check_fails));
        app.request_update();
        settle(&mut app, |a| a.update_phase() == &UpdatePhase::Idle);
        assert!(app.flash_active().unwrap().contains("try again in 1m40s"));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn managed_copy_gets_command_and_y_does_nothing() {
        let mut app = app(UpdateOps {
            exe: cargo_exe,
            ..ops(latest_newer)
        });
        app.request_update();
        settle(&mut app, |a| a.mode == Mode::Update);
        assert!(
            app.update_offer()
                .unwrap()
                .command
                .as_deref()
                .unwrap()
                .starts_with("cargo install")
        );
        app.accept_update();
        assert_eq!(app.mode, Mode::Update);
        assert_eq!(app.update_phase(), &UpdatePhase::Idle);
    }

    #[test]
    fn failed_install_is_shown_atop_next_dialog() {
        let mut app = app(UpdateOps {
            install: install_fails,
            ..ops(latest_newer)
        });
        app.request_update();
        settle(&mut app, |a| a.mode == Mode::Update);
        app.accept_update();
        settle(&mut app, |a| a.update_phase() == &UpdatePhase::Idle);
        assert_eq!(app.update_note(), Some(("update failed (U)".into(), true)));

        app.request_update();
        settle(&mut app, |a| a.mode == Mode::Update);
        assert_eq!(
            app.update_offer().unwrap().failure.as_deref(),
            Some("updating to 9999.1.1 failed: checksum mismatch")
        );
    }

    #[test]
    fn check_finishing_mid_edit_does_not_open_dialog() {
        let mut app = app(ops(latest_newer));
        app.request_update();
        app.mode = Mode::Insert;
        settle(&mut app, |a| a.update_phase() == &UpdatePhase::Idle);
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.update_note().unwrap().0, format!("↑ {NEWER} (U)"));
    }

    #[test]
    fn second_u_while_checking_only_reports() {
        let mut app = app(ops(latest_newer));
        app.request_update();
        app.request_update();
        assert_eq!(app.flash_active(), Some("already checking for updates…"));
    }
}
