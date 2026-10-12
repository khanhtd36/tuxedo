//! Self-update: download a fork release for this platform, verify it against
//! its `.sha256`, and swap it in for the running executable.
//!
//! Downloads shell out to `curl`, like the update check. On Windows a running
//! executable can't be overwritten but can be renamed, so the old one moves
//! aside to `<exe>.old` first; [`cleanup`] removes it at a later start.

use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::REPO_URL;

const USER_AGENT: &str = concat!("tuxedo/", env!("CARGO_PKG_VERSION"));
/// How often a download reports its progress.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Where an install has got to; [`install`] reports each one as it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Downloading { done: u64, total: Option<u64> },
    Verifying,
    Installing,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Downloading {
                done,
                total: Some(total),
            } => write!(
                f,
                "downloading {}/{} MB",
                megabytes(*done),
                megabytes(*total)
            ),
            Step::Downloading { done, total: None } => {
                write!(f, "downloading {} MB", megabytes(*done))
            }
            Step::Verifying => f.write_str("verifying"),
            Step::Installing => f.write_str("installing"),
        }
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

/// The GitHub release page for fork `version`.
pub fn release_url(version: &str) -> String {
    format!("{REPO_URL}/releases/tag/fork-v{version}")
}

/// Install fork `version` in place of `exe`, reporting progress through
/// `report`. Errors are user-facing sentences.
pub fn install(version: &str, exe: &Path, report: &mut dyn FnMut(Step)) -> Result<(), String> {
    let asset = asset_name(version).ok_or("no release build for this platform")?;
    let url = format!("{REPO_URL}/releases/download/fork-v{version}/{asset}");
    let tmp = std::env::temp_dir().join(format!("tuxedo-update-{}", std::process::id()));
    fs::create_dir_all(&tmp).map_err(|e| format!("can't create {}: {e}", tmp.display()))?;
    let result = (|| {
        let archive_path = tmp.join(&asset);
        download(&url, &archive_path, report)?;
        let sums = fetch_text(&format!("{url}.sha256"))?;
        report(Step::Verifying);
        let archive = fs::read(&archive_path).map_err(|e| format!("can't read download: {e}"))?;
        verify(&archive, &sums)?;
        report(Step::Installing);
        let bin = extract(&archive, asset.ends_with(".zip"))?;
        replace(exe, &bin).map_err(|e| format!("can't replace {}: {e}", exe.display()))
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

/// Remove the copies a previous update moved aside (`<exe>.old` and
/// `<exe>.old-<timestamp>`). One still running stays locked and is skipped.
pub fn cleanup(exe: &Path) {
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else {
        return;
    };
    let old = format!("{}.old", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        if file == old || file.starts_with(&format!("{old}-")) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Release asset for `version` on this platform, as named by
/// `fork-release.yml`. `None` on a platform the fork doesn't build.
fn asset_name(version: &str) -> Option<String> {
    let (target, ext) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => ("x86_64-pc-windows-msvc", "zip"),
        ("linux", "x86_64") => ("x86_64-unknown-linux-gnu", "tar.gz"),
        ("linux", "aarch64") => ("aarch64-unknown-linux-gnu", "tar.gz"),
        ("macos", "x86_64") => ("x86_64-apple-darwin", "tar.gz"),
        ("macos", "aarch64") => ("aarch64-apple-darwin", "tar.gz"),
        _ => return None,
    };
    Some(format!("tuxedo-fork-v{version}-{target}.{ext}"))
}

/// Download `url` to `dest`, reporting the bytes written so far. curl gives
/// up on a connection that stalls below 1 KB/s for 30 seconds.
fn download(url: &str, dest: &Path, report: &mut dyn FnMut(Step)) -> Result<(), String> {
    let total = content_length(url);
    let mut child = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "10",
            "--speed-limit",
            "1024",
            "--speed-time",
            "30",
            "-A",
            USER_AGENT,
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't run curl: {e}"))?;
    loop {
        let done = fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        report(Step::Downloading { done, total });
        let status = child
            .try_wait()
            .map_err(|e| format!("download failed: {e}"))?;
        if let Some(status) = status {
            if status.success() {
                return Ok(());
            }
            let mut err = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_string(&mut err);
            }
            let err = err.trim().trim_start_matches("curl: ");
            return Err(if err.is_empty() {
                format!("download failed ({status})")
            } else {
                format!("download failed: {err}")
            });
        }
        thread::sleep(PROGRESS_EVERY);
    }
}

/// Size of `url` from a HEAD request, following redirects. `None` when the
/// request fails or the server doesn't say; progress then shows no total.
fn content_length(url: &str) -> Option<u64> {
    let out = Command::new("curl")
        .args(["-fsSIL", "-m", "10", "-A", USER_AGENT, url])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Each redirect hop prints its own headers; the last length is the file's.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .next_back()
}

fn fetch_text(url: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "-m", "30", "-A", USER_AGENT, url])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("can't download {url}"));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("{url} isn't text"))
}

