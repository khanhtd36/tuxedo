//! Update check and self-update.
//!
//! Three pieces:
//!
//! 1. [`run`] — handler for `tuxedo update [--check]`. Finds the latest fork
//!    release and, unless Homebrew or Cargo manage this copy, self-updates to
//!    it (see [`install`]). For a managed copy it prints the command to run.
//!
//! 2. [`spawn_check`] — background thread invoked at TUI startup that consults
//!    a cache under `$XDG_CACHE_HOME/tuxedo/latest_version.json`. If the cache
//!    is missing or older than 24h, it shells out to `curl` to read the
//!    `tag_name` of the latest GitHub release, rewrites the cache, and returns
//!    the tag through an mpsc channel. The TUI's status bar reads it and
//!    appends an `↑ <version>` hint when newer than the running build. All
//!    failures are silent — a stale or missing cache simply means no hint.
//!
//! 3. [`check_now`] — the check behind `U` and `tuxedo update`. Asks GitHub
//!    now rather than trusting the day-old cache, but at most once per
//!    [`MANUAL_COOLDOWN`] so a held-down `U` can't hammer the API.

pub mod install;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use install::{Step, cleanup, release_url};

/// How tuxedo appears to have been installed, judged by the path of the
/// currently-running executable. Decides whether it self-updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Homebrew,
    Cargo,
    Binary,
    Unknown,
}

impl InstallKind {
    /// How tuxedo installed as `exe` (if known) is installed.
    pub fn of(exe: Option<&Path>) -> Self {
        exe.map(detect_kind).unwrap_or(InstallKind::Unknown)
    }

    /// What to run instead of a self-update, for a copy tuxedo doesn't
    /// replace itself. `None` for a [`InstallKind::Binary`], which does.
    pub fn upgrade_command(self) -> Option<String> {
        match self {
            InstallKind::Homebrew => Some(format!("brew update && brew upgrade {BREW_FORMULA}")),
            InstallKind::Cargo => Some(format!("cargo install --git {REPO_URL} --force")),
            InstallKind::Unknown => Some(format!("download {REPO_URL}/releases/latest")),
            InstallKind::Binary => None,
        }
    }
}

/// The latest fork version and, when it came from the cache inside the
/// cooldown, how many seconds ago it was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    pub age_secs: Option<u64>,
}

/// Run `tuxedo update [--check]`. Returns the process exit code.
pub fn run(args: &[String]) -> io::Result<i32> {
    let check_only = match args {
        [] => false,
        [a] if a == "--check" => true,
        _ => {
            eprintln!("usage: tuxedo update [--check]");
            return Ok(2);
        }
    };
    let current = env!("CARGO_PKG_VERSION");
    let latest = match check_now() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tuxedo: {e}");
            return Ok(1);
        }
    };
    let checked = latest
        .age_secs
        .map(|s| format!(" (checked {} ago)", format_wait(s)))
        .unwrap_or_default();
    let version = latest.version;
    if !is_newer(&version, current) {
        println!("tuxedo {current} is the latest{checked}.");
        return Ok(0);
    }
    println!("tuxedo {current} → {version}{checked}");
    println!("{}", release_url(&version));
    if check_only {
        return Ok(0);
    }
    let exe = std::env::current_exe().ok();
    let kind = InstallKind::of(exe.as_deref());
    let (Some(exe), None) = (exe, kind.upgrade_command()) else {
        println!();
        println!("This copy doesn't update itself. Instead:");
        println!();
        println!("    {}", kind.upgrade_command().unwrap_or_default());
        return Ok(0);
    };
    // One line, rewritten in place as the download goes. Padded rather than
    // erased with an escape code, which older Windows consoles print raw.
    let mut width = 0;
    let result = install::install(&version, &exe, &mut |step| {
        let line = step.to_string();
        width = width.max(line.len());
        print!("\r{line:<width$}");
        let _ = io::stdout().flush();
    });
    if width > 0 {
        print!("\r{:width$}\r", "");
    }
    match result {
        Ok(()) => {
            println!("Updated to {version}.");
            Ok(0)
        }
        Err(e) => {
            eprintln!("tuxedo: update failed: {e}");
            Ok(1)
        }
    }
}

