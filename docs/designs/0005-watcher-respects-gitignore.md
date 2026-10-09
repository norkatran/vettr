# 0005: File watcher respects .gitignore and survives project switches

> Note: since design 0007 the app is Rust + egui (it was Electron + TypeScript + React). Outdated references below are ~~struck through~~ and followed by the current equivalent.

Status: Fulfilled

## Problem

Opening another project can freeze the app and stall the whole OS. The cause is the file watcher (~~`src/main/watcher.ts`, started by `activateProject` in `src/main/index.ts`~~ now `src/host/watcher.rs`, started by the `Backend`):

1. **It watches ignored trees.** `isIgnored` skips only `node_modules` and most of `.git`. It never consults `.gitignore`, so Composer's `vendor/`, build output, caches and virtualenvs are all watched. chokidar 4 and later has no fsevents backend, so on macOS it opens one `fs.watch` handle per file or directory. A large ignored tree can exhaust the system's file descriptors.
2. **Project switches can leak watchers.** `watchProject` sets `stopWatching = null` before awaiting the new watcher's initial scan. If another project opens during that scan, the second call sees no previous watcher, and the first one is overwritten and never closed. Each leak keeps a full-tree watcher alive.
3. **Failures are silent.** The `error` handler discards everything, so `EMFILE` and `ENOSPC` go unnoticed.

## Decision

- **Ask git once, up front.** Before starting chokidar, list the ignored paths with a single command, `git ls-files --others --ignored --exclude-standard --directory -z`. It reports fully ignored directories as a single entry (`vendor/`) instead of enumerating their files, so it stays cheap on large trees. The watcher ignores those directories and everything under them. A per-path `git check-ignore` is rejected, because it would run once per discovered file.
- **Git decides what is ignored, nothing is hard-coded.** The built-in `node_modules` rule is removed: a directory is skipped only if `.gitignore` (or another git exclude) says so. A project that tracks or deliberately un-ignores `node_modules` gets it watched. Only the `.git` allowlist (`HEAD`, `index`) stays, since git's own directory is never reported as ignored.
- **Fail soft.** Projects are always opened at a git repo root, so the command should not fail. If it does, watch with the `.git` allowlist only, rather than not watching, and log the error.
- **Serialise watcher changes.** `watchProject` runs through a promise chain, like `AgentManager`. A start for a project that was replaced while it initialised is closed immediately, so at most one watcher is alive.
- **Surface errors.** Watcher errors are logged. On `EMFILE` or `ENOSPC` the watcher is closed, so the failure does not degrade the machine further. The window-focus reload remains as the fallback.

## Scope

In scope: the ignore list, the project-switch race and watcher error handling ~~in the main process~~ in the host.

Out of scope:
- Reacting to `.gitignore` edits while a project is open. The list is computed when the watch starts, and the focus reload covers the gap.
- Negated patterns that un-ignore a path inside an ignored directory. `--directory` collapses these cases, so the directory stays ignored.
- Changes to the agent or sandbox lifecycle.

## To do

- [x] `listIgnored(root)` in ~~`src/main/git.ts`~~ `src/host/git.rs`, with tests, using `--directory -z`.
- [x] Use it in `watchTree`: build the ignore matcher from the result and remove the hard-coded `node_modules` rule from `isIgnored`, falling back to the `.git` allowlist alone on failure.
- [x] Serialise project switches (~~`createProjectWatcher` in `src/main/watcher.ts`~~ the project watcher in `src/host/watcher.rs`) and close stale watchers. Switches replaced before their turn never start a watcher at all.
- [x] Log watcher errors and close the watcher on `EMFILE` or `ENOSPC`.
- [x] Tests: ignored directory produces no events, and rapid project switches leave one watcher open.
- [x] Update the file watching paragraph in `docs/PROJECT_BRIEF.md` (it currently says `.gitignore` is not consulted).
- [x] Mark this design `Fulfilled`.
