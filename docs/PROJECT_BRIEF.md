# Agent-first review IDE: project brief

Project name: **vettr** (pronounced "vetter": to vet a change is to check it carefully). This brief captures the concept, the decisions made so far, the known hard problems, and a suggested build order. It is written to be handed to a coding agent as starting context.

Items marked **Decided** came from the product owner. Items marked **Suggested** are recommendations that have not been confirmed and can be changed. The former open questions are now settled; see section 8.

## 1. Concept

A native desktop app (Rust and egui) for working on a code repository through agents rather than through an editor. The closest mental model is **GitHub Desktop plus an agent**: it shows diffs, commits, and pushes, and it hands off to a real editor for any manual editing.

The user never browses a file tree to see what happened. They prompt, watch the agent work, review a pull-request-style diff, comment on lines, send the comments back to the agent, and commit when satisfied.

## 2. Core user flow

1. **Open a project** from the File menu (Open Project, Ctrl+O, or Recent Projects). The last project is reopened automatically on launch. The app shows the Session view in its empty state: a prompt input.
2. **Enter requirements.** The prompt is passed to an agent.
3. **Watch the agent work.** The session view looks like any coding harness: streamed messages, tool calls and file edits. The user stays in the loop.
4. **Open the Changes view.** A single diff/changes button shows everything that changed, laid out like a merge/pull request comparison on a git host.
5. **Comment on specific lines.** Comments are batched and sent to the agent(s) to fix.
6. **Repeat** steps 3 to 5 until the changes are right.
7. **Commit and push.** Stage the files, write a message, commit, push, as usual.

## 3. Decisions

| Topic | Status | Decision |
| --- | --- | --- |
| Platform | Decided | Native desktop app, not a browser tab: a single Rust binary, no Chromium (design [0007](designs/0007-rust-egui-rewrite.md); it was an Electron app before). You select the app and you're in it. |
| Language | Decided | Rust for the app. The sandbox runner stays TypeScript because it wraps the Claude Agent SDK, which has no Rust equivalent. |
| UI framework | Decided | `eframe`/`egui` 0.36, immediate mode, no async runtime. |
| Look and feel | Decided | VS Code / Atom-style UI, drawn with egui. |
| Paradigm | Decided | Agent-first. The prompt (the empty state of the Session view) is the entry point. |
| Navigating changes | Decided | A diff/changes view replaces the sidebar file tree as the main way to see what happened. |
| Sidebar | Decided | VS Code-style activity bar (Session, Changes) with a collapsible side panel (click the active icon or Ctrl+B); the Session panel holds the "New session" button. It is for switching surfaces and surface-specific lists, not a project file tree. |
| Review loop | Decided | Line-level comments on the diff, sent back to the agent(s) as a structured XML block; the agent answers through a reply tool and only the user resolves threads. |
| Manual editing | Decided | Out of scope. "Edit manually" opens the file in the user's own editor/IDE, which is user-configurable. |
| Git | Decided | Stage, commit, and push from inside the app. |
| Agent implementation | Decided | Wrap the Claude Agent SDK (Claude Code) rather than writing a harness. The wrapper (the runner, `runner/`) is TypeScript and runs in the container; the Rust host drives it over JSON lines on stdio behind an adapter interface. |
| Sandbox | Decided | Agents run with full permissions inside a Docker container with the project bind-mounted. Docker is a hard dependency. API keys and tokens are kept in the OS keychain (`keyring` crate), never in a file. |
| Commit and push | Decided | Strictly user-initiated via UI buttons; the agent never commits or pushes. The container has no git credentials and `.git` is mounted read-only. |
| First platform | Decided | Linux first. |
| Project persistence | Decided | The last opened project is reopened on launch; opening another project makes it the new default. File > Recent Projects lists the last 10 (most recent first). Stored by the host in `projects.json` under the data dir (`dirs::data_dir()/vettr`, the old Electron `userData`); folders that no longer exist are dropped. |
| Non-git folders | Decided | A project must be inside a git repository. The picker result is resolved with `git rev-parse --show-toplevel`, so a subfolder opens its repo root. Anything else is rejected with an error dialog (and dropped from recents); vettr never runs `git init` itself. The same check runs on Recent Projects clicks and on the persisted project at launch. |
| Status bar | Decided | Footer showing repo name, branch (short SHA when detached), `↓behind ↑ahead` against the upstream (zero counts hidden; "no upstream" when none is configured) and changed-file count. The branch and ahead/behind area is clickable to push when there is something to push. Read on the host with `git status --porcelain=v2 --branch` (using `--no-optional-locks`), refreshed on window focus rather than polling. |
| Command palette | Decided | Ctrl+Shift+P palette listing global git commands from a registry; see 6.2. |
| Themes | Decided | MVP ships light and dark only, as two egui visual/palette sets (`src/theme.rs`, `src/ui/palette.rs`). Follows the OS by default; a status bar toggle sets an explicit choice remembered in `ui_state.json` in the data dir. User-customisable or importable themes are post-MVP. |
| Brand | Decided | Logo is the "Delta Square": a square and its shifted copy with only the change inked (ink for what was, signal green for what is now), plus a custom lowercase geometric wordmark "vettr" whose shared crossbar across the two t's is the same green. Colours: ink `#12141A`, signal green `#12A474`, paper `#F4F5F7`, green on dark `#3DDC97`. Source of truth is `branding/` (generated by `branding/build_logo.py`; guidelines in `branding/README.md`). App icon, favicon and one-colour/on-dark variants are supplied. Trademark search and print proofing are still open. |

### Explicitly out of scope

To prevent drift towards a full IDE, the app does not include:

- A text editor (no editable buffers)
- Language servers, autocomplete, or go-to-definition
- A debugger
- An extension system
- A file tree as a primary navigation surface