/// Classify an executable path into an [`InstallKind`]. Exposed for tests.
pub fn detect_kind(exe: &Path) -> InstallKind {
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/")
        || s.starts_with("/opt/homebrew/")
        || s.starts_with("/usr/local/Homebrew/")
        || s.contains("/homebrew/Cellar/")
        || s.contains("/linuxbrew/")
    {
        return InstallKind::Homebrew;
    }
    if s.contains("/.cargo/bin/") || s.contains("\\.cargo\\bin\\") {
        return InstallKind::Cargo;
    }
    // A bare /usr/local/bin/tuxedo could be either a Homebrew shim (older
    // macOS) or a manual download. Without more signal, treat it as a binary.
    if !s.is_empty() {
        return InstallKind::Binary;
    }
    InstallKind::Unknown
}

/// Spawn the background update check. Returns a receiver that yields exactly
/// one message — `Some(tag)` if a cached or freshly-fetched tag is available,
/// otherwise `None`. The receiver is dropped when the thread exits, so a
/// disconnect on `try_recv` means "give up, nothing's coming".
pub fn spawn_check() -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
    thread::spawn(move || {
        let result = check_for_update();
        let _ = tx.send(result);
    });
    rx
}

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long to honor a cached *failure* (empty `tag`) before trying the
/// network again. Short enough to recover within an hour, long enough to
/// stop hammering GitHub once anonymous API calls have been rate-limited
/// (which is what burned us once during testing).
const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// Shortest gap between two GitHub calls from [`check_now`]; inside it, the
/// cached answer (success or failure) stands.
pub const MANUAL_COOLDOWN: Duration = Duration::from_secs(2 * 60);
const CURL_TIMEOUT_SECS: u64 = 5;
const RELEASE_URL: &str = "https://api.github.com/repos/khanhtd36/tuxedo/releases/latest";

// This is khanhtd36's fork: updates come from its releases, never upstream's,
// so neither a self-update nor an upgrade hint can replace the fork with
// upstream tuxedo.
const REPO_URL: &str = "https://github.com/khanhtd36/tuxedo";
const BREW_FORMULA: &str = "khanhtd36/tap/tuxedo-khanhtd36";

/// Fork release tags carry a `fork-v` prefix (`fork-v2026.8.1-khanhtd36.1`)
/// so they never collide with upstream's `v*` tags. Drop it, and any bare
/// `v`, so the tag is a plain version: [`install`] rebuilds the tag and asset
/// names from it, and it reads like a version in the status-bar hint.
fn version_of(tag: &str) -> &str {
    let tag = tag.strip_prefix("fork-").unwrap_or(tag);
    tag.strip_prefix('v').unwrap_or(tag)
}

fn check_for_update() -> Option<String> {
    let cache_path = cache_path();
    let now = now_epoch();
    // Honor the cache for both success and failure. A non-empty tag within
    // CACHE_TTL is a success entry; an empty tag within NEGATIVE_CACHE_TTL
    // is a "we just tried and it failed, don't pummel GitHub again" marker.
    if let Some(p) = &cache_path
        && let Some((ts, tag)) = read_cache(p)
    {
        let age = now.saturating_sub(ts);
        if !tag.is_empty() && age < CACHE_TTL.as_secs() {
            return Some(tag);
        }
        if tag.is_empty() && age < NEGATIVE_CACHE_TTL.as_secs() {
            return None;
        }
    }
    // Cache is stale, missing, or expired-negative — try the network. Cache
    // either outcome so we don't retry on every launch when offline or
    // rate-limited.
    let tag = fetch_latest_body()
        .and_then(|b| parse_tag_from_release_json(&b))
        .map(|t| version_of(&t).to_string());
    if let Some(p) = &cache_path {
        let _ = write_cache(p, now, tag.as_deref().unwrap_or(""));
    }
    tag
}

