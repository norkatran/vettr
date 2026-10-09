# 0012: Command-line arguments

Status: Fulfilled (not yet compiled: the authoring environment had no Rust toolchain, so run `cargo fmt`, `cargo clippy --all-targets` and `cargo test` before relying on it)

## Problem

vettr always opens the last project and the last used credential profile. Launching it from a terminal or script for a specific repository, or as a specific identity ("Work" vs "Personal"), needs the UI.

## Decision

`vettr [OPTIONS] [PROJECT]`

- **`PROJECT`** (positional, optional): a folder, relative paths resolved against the working directory. Resolved to its git repo root like the Open Project picker (a subfolder opens its repo) and made the current project, so it also lands first in Recent Projects. Without it, the persisted project opens as before.
- **`-p, --profile <NAME|ID>`**: the credential profile ("token") for this launch. Matches an id exactly, else a name ignoring case. It is **not remembered**: it only sets this process's active profile (`ProfileStore::select`), consistent with design 0006 where selection is per instance and `lastUsedId` only seeds new ones. Without it, the last used profile applies.
- `-h/--help`, `-V/--version`, `--` to end options.
- **Errors:** malformed arguments print usage to stderr and exit 2 before any window. An unknown `--profile` aborts the launch: `main` checks it before any window opens, prints the available names to stderr and exits 2 (a wrong identity must not silently run as another). A non-git project is reported on stderr and in an error dialog, then the app starts with the persisted project.
- **Multiple instances:** each launch is its own process (as before); no single-instance forwarding.
- **Where:** pure parsing in `src/cli.rs` (hand-rolled, no new dependency); applied in `Backend::new` (`apply_launch_options`) before the launch project starts, so no flash of the old project.

## Out of scope

Single-instance forwarding to a running window, selecting a profile by supplying a raw credential, opening a specific view or session, `.desktop` file `%f` wiring.

## To do

- [x] Design
- [x] `src/cli.rs` parser with tests
- [x] `ProfileStore::select` with test
- [x] Wire into `main.rs`, `VettrApp`, `Backend::new`
- [x] Brief and README updated
- [ ] Compile, clippy and run the tests