If a feature request implies any of these, the answer is "open it in your editor".

## 4. Suggested architecture

All of this is **Suggested**.

- **Shell:** one Rust binary (`eframe`/`egui`), crate at the repo root (`src/lib.rs`, `src/main.rs`). There is no main/renderer split and no IPC. The UI talks to a `Backend` (`src/backend.rs`), which owns the stores, the agent manager and the project watcher and pushes events to the UI thread over `std::sync::mpsc` channels, calling `egui::Context::request_repaint` when something arrives; the UI drains them every frame. There is no async runtime: blocking work (git, docker, HTTP, file dialogs via `rfd`) runs on worker threads, started from the UI through `Task::spawn` (`src/task.rs`) and polled from frames. Layout: pure logic in `src/*.rs` (diff, comments, session reducer, profiles, readiness and so on), host services in `src/host/`, egui code in `src/ui/`, with logic kept out of the UI modules so it can be unit tested.
- **UI:** egui (`src/ui/`; `app.rs` is the shell, with `session.rs`, `changes.rs`, `sidebar.rs`, `status_bar.rs`, `command_palette.rs`, `settings_view.rs` and others). Two main surfaces: Session (the prompt when no session is active, then agent activity with a follow-up input; a "New session" action returns to the prompt) and Changes (diff, comments, commit). There is no separate Home screen: with a single agent there is at most one session, so Home would only be the Session empty state. Revisit if parallel agents arrive (a session list in the side panel).
- **Agent:** Claude Code through the Claude Agent SDK, rendering its event stream. The SDK runs inside the sandbox container, in a small runner script. The Rust host starts the container and exchanges JSON lines with the runner over stdio. Keep this behind an adapter interface so that other agents can be added later.
- **Sandbox:** A Docker container with the project bind-mounted, run with the host uid/gid so files are not root-owned. The API key is stored on the host in the OS keychain (service `vettr`, via the `keyring` crate) and sent to the runner over stdin, never as an env var, `--env-file` or file, so it does not appear in `docker inspect`. The agent can still read it from the process that holds it, so this is an MVP measure; the planned hardening is a host-side proxy that adds the key to requests, leaving the container with only a placeholder token and `ANTHROPIC_BASE_URL` (to be verified against the SDK). Network access is open for now. The image holds Node, the SDK and the runner, and may later be overridden per project. The container has no git credentials, and the project's `.git` directory is bind-mounted read-only. This stops the agent committing and, importantly, stops it editing hooks or config, which the host's `git` would later run with the user's credentials. Read-only git commands (`git diff`, `git log`) still work; anything that writes (including index refreshes by `git status`) fails inside the container. Layouts where `.git` is a file or sits outside the project (worktrees, submodules) need the real git directory mounted read-only too.
- **Diff rendering:** Decided: the host runs `git diff` and parses it (`src/diff.rs`) into files, hunks and numbered lines; the UI draws them as plain tables in unified and split layouts, with all files stacked PR-style and the file list in the Changes side panel. This avoids a heavy editor-widget dependency and gives a structure that line comments can anchor to. Syntax highlighting uses `syntect` (pure-Rust regex engine, bundled grammars, chosen by file extension in `src/highlight.rs`): each hunk's old side (context + deletions) and new side (context + additions) are tokenized separately so multi-line constructs mostly colour correctly. Unknown extensions stay plain. Highlighting runs synchronously per hunk; make it lazy if very large diffs feel slow. The baseline includes untracked files without touching the user's index: everything is staged into a temporary copy of the index (`GIT_INDEX_FILE`) and diffed with `--cached` against `HEAD` (or the empty tree before the first commit), with rename detection on. A file with more than 5000 changed lines keeps its counts but its hunks are dropped, and binary files show a placeholder. Long diff lines are clipped, with no horizontal scroll yet.
- **Git:** Shell out to the user's installed `git` on the host. Their existing credentials, SSH keys, and config then work with no extra setup. Commit and push happen only when the user presses the button.
- **File watching:** Decided: the host watches the project with the `notify` crate (`src/host/watcher.rs`), debounced to 250 ms, and sends a repo-changed event to the UI, which reloads the Changes view and status bar. What git ignores is not watched: when a watch starts, one `git ls-files --others --ignored --exclude-standard --directory -z` (`list_ignored` in `src/host/git.rs`) lists the ignored paths, with fully ignored directories such as `vendor/` or `node_modules/` as single entries, and the watcher skips them and everything under them. Nothing is skipped by name, so `node_modules` is watched unless `.gitignore` excludes it. The list is read once per watch start, so `.gitignore` edits apply on the next project open. Inside `.git` only `HEAD` and `index` are watched so staging, commits and branch switches refresh the view. If git cannot list the ignored paths, the error is logged and the watcher runs without that filter. Project switches go through the project watcher in `src/host/watcher.rs`, which queues them and closes a watcher that finished starting after the project changed, so at most one is alive. Watcher errors are logged, and running out of handles (`EMFILE`, `ENFILE`, `ENOSPC`, mapped from `notify` errors) closes the watcher instead of straining the machine. Window focus still triggers a reload as a fallback. See design 0005.
- **Multiple agents (later):** One git worktree per agent, so parallel agents cannot overwrite each other. The MVP runs a single agent.

### Sandbox image and runner