/// Find the latest fork version for `U` or `tuxedo update`: from GitHub,
/// unless the cache was written within [`MANUAL_COOLDOWN`]. Shares the cache
/// with the background check, so the cooldown holds across restarts and
/// between tuxedo windows. Errors are user-facing sentences.
pub fn check_now() -> Result<Latest, String> {
    let cache_path = cache_path();
    let now = now_epoch();
    let cached = cache_path.as_deref().and_then(read_cache);
    match decide_manual(cached, now) {
        Manual::Cached { tag, age_secs } => Ok(Latest {
            version: tag,
            age_secs: Some(age_secs),
        }),
        Manual::CoolingDown { retry_in_secs } => Err(format!(
            "couldn't check for updates, try again in {}",
            format_wait(retry_in_secs)
        )),
        Manual::Fetch => {
            let tag = fetch_latest_body()
                .and_then(|b| parse_tag_from_release_json(&b))
                .map(|t| version_of(&t).to_string());
            if let Some(p) = &cache_path {
                let _ = write_cache(p, now, tag.as_deref().unwrap_or(""));
            }
            tag.map(|version| Latest {
                version,
                age_secs: None,
            })
            .ok_or_else(|| "couldn't check for updates".to_string())
        }
    }
}

/// What [`check_now`] does given the cache entry `(checked_at, tag)`.
#[derive(Debug, PartialEq, Eq)]
enum Manual {
    Cached { tag: String, age_secs: u64 },
    CoolingDown { retry_in_secs: u64 },
    Fetch,
}

fn decide_manual(cached: Option<(u64, String)>, now: u64) -> Manual {
    let cooldown = MANUAL_COOLDOWN.as_secs();
    match cached {
        // A cache stamped in the future (clock moved back) counts as stale.
        Some((ts, tag)) if ts <= now && now - ts < cooldown => {
            let age_secs = now - ts;
            if tag.is_empty() {
                Manual::CoolingDown {
                    retry_in_secs: cooldown - age_secs,
                }
            } else {
                Manual::Cached { tag, age_secs }
            }
        }
        _ => Manual::Fetch,
    }
}

/// `100` → `1m40s`, `40` → `40s`.
pub fn format_wait(secs: u64) -> String {
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s}s"),
        (m, 0) => format!("{m}m"),
        (m, s) => format!("{m}m{s}s"),
    }
}

fn fetch_latest_body() -> Option<String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "-m",
            &CURL_TIMEOUT_SECS.to_string(),
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            concat!("tuxedo/", env!("CARGO_PKG_VERSION")),
            RELEASE_URL,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Pull the `tag_name` value out of a GitHub release JSON payload. Doesn't
/// pull in a JSON parser — release payloads are well-formed enough that a
/// targeted string scan suffices, and a malformed payload simply returns
/// `None`. Exposed for unit testing.
pub fn parse_tag_from_release_json(body: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = &rest[colon + 1..];
    let q = after_colon.find('"')?;
    let after_q = &after_colon[q + 1..];
    let end = after_q.find('"')?;
    let tag = &after_q[..end];
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

/// True when `latest` is a strictly newer version than `current`, comparing
/// each `.`- or `-`-separated segment numerically. A leading `fork-v` or `v`
/// (e.g. `v2026.5.5`) is stripped on both sides. Non-numeric segments fall
/// back to lexicographic comparison of that segment, so the fork suffix in
/// `2026.8.1-khanhtd36.2` compares its build number as a number.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let l = version_of(latest);
    let c = version_of(current);
    let mut li = l.split(['.', '-']);
    let mut ci = c.split(['.', '-']);
    loop {
        match (li.next(), ci.next()) {
            (None, None) => return false,
            (Some(a), None) => return a.parse::<u64>().is_ok_and(|n| n > 0) || !a.is_empty(),
            (None, Some(_)) => return false,
            (Some(a), Some(b)) => match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(x), Ok(y)) if x != y => return x > y,
                (Ok(_), Ok(_)) => continue,
                _ => match a.cmp(b) {
                    std::cmp::Ordering::Greater => return true,
                    std::cmp::Ordering::Less => return false,
                    std::cmp::Ordering::Equal => continue,
                },
            },
        }
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = xdg_cache_home()?;
    Some(base.join("tuxedo").join("latest_version.json"))
}

fn xdg_cache_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_CACHE_HOME")
        && !v.is_empty()
    {
        let p = PathBuf::from(&v);
        if p.is_absolute() {
            return Some(p);
        }
    }
    Some(crate::xdg::home_dir()?.join(".cache"))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read a cache file. Returns `(epoch_secs, tag)` on success. Format is a
