# tuxedo-khanhtd36

> **Unofficial fork.** This is `khanhtd36`'s fork of [tuxedo](https://github.com/webstonehq/tuxedo), not affiliated with or endorsed by the original project. For the official release, see [webstonehq/tuxedo](https://github.com/webstonehq/tuxedo).

A fast, keyboard-driven terminal UI for [todo.txt](http://todotxt.org/).

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Release](https://img.shields.io/github/v/release/khanhtd36/tuxedo)](https://github.com/khanhtd36/tuxedo/releases)

![tuxedo demo](docs/demo.gif)

## what's different in this fork

- Long task rows wrap onto more lines in the list and archive views instead of being cut off at the pane edge ([#1](https://github.com/khanhtd36/tuxedo/pull/1)). A row takes at most two lines by default and ends in `…` when there's more text. `w` turns wrapping off and on; with it off, a cut row still ends in `…`. Configure in `config.toml`:

  ```toml
  wrap_rows = true     # false keeps every row on one line
  wrap_max_lines = 2   # 0 = no limit
  ```

- tuxedo updates itself from this fork's releases, not upstream's. `tuxedo update` installs the latest release (`--check` only reports it); in the TUI, `U` checks and offers it, and the download runs while you keep working. It works on a running copy, Windows included. A copy installed by Homebrew or Cargo shows the command to run instead.

This list covers behavior you'd notice while using tuxedo day to day; repository/process-only differences (the release pipeline, install scripts, contributor tooling) aren't included here.

## install (this fork)

```sh
curl -fsSL https://tuxedo.khanhtd36.dev/install.sh | sh
```

```powershell
irm https://tuxedo.khanhtd36.dev/install.ps1 | iex
```

```sh
winget install khanhtd36.tuxedo-khanhtd36
```

```sh
brew install khanhtd36/tap/tuxedo-khanhtd36
```

Binaries and checksums: [releases](https://github.com/khanhtd36/tuxedo/releases).

## docs

Everything else — usage, keybindings, configuration, the CLI, contributing — lives upstream: [webstonehq/tuxedo](https://github.com/webstonehq/tuxedo).

## development

- `mise run preflight` (fmt, clippy, tests; run before every commit)
- `mise run test_update_snapshots` after an intentional UI change

## license

MIT
