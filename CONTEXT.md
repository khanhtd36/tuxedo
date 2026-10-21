# tuxedo

A keyboard-driven terminal UI for todo.txt files. This repository is `khanhtd36`'s fork of `webstonehq/tuxedo`.

## Language

**Task row**:
Everything one task occupies in the list or archive view: its first display line plus any continuation lines.
_Avoid_: row (alone, when display line is meant), line

**Display line**:
One terminal line of the list or archive view. A task row is one or more display lines.
_Avoid_: row, wrapped row

**Row wrap**:
Spreading a task row whose text is wider than the pane over several display lines, breaking between words. Continuation lines start under the task text, not under the priority. On by default; `w` toggles it.
_Avoid_: line wrap, soft wrap

**Wrap line limit**:
The most display lines one task row may take (`wrap_max_lines`, default 2, `0` = no limit). A task row with more text ends its last display line in `…`. With row wrap off the limit is effectively 1.
_Avoid_: max rows, row limit

**Fork release**:
A tagged build+publish run of `khanhtd36/tuxedo`, triggered by pushing a `fork-v*` tag (e.g. `fork-v2026.8.1-khanhtd36.1`). Publishes a GitHub Release, submits a winget update, and bumps the Homebrew formula. Its own tag namespace, so upstream's `v*` tags can never trigger it.
_Avoid_: release (ambiguous: could mean upstream's)

**Fork version**:
`<upstream version>-khanhtd36.<N>` in `Cargo.toml`. `N` counts fork releases on one upstream version and resets to 1 when an upstream sync changes the base.

**Installer scripts**:
`website/install.sh` (POSIX) and `website/install.ps1` (Windows). They fetch the latest fork release asset straight from the GitHub Releases API and verify its `.sha256`.
_Avoid_: install script (singular: there are two)

**Install site**:
`tuxedo.khanhtd36.dev`, a GitHub Pages deployment of `website/` that serves the installer scripts.

**Fork winget package**:
`khanhtd36.tuxedo-khanhtd36` in winget-pkgs, updated by each fork release.

**Self-update**:
tuxedo replacing its own executable with the latest fork release: download the asset for this platform, verify its `.sha256`, swap it in. Works while tuxedo is running; on Windows the running executable is renamed aside (`tuxedo.exe.old`) and removed at a later start. Copies managed by Homebrew or Cargo don't self-update; tuxedo prints their upgrade command instead. A copy in the fork winget package's folder does self-update, so winget's recorded version can lag the real one.
_Avoid_: in-place update, auto-update, upgrade

**In-app update**:
A self-update started from the TUI rather than from `tuxedo update`.
_Avoid_: in-place update

**Fork Homebrew formula**:
`khanhtd36/tap/tuxedo-khanhtd36` in `khanhtd36/homebrew-tap`, rewritten by each fork release. Conflicts with upstream's `tuxedo` formula.

**Cursor**:
The highlighted task row in the list or archive view (`app.cursor`). Moving to a row (keyboard or click) and activating it (`e`/`i`/`x`, or double-click) are separate actions.
_Avoid_: selection (reserve for multi-select), focus (the codebase has no single unified focus concept — see Chooser)

**Chooser**:
A menu-like overlay where moving to an entry and activating it are the same action: the command palette, the project/context/saved-filter pickers, and the theme picker. Click behavior on a chooser entry is select-and-activate, unlike a Cursor row.
_Avoid_: picker (too narrow — excludes the command palette and theme picker), menu

**Multi-select**:
The set of tasks marked in Visual mode (`app.selection`, toggled with Space), distinct from the Cursor (which row is highlighted) and from a Chooser's single active entry.
_Avoid_: selection (alone — ambiguous with text selection and Cursor)