/// tiny JSON-ish payload: `{"checked_at": <int>, "tag": "<str>"}`. An empty
/// `tag` is a sentinel for a cached failure (see [`NEGATIVE_CACHE_TTL`]);
/// callers distinguish success from failure on the returned string. Older
/// builds cached the tag with its `v`, so it's normalized on the way out.
fn read_cache(path: &Path) -> Option<(u64, String)> {
    let body = std::fs::read_to_string(path).ok()?;
    let ts = scan_int(&body, "\"checked_at\"")?;
    let tag = scan_str(&body, "\"tag\"").unwrap_or_default();
    Some((ts, version_of(&tag).to_string()))
}

fn write_cache(path: &Path, now: u64, tag: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!("{{\"checked_at\": {now}, \"tag\": \"{tag}\"}}\n");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn scan_int(body: &str, key: &str) -> Option<u64> {
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

fn scan_str(body: &str, key: &str) -> Option<String> {
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = &rest[colon + 1..];
    let q = after_colon.find('"')?;
    let after_q = &after_colon[q + 1..];
    let end = after_q.find('"')?;
    Some(after_q[..end].to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_kind_homebrew_paths() {
        assert_eq!(
            detect_kind(&PathBuf::from("/opt/homebrew/bin/tuxedo")),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from(
                "/opt/homebrew/Cellar/tuxedo/2026.5.3/bin/tuxedo"
            )),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from(
                "/usr/local/Cellar/tuxedo/2026.5.3/bin/tuxedo"
            )),
            InstallKind::Homebrew
        );
        assert_eq!(
            detect_kind(&PathBuf::from(
                "/home/linuxbrew/.linuxbrew/Cellar/tuxedo/2026.5.3/bin/tuxedo"
            )),
            InstallKind::Homebrew
        );
    }

    #[test]
    fn detect_kind_cargo_path() {
        assert_eq!(
            detect_kind(&PathBuf::from("/home/m/.cargo/bin/tuxedo")),
            InstallKind::Cargo
        );
    }

    #[test]
    fn detect_kind_falls_back_to_binary() {
        assert_eq!(
            detect_kind(&PathBuf::from("/usr/local/bin/tuxedo")),
            InstallKind::Binary
        );
        assert_eq!(
            detect_kind(&PathBuf::from("/tmp/tuxedo")),
            InstallKind::Binary
        );
    }

    #[test]
    fn detect_kind_winget_package_is_binary() {
        let exe = PathBuf::from(
            r"C:\Users\m\AppData\Local\Microsoft\WinGet\Packages\khanhtd36.tuxedo-khanhtd36__DefaultSource\tuxedo.exe",
        );
        assert_eq!(detect_kind(&exe), InstallKind::Binary);
        assert_eq!(InstallKind::Binary.upgrade_command(), None);
        assert!(InstallKind::of(None).upgrade_command().is_some());
    }

    #[test]
    fn decide_manual_fetches_without_fresh_cache() {
        assert_eq!(decide_manual(None, 1_000), Manual::Fetch);
        assert_eq!(
            decide_manual(Some((800, "1.0".into())), 1_000),
            Manual::Fetch
        );
        // Stamped in the future: the clock moved back, so don't trust it.
        assert_eq!(
            decide_manual(Some((2_000, "1.0".into())), 1_000),
            Manual::Fetch
        );
    }

    #[test]
    fn decide_manual_reuses_answer_inside_cooldown() {
        assert_eq!(
            decide_manual(Some((960, "1.0".into())), 1_000),
            Manual::Cached {
                tag: "1.0".into(),
                age_secs: 40
            }
        );
    }

    #[test]
    fn decide_manual_cools_down_after_failure() {
        assert_eq!(
            decide_manual(Some((980, String::new())), 1_000),
            Manual::CoolingDown { retry_in_secs: 100 }
        );
    }

    #[test]
    fn format_wait_reads_as_minutes_and_seconds() {
        assert_eq!(format_wait(40), "40s");
        assert_eq!(format_wait(100), "1m40s");
        assert_eq!(format_wait(120), "2m");
    }

    #[test]
    fn is_newer_handles_calver_segments() {
        // Same version
        assert!(!is_newer("2026.5.3", "2026.5.3"));
        // Patch bump
        assert!(is_newer("2026.5.4", "2026.5.3"));
        assert!(!is_newer("2026.5.3", "2026.5.4"));
        // Crucially: numeric (not lex) compare on patches >= 10
        assert!(is_newer("2026.5.10", "2026.5.9"));
        assert!(!is_newer("2026.5.9", "2026.5.10"));
        // Month rollover
        assert!(is_newer("2026.10.1", "2026.9.5"));
        // Year rollover
        assert!(is_newer("2027.1.1", "2026.12.31"));
    }

    #[test]
    fn is_newer_strips_v_prefix() {
        assert!(is_newer("v2026.5.4", "2026.5.3"));
        assert!(is_newer("2026.5.4", "v2026.5.3"));
        assert!(!is_newer("v2026.5.3", "v2026.5.3"));
    }

    #[test]
    fn is_newer_compares_fork_versions() {
        let current = "2026.8.1-khanhtd36.2";
        assert!(!is_newer("fork-v2026.8.1-khanhtd36.2", current));
        assert!(!is_newer("fork-v2026.8.1-khanhtd36.1", current));
        assert!(is_newer("fork-v2026.8.1-khanhtd36.10", current));
        assert!(is_newer("v2026.9.1-khanhtd36.1", current));
        // A stale cached upstream tag for the same base version isn't newer.
        assert!(!is_newer("v2026.8.1", current));
    }

    #[test]
    fn version_of_drops_tag_prefixes() {
        assert_eq!(
            version_of("fork-v2026.8.1-khanhtd36.4"),
            "2026.8.1-khanhtd36.4"
        );
        assert_eq!(version_of("v2026.8.1"), "2026.8.1");
        assert_eq!(version_of("2026.8.1"), "2026.8.1");
    }

    #[test]
    fn read_cache_normalizes_old_v_tag() {
        let dir = std::env::temp_dir().join(format!(
            "tuxedo-update-vtag-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_version.json");
        write_cache(&path, 1_700_000_000, "v2026.8.1-khanhtd36.4").unwrap();
        let (_, tag) = read_cache(&path).unwrap();
        assert_eq!(tag, "2026.8.1-khanhtd36.4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_newer_handles_segment_count_mismatch() {
        // "2026.5" vs "2026.5.0" — equal in spirit, but with a non-zero suffix
        // the longer one is newer.
        assert!(is_newer("2026.5.1", "2026.5"));
        assert!(!is_newer("2026.5", "2026.5.1"));
    }

    #[test]
    fn parse_tag_extracts_first_tag_name() {
        let body = r#"{"url":"x","tag_name":"v2026.5.5","name":"2026.5.5"}"#;
        assert_eq!(
            parse_tag_from_release_json(body).as_deref(),
            Some("v2026.5.5")
        );
    }

    #[test]
    fn parse_tag_with_whitespace_and_extra_keys() {
        let body = r#"
        {
          "url": "x",
          "tag_name" : "2026.5.10" ,
          "draft": false
        }
        "#;
        assert_eq!(
            parse_tag_from_release_json(body).as_deref(),
            Some("2026.5.10")
        );
    }

    #[test]
    fn parse_tag_returns_none_on_missing_field() {
        let body = r#"{"name":"hi"}"#;
        assert!(parse_tag_from_release_json(body).is_none());
    }

    #[test]
    fn cache_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "tuxedo-update-cache-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_version.json");
        write_cache(&path, 1_700_000_000, "2026.5.5").unwrap();
        let (ts, tag) = read_cache(&path).unwrap();
        assert_eq!(ts, 1_700_000_000);
        assert_eq!(tag, "2026.5.5");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_cache_none_on_missing_file() {
        assert!(read_cache(&PathBuf::from("/tmp/does-not-exist-xyzzy")).is_none());
    }

    #[test]
    fn cache_round_trip_empty_tag_is_negative_marker() {
        let dir = std::env::temp_dir().join(format!(
            "tuxedo-update-neg-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_version.json");
        // A failed check writes an empty tag.
        write_cache(&path, 1_700_000_000, "").unwrap();
        let (ts, tag) = read_cache(&path).unwrap();
        assert_eq!(ts, 1_700_000_000);
        assert_eq!(tag, "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
