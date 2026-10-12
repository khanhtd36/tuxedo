---
name: sync-fork
description: Sync this fork's main with upstream/main, verify, and push — then report. Invoke by hand as /sync-fork.
disable-model-invocation: true
allowed-tools: Bash(git:*) Bash(cargo:*) Bash(gh:*)
---

Fixed pipeline, no arguments, run start to finish every time it's invoked. Every
step below still owes the final report — an early stop skips later steps, not
the report.

## 1. Preflight

Confirm the current branch is `main` and `git status --porcelain` is empty.
Either check fails → stop, tell the user which one and why, skip everything
else including push, then go straight to the Report with the other
sections marked not reached.

## 2. Fetch

`git fetch upstream`. Run `git log --oneline main..upstream/main` and keep
its output verbatim as the **changelog** — one line per upstream commit about
to be merged in.

Empty output → nothing to merge. Skip straight to the Report: changelog is
"up to date, nothing to merge", conflict resolution and verify/push status are
"not reached".

## 3. Merge

`git merge upstream/main --no-edit`.

On conflict, resolve only these two kinds yourself; leave anything else in
progress exactly as git left it, list it with
`git diff --name-only --diff-filter=U`, and skip build/test/push:

- `README.md`: keep ours (`git checkout --ours README.md`). The fork README
  defers docs to upstream on purpose.
- `tests/snapshots/*.snap`: take theirs, then after the merge is otherwise
  resolved run `INSTA_UPDATE=always cargo test --test snapshots --locked`
  and review the regenerated diff. Row wrap changes most full-frame
  snapshots, so these conflict on almost every upstream UI change.

## 4. Verify

`cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`,
then `cargo test --locked --no-fail-fast`. On Windows two upstream failures are
expected and don't block: the `unreachable_patterns` lint in `src/serve/mod.rs`
and `ui::title::tests::keeps_absolute_path_when_home_is_unknown`. Anything else
failing → stop before push. Keep the failing command and the tail of its output
for the report.

## 5. Push

```
GH_CONFIG_DIR="$HOME/.config/gh-khanhtd36" git push origin main
```

Never push tags from this clone: it holds upstream's `v*` tags.

## 6. Report

Always end with exactly these three sections, in this order, regardless of
where the pipeline stopped:

```
## Changelog
<the commits merged in, or "up to date, nothing to merge">

## Conflict resolution
<"none", what was auto-resolved (README, snapshots), or the conflicting
files left in progress for the user to resolve by hand>

## Verify and push status
<fmt, clippy and test results, and whether the push succeeded — or
"not reached" for whichever steps a stop above skipped>
```

A new upstream version means the next fork release resets `N` to 1 (see the
fork-release skill).