/// Check `archive` against a `.sha256` file (`<hex>  <name>`).
fn verify(archive: &[u8], sums: &str) -> Result<(), String> {
    let expected = sums
        .split_whitespace()
        .next()
        .ok_or("the checksum file is empty")?
        .to_ascii_lowercase();
    let actual: String = Sha256::digest(archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        ))
    }
}

fn binary_name() -> String {
    format!("tuxedo{}", std::env::consts::EXE_SUFFIX)
}

/// The tuxedo executable inside a release archive, wherever it sits: the
/// zip is flat, the tarballs hold it one folder down.
fn extract(archive: &[u8], is_zip: bool) -> Result<Vec<u8>, String> {
    let name = binary_name();
    let missing = || format!("the release archive has no {name}");
    let corrupt = |e: &dyn fmt::Display| format!("can't read the release archive: {e}");
    let mut bin = Vec::new();
    if is_zip {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| corrupt(&e))?;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(|e| corrupt(&e))?;
            let path = file.name().map_err(|e| corrupt(&e))?;
            if path.rsplit(['/', '\\']).next() == Some(name.as_str()) {
                file.read_to_end(&mut bin).map_err(|e| corrupt(&e))?;
                return Ok(bin);
            }
        }
        return Err(missing());
    }
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().map_err(|e| corrupt(&e))? {
        let mut entry = entry.map_err(|e| corrupt(&e))?;
        let path = entry.path().map_err(|e| corrupt(&e))?;
        if path.file_name().is_some_and(|f| f == name.as_str()) {
            entry.read_to_end(&mut bin).map_err(|e| corrupt(&e))?;
            return Ok(bin);
        }
    }
    Err(missing())
}

/// Write `bin` next to `exe`, then swap it in.
fn replace(exe: &Path, bin: &[u8]) -> io::Result<()> {
    replace_with(exe, bin, cfg!(windows))
}

