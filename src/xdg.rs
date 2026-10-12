//! XDG Base Directory resolution shared between `config` and `theme` loaders.

use std::path::PathBuf;

/// Resolve the XDG base config directory. Per the XDG Base Directory Spec,
/// `XDG_CONFIG_HOME` MUST be an absolute path; relative values are ignored.
/// We warn once so users debugging path resolution can see why their relative
/// override didn't take effect.
pub fn config_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_CONFIG_HOME")
        && !v.is_empty()
    {
        let p = PathBuf::from(&v);
        if p.is_absolute() {
            return Some(p);
        }
        eprintln!(
            "tuxedo: ignoring non-absolute XDG_CONFIG_HOME={:?} (per XDG spec)",
            p.display()
        );
    }
    Some(home_dir()?.join(".config"))
}

/// Resolve the user's home directory: `$HOME`, else `%USERPROFILE%`.
/// Native Windows shells (PowerShell, cmd) don't set `HOME`; without the
/// fallback the config, themes and keybinds would never load there.
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty())
        .map(PathBuf::from)
}