Decided: `sandbox/Dockerfile` builds `vettr-sandbox` from `node:22-slim` (glibc, because the SDK ships a native `claude` binary per platform) with `git`, `ripgrep`, `curl`, Python (pip, venv) and a C build chain, the Agent SDK installed at the version pinned in `runner/package.json`, and the bundled runner as the entrypoint. `npm run build:sandbox` (run in `runner/`) bundles `runner/src` with esbuild (SDK kept external, output `runner/dist/runner.mjs`) and builds the image. The runner reads commands on stdin and writes events on stdout; stderr (including the SDK's own) is diagnostics only. It uses `bypassPermissions`, loads project settings only (so a project `CLAUDE.md` applies but nothing from the container home) and passes the API key to the SDK process as `ANTHROPIC_API_KEY` (the key is visible to the agent, as noted under Sandbox). The container runs as the host uid/gid with a real world-writable `HOME=/home/agent` created in the image (not a tmpfs; it is discarded with the container). User-level installs work through prefixes set in the image (`NPM_CONFIG_PREFIX`, `PIP_USER`, `PYTHONUSERBASE`, `CARGO_HOME`, with their bin dirs on `PATH`); system installs (`apt`, `sudo`) stay impossible because the container is unprivileged. Translation of SDK messages into app events lives in `runner/src/translate.ts` (the host has a Rust port of it, see 5.8, so the logic exists twice and the two must stay in step): a `file-edited` event is emitted when an Edit, MultiEdit, Write or NotebookEdit call finishes without an error. A failed turn yields `error` then `turn-finished`, so the UI always unlocks.

### Container lifecycle

Decided: the container is started by `start_sandbox` (`src/host/sandbox_runtime.rs`) as `docker run --rm -i --init` and stopped with SIGTERM to the `docker` process, which `--init` forwards. Arguments come from `build_run_args` (`src/sandbox.rs`): host uid/gid, all capabilities dropped, `no-new-privileges`, and the project mounted at the same absolute path as on the host, so paths in events (such as `file-edited`) need no translation. Read-only overlays come after the project mount: the git dir, the common dir for linked worktrees, and the `.git` file in worktrees and submodules (so the agent cannot redirect it). Verified against real Docker: the agent can write project files (owned by the host user), but writing hooks fails and `git commit` cannot take the index lock, while `git log` works. `check_docker` reports a missing daemon or missing image with a message saying how to fix it.

SELinux: on hosts with SELinux enforcing (Fedora, RHEL) the container cannot use bind mounts at all by default. Decided: run with `--security-opt label=disable` rather than relabelling with `:z`, which would change labels on the user's files. Isolation still comes from Docker's namespaces and the dropped capabilities.

Secrets are stored by `KeyringStore` (`src/host/secret.rs`) in the OS keychain through the `keyring` crate, behind a `SecretStore` trait (injected so it is testable). There is no secret file, and saving fails if the keychain is unavailable instead of falling back to plain text. Non-secret files are written atomically (mode 0600 where it matters).

### Claude adapter and backend API

Decided: `ClaudeAdapter` (`src/host/claude_adapter.rs`) implements the interface over the container. Its dependencies (Docker check, container start and stop, API key) are injected so it is tested against a fake process. `warm(cwd, resume?)` claims the single session slot synchronously (so concurrent starts cannot both proceed), checks Docker, reads the key, starts the container and writes `init`, leaving an idle query; `start` then only sends the first prompt. A warm agent is reused only when its project and resume id match, otherwise `start` restarts it; a `warm` that arrives while one is in progress joins it. Both reject with a user-facing message when Docker or the image is missing or no key is saved. Container stdout goes through `LineBuffer` and `parse_event_line` (`src/agent.rs`); stderr is kept (last 2000 characters) and attached to the error shown if the container dies unexpectedly. When the container exits the adapter emits `exited`, preceded by an `error` only when the exit was unrequested, non-zero and the runner had not already reported one. A fatal SDK error ends the runner process (it reports one `error`, then exits with code 1), so a failed turn ends the session: the user starts a new one. `stop()` is quiet. The adapter is driven by the agent manager (next section), which also stops it on a project switch or app quit.

Backend API (`src/backend.rs`, replacing the old IPC layer): methods such as agent start (prompt, optional resume), send, interrupt and stop (which means "new session": see the manager), readiness, and profile add, update, remove and set-active; events (agent events, readiness, profile changes, repo changes) arrive on one channel the UI polls. Fallible calls return `Result<T, String>` with a user-facing message. Agent start uses the backend's current project rather than a path from the UI, and the key never reaches the UI after it is saved.

### Agent lifecycle and readiness

Decided (design [0002](designs/0002-agent-lifecycle.md)): the agent is **prewarmed** and every agent-directed input is gated on one readiness state owned by the host. `AgentManager` (`src/host/agent_manager.rs`) drives the adapter and owns `Readiness` (`src/readiness.rs`): `status` is `idle | starting | ready | stopping | error` and `reason` is `no-key | docker-unavailable | building-image | starting | crashed-restarting | offline | error`, with an optional message. It is pushed to the UI as backend events.

- **Launch and project open:** check Docker, then a saved key, then warm the agent for the project (the container and an idle SDK `query()`, so `Query` methods such as `supportedCommands()` are available before any prompt; verified to resolve in about 0.6 s). No key leaves it `idle/no-key`; Docker problems are `error/docker-unavailable`.
- **Serial queue and project generation:** all transitions run through one queue, so a start never overlaps a stop. A cycle for a project that has been replaced since it was queued is discarded. Opening the same project is a no-op; opening another while a turn is running asks for confirmation (work in progress is lost), then tears the agent down and warms a new one.
- **Resume is a re-init:** `resume` is fixed when the SDK query is created, so starting a stored session restarts the warm agent with it. The first prompt of a never-used warm agent reuses it. "New session" restarts a used agent and does nothing for an unused one.
- **Crash recovery:** an unrequested exit restarts the agent resuming the stored session id, with an info notification, and gives up after 3 crashes with no finished turn between.
- **Key and profile changes:** saving a key for the profile in use, switching profile, or removing the profile in use restarts the agent (or stops it when no profile is left); if a turn is running a confirmation dialog warns that work will be lost (a key is validated before the dialog).
- **Gating:** the prompt, follow-up input and comment editing derive their state only from readiness. With no key the prompt is replaced by the key entry form; otherwise they are disabled with the reason shown. Existing comment threads stay visible but read-only, an unsaved draft is kept, and Send is blocked with the reason. The host also rejects start, send and interrupt unless `ready`. Comments are UI-only state with no backend calls of their own, so they reach the agent only through those gated calls. A review's comments are marked sent only after the agent has taken the message (`send_review_round`, `src/review_send.rs`).
- **Image build:** a missing sandbox image is built by the app (`src/host/sandbox_image.rs`) when `runner/dist/runner.mjs` and the SDK version are available, streaming the latest build line as the readiness message; otherwise the user is told to run `npm run build:sandbox` in `runner/`. A packaged app would need the runner and Dockerfile bundled as resources (shipping work).
- **Slash commands (design [0003](designs/0003-slash-commands.md)):** the runner emits a `commands` event with the full list from `supportedCommands()` once the idle query exists, and again whenever the SDK sends `commands_changed`; the list is always replaced, never merged. `AgentManager` caches it (cleared on exit) for the UI. The composer shows a filterable menu while the text is `/` plus a partial name (`src/slash_commands.rs`): arrows move, Tab or Enter completes, Escape dismisses.
- **Notifications:** `error` and `info` levels (info fades after 6 s). `describe_transition` (`src/readiness.rs`) decides what to announce: Docker problems, image build start and end, crash restarts.
- **Orphan containers:** containers are labelled `vettr.app=1` and `vettr.pid=<owner pid>`; at launch a background sweep removes labelled containers whose owner process is gone (live owners, such as a second vettr, are left alone). The agent is stopped on quit.
- **Not done:** prompt and comment drafts are not kept per project (dropped from the design); the comment draft lives in the Changes view only.

### Session view

Decided: the Session view is a transcript plus one input. Its state is a pure reducer over agent events (`src/session.rs`) held in `AgentSessionModel` (`src/ui/agent_session.rs`) above the views, so switching to Changes and back keeps the transcript, and opening another project resets it. States: idle (the prompt), running (Stop button, "Working..."), waiting (follow-up input) and ended (a banner with New session). Items are the user's messages, assistant text, tool calls (collapsed rows with a one-line summary, expandable to the input and output, output truncated at 5000 characters), file edits (path relative to the project) and errors. A turn the user stopped shows "Interrupted" and hides the error the SDK reports for it. Assistant text is plain text for now (no Markdown rendering; known limitation). A failed start returns to the prompt with the text kept and the message shown.

Credentials: an Anthropic API key or a Claude OAuth token (from `claude setup-token`) is entered in the same field, saved as a named **profile** (design [0006](designs/0006-credential-profiles.md)). Any number of profiles can be saved ("Work", "Personal"; names are user-defined, unique ignoring case, at most 40 characters), in `profiles.json` in the data dir (`src/host/profile_store.rs`, re-read from disk on every operation so concurrent instances do not clobber each other). `profiles.json` holds metadata only (id, name, last used); the credentials themselves are in the OS keychain. Profiles and keys saved by the Electron version (encrypted with `safeStorage`, which Rust cannot read) and the legacy single `apikey` file are not migrated: users re-enter their key once. The profile in use is held **in memory per app process**, so switching in one instance never affects another; the file's `lastUsedId` only seeds which profile a new instance starts with. Windows of one instance share the agent and so the profile. The Session view shows the entry form (with a name field) only when no profile exists; otherwise profiles are managed at the top of the Settings view (Use, Edit to rename or replace the key, Remove, Add profile). Switching restarts the agent keeping the stored session and asks first if a turn is running. If another instance deletes the profile in use, this one reports no key until one is picked. The title bar shows the active profile's name in a chip beside "vettr" (click it for Settings). The value is shown as plain text only while it is being typed and is never displayed again once saved (the UI only learns whether a key exists). `src/credential.rs` tells them apart by prefix (`sk-ant-oat` means an OAuth token, anything else an API key) and the runner sets `CLAUDE_CODE_OAUTH_TOKEN` or `ANTHROPIC_API_KEY` accordingly, never both. The `init` command's field is `credential`. It is checked against `GET /v1/models` before saving (`src/host/api_key_check.rs`, HTTP through `ureq`) because Claude Code treats a rejected key as retryable and backs off through 11 attempts over several minutes, which looks like a hang. Only a 401 or 403 rejects it; being offline does not block saving. OAuth tokens are checked with a narrower rule (a spike showed a bad bearer token gets a distinctive 401 "OAuth access token is invalid"): only a 401 whose message says invalid, expired or revoked rejects it, and everything else saves, because the models endpoint's answer to a valid long-lived token is unconfirmed. Check with a real token before relying on it; first-use failure remains the backstop. Open question: check Anthropic's current terms on using subscription OAuth tokens through the Agent SDK in a third-party app, and keep or remove token support accordingly.

The runner points Claude Code at its own temp directory (`CLAUDE_CODE_TMPDIR`). Mounting the project at its host path makes Docker create the parent directories as root, and Claude Code refuses a root-owned `/tmp/claude-<uid>`, which broke projects under `/tmp`.

### Agent adapter interface (sketch)

The UI should depend only on a small interface, roughly:

- `start(prompt, cwd)` begins a session
- `send(message)` sends a follow-up, including batched review comments
- `interrupt()` stops the current turn
- An event stream covering: assistant text, tool call started/finished, file edited, turn finished, error (and approval requested, once approvals are supported)
- `respondToApproval(id, allow)` is kept in the interface for later; the MVP does not use it as the sandbox grants full permissions
- `stop()` ends the session and releases the container

Decided: the interface and events live in `src/agent.rs` (the protocol types must match the runner's `runner/src/agent.ts` byte for byte). Events are `text`, `tool-started`, `tool-finished`, `file-edited`, `turn-finished`, `error` and `exited`. The runner protocol is one JSON object per line on stdio: the host sends `init` (API key and cwd, always first), `prompt` and `interrupt`; the runner replies with events. `LineBuffer` reassembles lines from stream chunks and `parse_event_line` drops anything malformed.

## 5. Hard problems and how to approach them

### 5.1 Comment anchoring

Once the agent rewrites a file, the lines that comments point at move or disappear. Git hosts handle this by marking comments "outdated".

Suggested approach:

- Store each comment with: file path, diff side (old/new), line range, a snapshot of the commented lines, and the review round it was made in.
- When a new round arrives, try to re-anchor by matching the snapshot text. If it no longer matches, mark the comment **outdated** and keep it visible in a collapsed state.
- Show a round-to-round diff so the user can see what the agent changed in response.

**Built so far (round 1):** clicking a line number in the diff opens a comment editor (shift-click selects a range); the comment is stored with file, staged or unstaged diff, side, range, snapshot, text and round (`ReviewComment` in `src/comments.rs`, held in memory by `ReviewModel` in `src/ui/changes_model.rs` and reset when the project changes). Pending comments can be edited or deleted. "Send N comments to agent" formats them with `format_review` (file, lines, quoted code, comment) and sends them as a follow-up message, so it needs a session waiting for input, or, when no session is live (none started, or the last one ended), it starts a new one with the review as the first prompt (it is blocked unless the agent is ready). The comments are marked sent, greyed, and the round advances only once the agent has taken the message; if delivery fails they stay pending and a notification says so.

**Structured format and agent replies (built, design [0004](designs/0004-structured-review.md)):** a review is sent as a short introduction plus a `<vettr-review round="N">` XML block, one `<comment id file side lines [outdated]>` per comment holding `<code>` (the snapshot) and `<note>` (the text), with XML-escaped content (`format_review` and its inverse `parse_review`, `src/comments.rs`). Every comment has a UUID that stays the same across rounds. The Session page recognises these messages in the transcript and draws them as comment cards, so reviews survive reloads. Opening a stored session rebuilds its sent comments from the transcript (`rehydrate`, then `reanchor`, both in `src/comments.rs`; the message does not record staged or unstaged, so each starts unstaged and is found on either) and continues from the next round; unsent drafts stay in memory. The runner offers the agent an in-process MCP tool, `mcp__vettr__respond_to_comment` (`comment_id`, `message`, optional `kind` of `question` or `resolved`), which does nothing itself: the app reads the call from the event stream (`src/replies.rs`) and shows the reply under its comment in both Session and Changes. The runner rejects unknown ids (not in a resumed session, where it never saw earlier rounds). `kind` is advisory. **Resolution:** the user alone resolves a comment, which resolves its whole thread; the ids are stored by the app per project in `<data dir>/projects/<name>-<hash>/resolved-comments.json` (`src/host/resolved_store.rs`, `src/resolution.rs`), shown collapsed in Session and Changes with Reopen, sent comments only, and the agent is not told.

**Re-anchoring (built):** `reanchor` runs whenever the changes reload (agent edits, manual edits, staging), not only when a round is sent, so manual edits are covered too. It looks for the snapshot as a run of consecutive lines with identical text on the comment's side of the file's diff, trying the same staged/unstaged diff first and then the other, and picks the match nearest the old position when there are several. A match moves the comment; no match marks it `outdated` (it keeps its last position, is not drawn on the diff, and is listed in a collapsed "outdated comments" group with its quoted snapshot). No fuzzy matching, by the decision in 8.3. Outdated is not sticky: if the exact text returns (for example the user undoes an edit) the comment anchors again. Outdated comments can still be sent; `format_review` warns the agent that the line numbers may be stale. Limits: only lines visible in the diff are in the snapshot, so a comment on a line whose context scrolls out of the diff goes outdated; a file rename also goes outdated.

**Round-to-round diff (built):** when a review is sent, the host records the whole working tree (untracked files included, ignored ones not) as a git tree object, using a throwaway index like the Changes view (`snapshot_tree` in `src/host/git.rs`), before the message reaches the agent. That tree id is the baseline for the round; it is held in memory with the comments and replaced by each later send. When a baseline exists, the Changes toolbar offers "All changes" and "Since last review": the latter diffs the current working tree against the baseline (`get_changes_since` in `src/host/git.rs`, refreshed by the same watcher and focus triggers) and is read-only, with no staging buttons or comments. It reflects the user's own edits as well as the agent's, since both are revisions. It is a diff of the tree as a whole, so it ignores staging. The baseline tree is unreferenced, so git could prune it after its two-week grace period; that is fine while comments live only in memory, and persisting them (5.8) would need a hidden ref as in 5.4.

### 5.2 Manual edits made in the external editor

Because editing happens outside the app, the Changes view must update when files change on disk (built: file watcher, plus refresh on window focus).

Decided: the app does **not** track which lines or files were edited externally. Telling the agent about external edits and special-casing comments on externally edited lines were dropped: attributing an edit to the user or the agent is not trivial and the benefit is small. Comments already go outdated through snapshot re-anchoring (5.1), whoever made the edit, and the agent sees the current files on disk when it reads them.

### 5.3 Open in external editor

- Configurable command template, for example `code {project} -g {file}:{line}` (`{project}` makes the editor open the project folder too).
- Clicking from the diff should land on the exact line. Built: each file header in the Changes view has a 3-dot menu (right of Stage/Unstage) whose first item is "Open in editor", landing on the file's first added line (per-line opening may follow). The template is split into argv and `{file}`/`{line}`/`{project}` substituted per argument, then spawned detached without a shell (`build_editor_command` in `src/editor.rs`); the path must stay inside the project. Disabled for deleted files; an unset command tells the user to choose one in Settings.
- Offer presets for common editors plus a custom command.
- Decided: the command lives in a general **Settings** view (a Settings item at the bottom of the activity bar, which opens a view in the main area). Settings are stored host-side in `settings.json` in the data dir, validated on load (`src/settings.rs`, `src/host/settings_store.rs`). The theme (`ui_state.json`) and the keys (keychain) keep their own stores. New preferences should be added there rather than getting their own files.

### 5.4 Checkpoints and undo

Users need to roll back a bad agent turn without losing their own uncommitted work. Options include a snapshot per turn (for example a hidden ref or stash-like commit) that can be restored. Decided: deferred past the MVP; for now the user manages commits and repo state themselves. The session model was reviewed and needs no preparation, because adding checkpoints later is purely additive: a turn number and an optional `checkpoint` ref on user transcript items, plus a host-side snapshot taken in the adapter before each `start` and `send` (no runner protocol change). Prefer a git-based snapshot in a hidden ref, stored per project on the host so it outlives the session (a fatal error ends the session and a project switch resets the transcript). It should not rely on the SDK's own file checkpointing, which would miss Bash-made changes and the user's hand edits.

### 5.5 Command safety

The agent has full permissions, so the Docker sandbox is the safety boundary. Only the project directory is mounted, no git credentials are present, `.git` is read-only, and commit and push stay with the user. Network access is open for now, and the API key is visible to the agent inside the container (see the Sandbox bullet in section 4). Approval prompts with a user-extendable allowlist, restricted networking, and a host-side API proxy are later work.

### 5.6 Diff baseline

"What has changed" needs a defined baseline. Decided: working tree against `HEAD` (including untracked files), since that matches what will be committed. A possible later addition is "changes since this session started" or "changes since the last review round".

### 5.7 Shipping

Code signing, auto-update, cross-platform quirks, and performance on very large diffs (plus binary files and renames) are the long tail. Defer until the core loop works.

The app ships as a single binary (`cargo build --release`, thin LTO, stripped). The sandbox image is built from `runner/dist/runner.mjs` and `sandbox/Dockerfile`, which the app finds next to the working tree (or under `share/vettr` beside the executable); packaging those as resources, installers, signing and auto-update are still to do.

Formatting and linting use `rustfmt` and `clippy` for the app (`cargo fmt --check`, `cargo clippy --all-targets`) and Biome for the runner (config in `runner/biome.json`). Tests are inline `#[cfg(test)] mod tests` in the same Rust files (about 536 unit tests, run with `cargo test`; `tempfile` for filesystem tests, injected traits and closures for spawn, clock and keychain). The runner keeps Vitest (about 45 tests, `cd runner && npm test`) and its 100% per-file coverage thresholds. The Rust app has **no per-file coverage gate any more**: the Electron version enforced 100% on every logic module, and that was not carried over. Keep UI code thin and logic in testable modules. CI runs on Forgejo Actions (`.forgejo/workflows/ci.yml`) with two jobs on pushes to `main` and pull requests: the app (fmt, clippy, test, release build) and the runner (typecheck, lint, format check, test with coverage, build). Nothing is published yet, so there is no release job.

**Known limitations of the port:** the build environment had no GPU or display, so the UI was verified only through headless egui frame tests (the test in `src/ui/app.rs`) and unit tests, not by eye; expect visual polish issues on first real use. The Docker sandbox flow (image build, container start, agent turn) was not exercised end to end during the port. Assistant text is plain text and long diff lines are clipped.

### 5.8 Session persistence and resume

Decided: built in steps (see [design 0001](designs/0001-mvp.md)). The Agent SDK supports resuming (`resume: '<sessionId>'`). The SDK jsonl is the source of truth; vettr keeps no transcript format of its own.

- **Storage (built):** each project has a folder `<data dir>/projects/<name>-<hash of the project path>/transcripts` (`src/host/transcripts.rs`). Moving a project makes it a new one, so its old sessions are orphaned. That folder is mounted into the container at `/vettr-config` and set as `CLAUDE_CONFIG_DIR`, so the SDK writes its transcripts (under `projects/`) to the host and they outlive the `--rm` container. Only that folder is shared, never the host's real `~/.claude`, which would expose host settings and credentials. The host creates it as the host user before `docker run`, and the container runs as the same uid.
- **Listing (built):** the host has no Node, so it does not use the SDK's `listSessions`. `src/host/sessions.rs` reads the SDK's jsonl transcripts directly (`projects/<encoded cwd>/<sessionId>.jsonl` under the project's transcripts folder, the encoded cwd being the absolute path with every non-alphanumeric character replaced by `-`), deriving titles and the message chain the way the SDK does, and returns `{ id, title, lastModified }`, newest first (`src/sessions.rs` has the types and formatting).
- **Session list (clicking built):** clicking a session, or picking it from the palette, replaces the current session and rewrites the chat display from the replay. The Session side panel lists the project's sessions under "New session" as `[timestamp] title` (truncated to fit). A "Session: Resume Session" palette command offers the same list.
- **Replay (built):** `src/host/sessions.rs` reads the stored messages and runs them through `src/host/translate.rs`, a Rust port of the runner's `Translator` (`runner/src/translate.ts`), and the session reducer, returning a `SessionState` left `waiting` with unfinished tools `stopped`. User text becomes a prompt, `[Request interrupted by user…]` becomes an "Interrupted" notice, and user messages starting with a tag (slash-command echoes, system reminders) are dropped. **The translation logic is duplicated** (TypeScript in the container for live events, Rust on the host for replay), so a change to one must be mirrored in the other.
- **Resuming (built):** sending a message in an opened stored session, or in one whose agent exited, makes the manager restart the agent with `resume: <sessionId>` (the UI does not stop it first), adding a "Session resumed" notice to the transcript. **Runner side:** the runner reports the SDK `session_id` as a `session-started` event (emitted by the `Translator` and stored as `sessionId` in the session state), and `init` gains an optional `resume` passed to `query()`. Errors and other transient notices are not stored, so they are not restored.
- **Rules:** switching session is disabled while the agent is working. Resuming while a session is live but idle or waiting ends it. A session that ended or crashed can be resumed too, with a notice. Opening a stored session rebuilds its sent comments from the transcript (see 5.1); round baselines stay in memory and are not kept across a switch. Resolved comments are stored per project, not per session.

## 6. Suggested build order

Each milestone should be usable on its own.

1. **Shell.** Native app, open a project folder, prompt input in the Session view, basic VS Code-style layout.
2. **Changes view.** File list plus diff of working tree against `HEAD`. Unified and split views. Built before the agent session so diffs from an agent run externally (for example Claude Code in a terminal) can be reviewed straight away.
3. **Agent session.** Build the sandbox image and container lifecycle. Wrap the Agent SDK behind the adapter, running in the container. Stream its output, show tool calls and file edits, and handle interrupt.
4. **Commit and push.** Brought forward ahead of line comments because it is small, well defined and completes a usable loop (prompt, watch, review, commit, push). Changes view split into collapsible Staged and Unstaged accordions; file-level stage and unstage only (no hunk staging); commit message input and button; push to `origin`. All user-initiated. See section 6.1.
5. **Line comments.** Add comments on lines or ranges, batch them, send to the agent as a structured message (file, line range, quoted code, comment text). Handle a second round with outdated-comment logic.
6. **External editor.** "Open in editor" from the diff with jump-to-line, and file watching.

### 6.1 Commit and push design

Decided:

- **Staging:** file-level only. Hunk staging is out of scope for now (it needs patch building for `git apply --cached`).
- **Layout:** the Changes view has two accordions, Staged changes and Unstaged changes, so every change is still visible at once with a clear separation of what is where. Each file is listed under the section matching its index state; a partially staged file appears in both.
- **Stage and unstage (built):** `stage_files` and `unstage_files` in `src/host/git.rs`. Staging runs `git add --all -- <paths>` (so deletions and untracked files work) and unstaging runs `git reset -q -- <paths>` (`restore --staged` fails before the first commit). Both use `--literal-pathspecs` so names with glob characters are safe, and a renamed file is moved by passing both its old and new path. They return `Ok` or git's message as the error. Each file in the Changes side panel also has a `+` (stage) or `−` (unstage) button beside its name, in addition to the button on its diff header. The Changes view has per-file and per-group ("Stage all" / "Unstage all") buttons inside collapsing accordions.
- **Commit (built):** a textarea and Commit button at the top of the Changes side panel (`commit_staged` in `src/host/git.rs`, runs `git commit -m`). Ctrl+Enter submits, like the prompt input. The message clears only on success; failures appear as notifications. The user types the message and the host `git commit` runs over the staged files only. While it runs, the message input is greyed out and disabled with a spinner beside it.
- **Push:** the status bar branch and `↑ahead`/`↓behind` area, clickable only when ahead of the upstream, which runs `git push` to the branch's upstream using the host `git`, with prompting disabled (`GIT_TERMINAL_PROMPT=0`, no askpass, SSH `BatchMode`). If any input would be required (passphrase, credentials) the push fails and the user is notified. Interactive auth is later work. The area shows the same busy state (disabled with a spinner) while pushing.
- **Errors (built):** any failure (hooks, auth, rejected push, nothing staged) is shown as a dismissable popup in the top-right of the window with git's output (`src/ui/notifications.rs`, `Notifier`). Popups stay until dismissed. The message input keeps its text so nothing is lost.
- **Publish (built):** when the branch has no upstream, the status bar shows a "Publish Branch" button, as VS Code does. With one remote it runs `git push -u <remote> HEAD` straight away, with several it shows a picker (`origin` first), and with none it shows a notification (`list_remotes` and `publish_branch` in `src/host/git.rs`). It is hidden when HEAD is detached. Same no-prompt environment and busy state as push.

### 6.2 Command palette

Decided:

- **What it is:** a VS Code/Atom-style palette for global commands, opened with Ctrl+Shift+P or the titlebar "Commands" button. It lists git commands (`Git: <title>`) `Session: New Session`, `Session: List Sessions` (a wide second palette of stored sessions, same `[timestamp] title` format as the sidebar), and view jumps (`Jump To: Session | Changes | Settings`, which work without an open project via `needsProject: false`); other commands (agent) can join the same registry later.
- **Registry (built):** `src/ui/commands.rs` holds `{id, category, title, run(ctx, project)}` entries. `run` gets a context with `pick`, `input`, `notify`, `stagedCount`, `focusCommit`, `showView` and `newSession`. The palette (`src/ui/command_palette.rs`) is one overlay that serves both the command list and any follow-up step a command asks for (a branch picker, a name, a confirmation), so those steps stay in the palette. Fuzzy matching is `src/fuzzy.rs`.
- **Always listed:** commands are not hidden or disabled by context for the MVP. A command that cannot run (no project, nothing staged, no other branches, no remotes, git refusing) shows an error notification with the reason. An `enabled(ctx)` hook can be added later without changing the shape.
- **Commands:** Fetch (`--prune`), Pull, Push, Publish Branch, New Branch, Change Branch, Delete Branch, Commit, Stage All, Unstage All, Discard All Changes, Stash, Pop Stash, Merge Branch into Current, Rebase Current Branch onto. Push and Publish reuse the status bar's calls. Out of scope: worktree commands, and pull or merge request management (the project uses Forgejo and GitLab, and supporting several hosts is too much for now).
- **Git actions:** the rest run through one backend call taking a `GitAction` (`src/git_actions.rs`) that `plan_git_action` turns into git commands; `run_git_action` in `src/host/git.rs` executes them with the no-prompt environment and `GIT_MERGE_AUTOEDIT=no`. `list_branches` lists local and remote-tracking branches.
- **Commit:** does not duplicate the Changes view. It shows the Changes view and focuses the commit message input (the commit box), or notifies if nothing is staged.
- **Branches:** Change Branch uses `git switch`, so git decides what happens to uncommitted changes: non-conflicting ones carry over and a conflict is reported with git's message. A remote-only branch is offered too (switching creates a tracking branch). Delete Branch uses `branch -d`, so a branch with unmerged work is refused, and it hides the current branch. Branch names starting with `-` are rejected so they cannot be read as options.
- **Merge and rebase:** run non-interactively. If one fails (conflicts, usually) it is aborted straight away and the notification says so, because the app has no way to continue or abort a half-done merge. Resolving conflicts is a terminal job for now.
- **Discard All Changes:** `reset --hard` plus `clean -fd` (ignored files are kept), behind a Discard/Cancel choice in the palette. It does not work before the first commit.
- **Status refresh:** the status bar reloads after any palette command, since fetch, pull and push change remote-tracking refs that the file watcher does not see.

Later:

- Interactive git authentication for push
- Hunk-level staging
- Checkpoints and undo per agent turn
- Approval prompts, allowlist and restricted sandbox networking
- Host-side API proxy so the real key never enters the container
- Multiple parallel agents using worktrees
- Additional agent adapters
- Packaging, signing, and auto-update
- Markdown rendering of assistant text, horizontal scroll for long diff lines

Rough effort for a solo experienced developer: a prototype in 1 to 2 weeks, a usable MVP in 4 to 8 weeks, a polished product in 6 months or more.

## 7. Competitive landscape

Researched on 2 October 2026. This space moves quickly, so recheck before relying on it.

### Agent-first apps with diff review (closest)

- **Conductor** (Melty Labs): Mac-only app that runs Claude Code and Codex in isolated git worktrees, with in-app diff review and PR handoff. Can open a workspace in an external IDE for editing. <https://conductor.build>
- **Warp:** Code review panel with inline comments on agent diffs, batched and sent back to the agent. <https://docs.warp.dev/agent-platform/local-agents/interactive-code-review/>
- **Codex app** (OpenAI): Agent available as an app, CLI, editor integrations, and cloud environments.

### Small review-loop tools

These implement the line-comment loop as a standalone local web UI.

- **diffx:** <https://github.com/wong2/diffx>
- **CodeChat:** <https://github.com/alexmx/codechat>
- **Crit:** shows a diff between review rounds. <https://sharedcontext.ai/plugins/external/tomasz-tomczyk/crit>

### Full AI IDEs

These are what this project deliberately simplifies away from.

- **Cursor:** VS Code fork with an agent mode.
- **Devin Desktop** (formerly Windsurf): full IDE with agents and review tools.
- **GitHub Copilot:** agent mode with reviewable diffs.

### Agents to wrap rather than compete with

Claude Code, Codex CLI, OpenCode.

### Where this project can differ

- **Cross-platform.** Conductor is Mac only; the Rust crates used here also run on Linux (first target), Windows and macOS.
- **Review experience.** The comment-on-diff loop exists in several tools, so it needs to be noticeably better here: comment anchoring, round-to-round diffs, and a fast path from comment to fix.
- **A native app, not a browser tab.** The small review tools all run as a local server plus a browser tab.
- **Strict scope.** No editor, by design.

## 8. Decisions on former open questions

1. ~~**Which agent to wrap first?**~~ Decided: Claude Code via the Claude Agent SDK, running inside the sandbox container. The Rust host talks to a runner in the container over stdio (JSON lines), behind the adapter interface. Only one adapter in the MVP.
2. ~~**Multi-agent in the MVP?**~~ Decided: single agent. Multi-agent and worktrees are deferred.
3. ~~**Comment rule after manual edits.**~~ Decided: mark as outdated (collapsed, not re-anchored by guessing). No special tracking of external edits: snapshot re-anchoring handles it regardless of who edited.
4. ~~**Diff baseline.**~~ Decided: working tree against `HEAD`, including untracked files.
5. ~~**Permission model.**~~ Decided: agents get full permissions inside a Docker sandbox, so there are no approval prompts in the MVP. Docker is a hard dependency. The project is bind-mounted into the container, run with the host uid/gid, with the API key sent to the runner over stdin and open network access for now. The container has no git credentials and `.git` is mounted read-only (so hooks and config cannot be tampered with). Commit and push are strictly user-initiated, via buttons in the UI, and the user writes the commit message. Approval prompts, an allowlist, restricted networking and a host-side API proxy are later work.
6. ~~**Platform priority.**~~ Decided: Linux first.
7. ~~**Name.**~~ Decided: **vettr** (pronounced "vetter"). It replaced the working name "agentide", which clashed with two existing AgentIDE projects (a Rust MCP CLI and a Homebrew-distributed agent IDE). Also rejected after screening: Deltai (crowded with "Delta AI" companies), Margin (an existing agent review tool of that name), and Hunkr, Diffra, Revly, Tessel, Quire, Glossa (too close to existing products). A trademark search for vettr is still open; one unverified concern is a Canadian stock-research company with a similar name.