/// `rename_aside` moves the current `exe` to `<exe>.old` before the new one
/// takes its name, which Windows needs for a running executable. If `.old`
/// itself is locked (a copy from an earlier update still running) the old
/// one gets a timestamped name instead. Unix replaces a running file's name
/// freely, so it skips that step.
fn replace_with(exe: &Path, bin: &[u8], rename_aside: bool) -> io::Result<()> {
    let next = sibling(exe, ".new");
    fs::write(&next, bin)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&next, fs::Permissions::from_mode(0o755))?;
    }
    if !rename_aside {
        return fs::rename(&next, exe).inspect_err(|_| {
            let _ = fs::remove_file(&next);
        });
    }
    let mut old = sibling(exe, ".old");
    let _ = fs::remove_file(&old);
    if old.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
        old = sibling(exe, &format!(".old-{stamp}"));
    }
    if let Err(e) = fs::rename(exe, &old) {
        let _ = fs::remove_file(&next);
        return Err(e);
    }
    if let Err(e) = fs::rename(&next, exe) {
        let _ = fs::rename(&old, exe); // put the running one back
        return Err(e);
    }
    Ok(())
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tuxedo-install-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha256_hex(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn verify_accepts_matching_checksum() {
        let data = b"tuxedo";
        let sums = format!("{}  tuxedo.zip\n", sha256_hex(data).to_uppercase());
        assert!(verify(data, &sums).is_ok());
    }

    #[test]
    fn verify_refuses_mismatched_checksum() {
        let sums = format!("{}  tuxedo.zip\n", sha256_hex(b"other"));
        let err = verify(b"tuxedo", &sums).unwrap_err();
        assert!(err.starts_with("checksum mismatch"), "{err}");
    }

    #[test]
    fn verify_refuses_empty_checksum_file() {
        assert!(verify(b"tuxedo", "  \n").is_err());
    }

    #[test]
    fn extract_finds_binary_at_zip_root() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("README.md", opts).unwrap();
            zip.write_all(b"readme").unwrap();
            zip.start_file(binary_name(), opts).unwrap();
            zip.write_all(b"new binary").unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(extract(buf.get_ref(), true).unwrap(), b"new binary");
    }

    #[test]
    fn extract_finds_binary_inside_tarball_folder() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut tar = tar::Builder::new(&mut gz);
            for (path, body) in [
                ("tuxedo-fork-v1-x/README.md", &b"readme"[..]),
                (
                    &format!("tuxedo-fork-v1-x/{}", binary_name())[..],
                    b"new binary",
                ),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append_data(&mut header, path, body).unwrap();
            }
            tar.finish().unwrap();
        }
        let archive = gz.finish().unwrap();
        assert_eq!(extract(&archive, false).unwrap(), b"new binary");
    }

    #[test]
    fn extract_errors_without_binary() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("README.md", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.finish().unwrap();
        }
        assert!(extract(buf.get_ref(), true).is_err());
    }

    #[test]
    fn replace_swaps_in_place() {
        let dir = temp_dir("plain");
        let exe = dir.join("tuxedo");
        fs::write(&exe, "old").unwrap();
        replace_with(&exe, b"new", false).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(!sibling(&exe, ".new").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_renames_running_copy_aside() {
        let dir = temp_dir("aside");
        let exe = dir.join("tuxedo");
        fs::write(&exe, "old").unwrap();
        fs::write(sibling(&exe, ".old"), "older").unwrap();
        replace_with(&exe, b"new", true).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(sibling(&exe, ".old")).unwrap(), b"old");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_uses_timestamped_name_when_old_is_locked() {
        let dir = temp_dir("locked");
        let exe = dir.join("tuxedo");
        fs::write(&exe, "old").unwrap();
        // A non-empty folder can't be removed as a file: stands in for an
        // `.old` that an earlier, still-running copy holds open.
        let old = sibling(&exe, ".old");
        fs::create_dir_all(old.join("x")).unwrap();
        replace_with(&exe, b"new", true).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        let stamped: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("tuxedo.old-"))
            .collect();
        assert_eq!(stamped.len(), 1, "{stamped:?}");
        assert_eq!(fs::read(dir.join(&stamped[0])).unwrap(), b"old");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_fails_cleanly_when_exe_is_missing() {
        let dir = temp_dir("missing");
        let exe = dir.join("tuxedo");
        assert!(replace_with(&exe, b"new", true).is_err());
        assert!(!sibling(&exe, ".new").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_old_copies_only() {
        let dir = temp_dir("cleanup");
        let exe = dir.join("tuxedo.exe");
        for name in [
            "tuxedo.exe",
            "tuxedo.exe.old",
            "tuxedo.exe.old-20261012120000",
            "tuxedo.exe.older",
            "other.exe.old",
        ] {
            fs::write(dir.join(name), "x").unwrap();
        }
        cleanup(&exe);
        let mut left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["other.exe.old", "tuxedo.exe", "tuxedo.exe.older"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn asset_name_matches_release_naming() {
        let name = asset_name("2026.8.1-khanhtd36.3");
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert_eq!(
                name.as_deref(),
                Some("tuxedo-fork-v2026.8.1-khanhtd36.3-x86_64-pc-windows-msvc.zip")
            );
        }
        if let Some(name) = name {
            assert!(name.starts_with("tuxedo-fork-v2026.8.1-khanhtd36.3-"));
        }
    }

    /// Downloads a real fork release; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "network: downloads a fork release from GitHub"]
    fn installs_published_release() {
        let dir = temp_dir("network");
        let exe = dir.join(binary_name());
        fs::write(&exe, "old").unwrap();
        let mut steps = Vec::new();
        install("2026.8.1-khanhtd36.2", &exe, &mut |s| steps.push(s)).unwrap();
        assert!(fs::metadata(&exe).unwrap().len() > 100_000);
        assert!(steps.contains(&Step::Verifying));
        assert!(matches!(
            steps.first(),
            Some(Step::Downloading { total: Some(_), .. })
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_reads_as_progress() {
        let s = Step::Downloading {
            done: 3_100_000,
            total: Some(4_000_000),
        };
        assert_eq!(s.to_string(), "downloading 3.1/4.0 MB");
        let s = Step::Downloading {
            done: 500_000,
            total: None,
        };
        assert_eq!(s.to_string(), "downloading 0.5 MB");
        assert_eq!(Step::Verifying.to_string(), "verifying");
    }
}
