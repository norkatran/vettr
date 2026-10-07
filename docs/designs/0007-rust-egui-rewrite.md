# 0007: Rewrite the app in Rust with egui

Status: Fulfilled

## Problem

vettr was an Electron + TypeScript + React app. The product owner asked for the whole app to be rewritten in Rust with egui: one native process, no Chromium, a much smaller footprint.

## Decision

- The desktop app is a single Rust binary (`eframe`/`egui`). There is no main/renderer split and no IPC layer: the UI talks to a `Backend` (`src/backend.rs`) that owns the host services and pushes events to the UI thread over `std::sync::mpsc` channels, calling `egui::Context::request_repaint` when something arrives.
- No async runtime. Blocking work (git, docker, HTTP) runs on worker threads started by the `Backend`.
- **The sandbox runner stays TypeScript.** It runs inside the Docker container and wraps the Claude Agent SDK, which has no Rust equivalent. It moved to `runner/` with its own `package.json`; the host talks to it over the same JSON-lines protocol on stdio. Everything else (git, docker, watcher, stores, diff, comments, session reducer, UI) is Rust.
- Session listing and replay no longer use the SDK's `listSessions`/`getSessionMessages` (there is no Node in the host). The host reads the SDK's jsonl transcripts directly (`src/host/sessions.rs`) and runs them through a Rust port of the runner's `Translator` (`src/host/translate.rs`).
- Secrets (API keys, OAuth tokens) go in the OS keychain through the `keyring` crate instead of Electron `safeStorage`; `profiles.json` keeps only metadata. If the keychain is unavailable saving fails; there is no plain-text fallback.
- Syntax highlighting uses `syntect` (pure-Rust regex engine) instead of highlight.js.
- Assistant text is still plain text (no Markdown), as before.
- Config lives under `dirs::data_dir()/vettr` (the old Electron `userData`).

## Porting conventions (for everyone working on this)

- Source lives in `src/` (it lived in `rs/` while the Electron code still existed). Modules mirror the TypeScript: `src/shared/fooBar.ts` becomes `src/foo_bar.rs`, `src/main/fooBar.ts` becomes `src/host/foo_bar.rs`, React components become `src/ui/*.rs`.
- Rust 2021, stable, no nightly features, no `unsafe` except `libc` calls. Only the crates in `Cargo.toml`. If you truly need another crate, say so in your report rather than adding it.
- Names are snake_case; types keep their TS names. Errors are `Result<T, String>` with the user-facing message (the TS code resolves to `null | string`; use `Result<(), String>`).
- Types that cross a process or disk boundary derive `Serialize`/`Deserialize` with `#[serde(rename_all = "camelCase")]` so JSON formats match the TypeScript ones. The runner protocol (`src/agent.rs`) must match byte for byte.
- Every TS test file is ported to a `#[cfg(test)] mod tests` in the same Rust file, keeping the cases (they are the spec). Use `tempfile` for filesystem tests.
- No async, no global mutable state except where the TS had a singleton. Share state with `Arc<Mutex<_>>` and channels. Dependencies that the TS injected (spawn, clock, keychain) stay injected, as traits or boxed closures, so logic stays testable.
- **No compiler was available while this was written**, so code must be conservative: prefer owned `String`/`Vec` and `.clone()` over clever lifetimes, avoid borrow-checker traps (no holding a `MutexGuard` across a call that locks again), annotate types on closures when inference is doubtful, and check every external API against the crate source (downloaded under `/tmp/crates/<crate>-<version>/` in the authoring environment, or docs.rs) rather than memory. egui 0.36 differs from older versions.
- Keep UI code thin: logic belongs in the non-UI modules so it can be unit tested.

## Scope

All behaviour in `docs/PROJECT_BRIEF.md` carries over. Out of scope: new features, Markdown rendering, a Windows/macOS port beyond what the crates give for free (Linux first, as before).

## To do

- [x] Skeleton, `agent.rs` protocol types
- [x] Pure logic modules (diff, highlight, comments, replies, resolution, review send, fuzzy, slash commands)
- [x] Pure logic modules (session, sessions, readiness, repo status, git actions, profiles, credential, settings, projects, editor, theme, sandbox args)
- [x] Host: git, watcher, transcripts, sessions replay, translate
- [x] Host: stores, keychain, API key check
- [x] Host: sandbox runtime and image, Claude adapter, agent manager
- [x] Runner moved to `runner/`
- [x] Backend glue
- [x] UI: shell, session view, changes view
- [x] Delete the Electron code, move `rs/` to `src/`
- [x] Update brief, README, CI

## Outcome

- The Electron app is gone. The crate is at the repo root (`Cargo.toml`, `src/`), and only the sandbox runner remains TypeScript, in `runner/` (own `package.json`, Vitest, Biome). CI has an app job (fmt, clippy, test, release build) and a runner job.
- All behaviour in the brief carried over. `docs/PROJECT_BRIEF.md` and the README were updated to the Rust architecture.
- **Dropped:** migration of legacy keys and profiles (the old `safeStorage` files cannot be read from Rust, so users re-enter their key once), and the per-file 100% coverage gate for the app (the runner keeps its gate).
- **Duplicated on purpose:** the SDK message translator exists in TypeScript (`runner/src/translate.ts`, live events) and Rust (`src/host/translate.rs`, transcript replay).
- **Tests:** 536 Rust unit tests and 45 runner tests. Unlike the authoring notes above (written before a compiler was available), the build was verified with a compiler during the port.
- **Port notes and limitations:** the environment had no GPU or display, so the UI was verified only by headless egui frame tests (`src/ui/app.rs`) and unit tests, not by eye. The Docker sandbox flow was not exercised end to end. Assistant text is still plain text and long diff lines are clipped (no horizontal scroll).
