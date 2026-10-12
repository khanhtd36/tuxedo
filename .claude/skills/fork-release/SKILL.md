---
name: fork-release
description: Cut a new khanhtd36/tuxedo fork release -- bump version, regenerate snapshots, tag, push, watch CI build+publish+winget+homebrew. Use when the user asks to release a new version, cut a release, or bump the fork version.
---

# Fork release

Releasing `khanhtd36/tuxedo` (the fork, not upstream `webstonehq/tuxedo`) is a short local sequence; `.github/workflows/fork-release.yml` takes over from the pushed tag. Never push a `v*` tag or run upstream's `release.yml`: it publishes to `webstonehq/homebrew-tap`.

## Steps

1. **Sync main.**
   ```
   git checkout main
   GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" git pull origin main
   ```
   Working tree must be clean before continuing.

2. **Bump the version** in `Cargo.toml` (`version = "X.Y.Z-khanhtd36.N"`). Bump only `N` for a fork-only release. Reset `N` to `1` when `X.Y.Z` changed, i.e. this release also picked up a new upstream sync.

3. **Refresh the lockfile and snapshots, then commit.** The status bar prints the version, so every full-frame snapshot changes with it.
   ```
   cargo update -p tuxedo --offline
   INSTA_UPDATE=always cargo test --test snapshots --locked
   cargo test --locked
   git add Cargo.toml Cargo.lock tests/snapshots
   git commit -m "chore: bump version to X.Y.Z-khanhtd36.N"
   ```
   On Windows `ui::title::tests::keeps_absolute_path_when_home_is_unknown` fails on upstream too; ignore it. Anything else failing blocks the release.

4. **Push main, tag, push the tag.**
   ```
   GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" git push origin main
   git tag fork-vX.Y.Z-khanhtd36.N
   GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" git push origin fork-vX.Y.Z-khanhtd36.N
   ```
   Push the one tag by name. Never `git push --tags`: the clone holds upstream's `v*` tags too.

5. **Watch the run.**
   ```
   GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" gh run list --repo khanhtd36/tuxedo --workflow fork-release.yml --limit 1
   GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" gh run watch <run-id> --repo khanhtd36/tuxedo --exit-status
   ```
   Done when `verify-version`, all five `build` jobs, `release`, `winget` and `homebrew` succeed. `winget` is `continue-on-error`, so also open its log and confirm it submitted a PR.

## Secrets

- `WINGET_TOKEN`: classic PAT with `public_repo`, used by `wingetcreate` to open the winget-pkgs PR.
- `HOMEBREW_TAP_DEPLOY_KEY`: private half of a write deploy key on `khanhtd36/homebrew-tap`.

## Gotchas

- **gh account**: the default `gh` account has no access to this fork. Wrap every `git push` / `gh` call with `GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36"`.
- **winget lag**: the winget-pkgs PR takes hours to merge and index, so `winget upgrade` lags the release.
