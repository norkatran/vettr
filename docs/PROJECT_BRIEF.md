# Agent-first review IDE: project brief

Project name: **agentide** (pronounced the same way as "agentic"). This brief captures the concept, the decisions made so far, the known hard problems, and a suggested build order. It is written to be handed to a coding agent as starting context.

Items marked **Decided** came from the product owner. Items marked **Suggested** are recommendations that have not been confirmed and can be changed. The former open questions are now settled; see section 8.

## 1. Concept

A desktop app (Electron) for working on a code repository through agents rather than through an editor. The closest mental model is **GitHub Desktop plus an agent**: it shows diffs, commits, and pushes, and it hands off to a real editor for any manual editing.

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
| Platform | Decided | Electron desktop app, not a browser tab. You select the app and you're in it. |
| Language | Decided | TypeScript throughout (strongly preferred). |
| Look and feel | Decided | VS Code / Atom-style UI. |
| Paradigm | Decided | Agent-first. The prompt (the empty state of the Session view) is the entry point. |
| Navigating changes | Decided | A diff/changes view replaces the sidebar file tree as the main way to see what happened. |
| Sidebar | Decided | VS Code-style activity bar (Session, Changes) with a collapsible side panel (click the active icon or Ctrl+B); the Session panel holds the "New session" button. It is for switching surfaces and surface-specific lists, not a project file tree. |
| Review loop | Decided | Line-level comments on the diff, sent back to the agent(s). |
| Manual editing | Decided | Out of scope. "Edit manually" opens the file in the user's own editor/IDE, which is user-configurable. |
| Git | Decided | Stage, commit, and push from inside the app. |
| Agent implementation | Decided | Wrap the Claude Agent SDK (Claude Code) rather than writing a harness. Keep it behind an adapter interface. |
| Sandbox | Decided | Agents run with full permissions inside a Docker container with the project bind-mounted. Docker is a hard dependency. |
| Commit and push | Decided | Strictly user-initiated via UI buttons; the agent never commits or pushes. The container has no git credentials and `.git` is mounted read-only. |
| First platform | Decided | Linux first. |
| Project persistence | Decided | The last opened project is reopened on launch; opening another project makes it the new default. File > Recent Projects lists the last 10 (most recent first). Stored by the main process in `projects.json` under Electron's `userData` dir; folders that no longer exist are dropped. |
| Non-git folders | Decided | A project must be inside a git repository. The picker result is resolved with `git rev-parse --show-toplevel`, so a subfolder opens its repo root. Anything else is rejected with an error dialog (and dropped from recents); agentide never runs `git init` itself. The same check runs on Recent Projects clicks and on the persisted project at launch. |
| Status bar | Decided | Footer showing repo name, branch (short SHA when detached), `↓behind ↑ahead` against the upstream (zero counts hidden; "no upstream" when none is configured) and changed-file count. The branch and ahead/behind area is clickable to push when there is something to push. Read in the main process with `git status --porcelain=v2 --branch` (using `--no-optional-locks`), refreshed on window focus rather than polling. |
| Themes | Decided | MVP ships light and dark only, defined as CSS variables switched by a `data-theme` attribute. Follows the OS by default; a status bar toggle sets an explicit choice remembered in `localStorage`. User-customisable or importable themes are post-MVP. |

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

- **Shell:** Electron with TypeScript. The main process owns the filesystem, git, and agent processes. The renderer owns the UI and talks to the main process over typed IPC.
- **UI:** React, or whichever framework the builder prefers. Two main surfaces: Session (the prompt when no session is active, then agent activity with a follow-up input; a "New session" action returns to the prompt) and Changes (diff, comments, commit). There is no separate Home screen: with a single agent there is at most one session, so Home would only be the Session empty state. Revisit if parallel agents arrive (a session list in the side panel).
- **Agent:** Claude Code through the Claude Agent SDK, rendering its event stream. The SDK runs inside the sandbox container, in a small runner script. The main process starts the container and exchanges JSON lines with the runner over stdio. Keep this behind an adapter interface so that other agents can be added later.
- **Sandbox:** A Docker container with the project bind-mounted, run with the host uid/gid so files are not root-owned. The API key is stored on the host with Electron `safeStorage` and sent to the runner over stdin, never as an env var, `--env-file` or file, so it does not appear in `docker inspect`. The agent can still read it from the process that holds it, so this is an MVP measure; the planned hardening is a host-side proxy that adds the key to requests, leaving the container with only a placeholder token and `ANTHROPIC_BASE_URL` (to be verified against the SDK). Network access is open for now. The image holds Node, the SDK and the runner, and may later be overridden per project. The container has no git credentials, and the project's `.git` directory is bind-mounted read-only. This stops the agent committing and, importantly, stops it editing hooks or config, which the host's `git` would later run with the user's credentials. Read-only git commands (`git diff`, `git log`) still work; anything that writes (including index refreshes by `git status`) fails inside the container. Layouts where `.git` is a file or sits outside the project (worktrees, submodules) need the real git directory mounted read-only too.
- **Diff rendering:** Decided: the main process runs `git diff` and parses it (`src/shared/diff.ts`) into files, hunks and numbered lines; the renderer draws them as plain tables in unified and split layouts, with all files stacked PR-style and the file list in the Changes side panel. This avoids a heavy Monaco dependency and gives a structure that line comments can anchor to. Syntax highlighting can be added later. The baseline includes untracked files without touching the user's index: everything is staged into a temporary copy of the index (`GIT_INDEX_FILE`) and diffed with `--cached` against `HEAD` (or the empty tree before the first commit), with rename detection on. A file with more than 5000 changed lines keeps its counts but its hunks are dropped, and binary files show a placeholder.
- **Git:** Shell out to the user's installed `git` on the host. Their existing credentials, SSH keys, and config then work with no extra setup. Commit and push happen only when the user presses the button.
- **File watching:** Decided: the main process watches the project with chokidar (`src/main/watcher.ts`), debounced to 250 ms, and sends a `repo:changed` event to the renderer, which reloads the Changes view and status bar. `node_modules` is skipped, and inside `.git` only `HEAD` and `index` are watched so staging, commits and branch switches refresh the view. Window focus still triggers a reload as a fallback. `.gitignore` is not consulted yet, so large ignored build directories are still watched.
- **Multiple agents (later):** One git worktree per agent, so parallel agents cannot overwrite each other. The MVP runs a single agent.

### Sandbox image and runner

Decided: `sandbox/Dockerfile` builds `agentide-sandbox` from `node:22-slim` (glibc, because the SDK ships a native `claude` binary per platform) with `git` and `ripgrep`, the Agent SDK installed at the version pinned in `package.json`, and the bundled runner as the entrypoint. `npm run build:sandbox` bundles `src/runner` with esbuild (SDK kept external) and builds the image. The runner reads commands on stdin and writes events on stdout; stderr (including the SDK's own) is diagnostics only. It uses `bypassPermissions`, loads project settings only (so a project `CLAUDE.md` applies but nothing from the container home) and passes the API key to the SDK process as `ANTHROPIC_API_KEY` (the key is visible to the agent, as noted under Sandbox). The container runs as the host uid/gid with `HOME=/tmp/home`. Translation of SDK messages into app events lives in `src/runner/translate.ts`: a `file-edited` event is emitted when an Edit, MultiEdit, Write or NotebookEdit call finishes without an error. A failed turn yields `error` then `turn-finished`, so the UI always unlocks.

### Container lifecycle

Decided: the container is started by `startSandbox` (`src/main/sandbox.ts`) as `docker run --rm -i --init` and stopped with SIGTERM to the `docker` process, which `--init` forwards. Arguments come from `buildRunArgs` (`src/shared/sandbox.ts`): host uid/gid, all capabilities dropped, `no-new-privileges`, and the project mounted at the same absolute path as on the host, so paths in events (such as `file-edited`) need no translation. Read-only overlays come after the project mount: the git dir, the common dir for linked worktrees, and the `.git` file in worktrees and submodules (so the agent cannot redirect it). Verified against real Docker: the agent can write project files (owned by the host user), but writing hooks fails and `git commit` cannot take the index lock, while `git log` works. `checkDocker` reports a missing daemon or missing image with a message saying how to fix it.

SELinux: on hosts with SELinux enforcing (Fedora, RHEL) the container cannot use bind mounts at all by default. Decided: run with `--security-opt label=disable` rather than relabelling with `:z`, which would change labels on the user's files. Isolation still comes from Docker's namespaces and the dropped capabilities.

The API key is stored by `createApiKeyStore` (`src/main/apiKey.ts`): encrypted with Electron `safeStorage` (injected so it is testable), file mode 0600, and it refuses to save if encryption is unavailable instead of falling back to plain text.

### Claude adapter and IPC

Decided: `ClaudeAdapter` (`src/main/claudeAdapter.ts`) implements the interface over the container. Its dependencies (Docker check, container start and stop, API key) are injected so it is tested against a fake process. `start` claims the single session slot synchronously (so concurrent starts cannot both proceed), checks Docker, reads the key, starts the container and writes `init` then the first prompt. It rejects with a user-facing message when Docker or the image is missing or no key is saved. Container stdout goes through `LineBuffer` and `parseEventLine`; stderr is kept (last 2000 characters) and attached to the error shown if the container dies unexpectedly. When the container exits the adapter emits `exited`, preceded by an `error` only when the exit was unrequested, non-zero and the runner had not already reported one. A fatal SDK error ends the runner process (it reports one `error`, then exits with code 1), so a failed turn ends the session: the user starts a new one. `stop()` is quiet, and a project switch or app quit stops the session.

IPC (`src/shared/ipc.ts`): `agentStart(prompt)`, `agentSend`, `agentInterrupt`, `agentStop`, `onAgentEvent`, `hasApiKey` and `setApiKey`. The start and send calls resolve to `null` or an error message rather than rejecting. `agentStart` uses the main process's current project instead of a path from the renderer, and the key never reaches the renderer after it is saved.

### Session view

Decided: the Session view is a transcript plus one input. Its state is a pure reducer over agent events (`src/shared/session.ts`) held in `useAgentSession` above the views, so switching to Changes and back keeps the transcript, and opening another project resets it. States: idle (the prompt), running (Stop button, "Working..."), waiting (follow-up input) and ended (a banner with New session). Items are the user's messages, assistant text, tool calls (collapsed rows with a one-line summary, expandable to the input and output, output truncated at 5000 characters), file edits (path relative to the project) and errors. A turn the user stopped shows "Interrupted" and hides the error the SDK reports for it. Assistant text is plain text for now (no Markdown rendering). A failed start returns to the prompt with the text kept and the message shown.

Credentials: the Session view accepts either an Anthropic API key or a Claude OAuth token (from `claude setup-token`) in the same field, shown when none is saved and replaceable with "Change API key or token". `src/shared/credential.ts` tells them apart by prefix (`sk-ant-oat` means an OAuth token, anything else an API key) and the runner sets `CLAUDE_CODE_OAUTH_TOKEN` or `ANTHROPIC_API_KEY` accordingly, never both. The `init` command's field is `credential`. The store, IPC calls and file keep their `apiKey` names for now. It is checked against `GET /v1/models` before saving (`src/main/apiKeyCheck.ts`) because Claude Code treats a rejected key as retryable and backs off through 11 attempts over several minutes, which looks like a hang. Only a 401 or 403 rejects it; being offline does not block saving. OAuth tokens are saved unchecked, because the models endpoint is not known to accept them, so a bad token shows up as the same slow retry. Open question: check Anthropic's current terms on using subscription OAuth tokens through the Agent SDK in a third-party app, and keep or remove token support accordingly.

The runner points Claude Code at its own temp directory (`CLAUDE_CODE_TMPDIR`). Mounting the project at its host path makes Docker create the parent directories as root, and Claude Code refuses a root-owned `/tmp/claude-<uid>`, which broke projects under `/tmp`.

### Agent adapter interface (sketch)

The UI should depend only on a small interface, roughly:

- `start(prompt, cwd)` begins a session
- `send(message)` sends a follow-up, including batched review comments
- `interrupt()` stops the current turn
- An event stream covering: assistant text, tool call started/finished, file edited, turn finished, error (and approval requested, once approvals are supported)
- `respondToApproval(id, allow)` is kept in the interface for later; the MVP does not use it as the sandbox grants full permissions
- `stop()` ends the session and releases the container

Decided: the interface and events live in `src/shared/agent.ts`. Events are `text`, `tool-started`, `tool-finished`, `file-edited`, `turn-finished`, `error` and `exited`. The runner protocol is one JSON object per line on stdio: the main process sends `init` (API key and cwd, always first), `prompt` and `interrupt`; the runner replies with events. `LineBuffer` reassembles lines from stream chunks and `parseEventLine` drops anything malformed.

## 5. Hard problems and how to approach them

### 5.1 Comment anchoring

Once the agent rewrites a file, the lines that comments point at move or disappear. Git hosts handle this by marking comments "outdated".

Suggested approach:

- Store each comment with: file path, diff side (old/new), line range, a snapshot of the commented lines, and the review round it was made in.
- When a new round arrives, try to re-anchor by matching the snapshot text. If it no longer matches, mark the comment **outdated** and keep it visible in a collapsed state.
- Show a round-to-round diff so the user can see what the agent changed in response.

**Built so far (round 1):** clicking a line number in the diff opens a comment editor (shift-click selects a range); the comment is stored with file, staged or unstaged diff, side, range, snapshot, text and round (`ReviewComment` in `src/shared/comments.ts`, held in memory by `useReviewComments` and reset when the project changes). Pending comments can be edited or deleted. "Send N comments to agent" formats them with `formatReview` (file, lines, quoted code, comment) and sends them as a follow-up message, so it needs a session waiting for input, or, when no session is live (none started, or the last one ended) and a key is saved, it starts a new one with the review as the first prompt; sent comments stay visible and greyed, and the round advances.

**Re-anchoring (built):** `reanchor` runs whenever the changes reload (agent edits, manual edits, staging), not only when a round is sent, so manual edits are covered too. It looks for the snapshot as a run of consecutive lines with identical text on the comment's side of the file's diff, trying the same staged/unstaged diff first and then the other, and picks the match nearest the old position when there are several. A match moves the comment; no match marks it `outdated` (it keeps its last position, is not drawn on the diff, and is listed in a collapsed "outdated comments" group with its quoted snapshot). No fuzzy matching, by the decision in 8.3. Outdated is not sticky: if the exact text returns (for example the user undoes an edit) the comment anchors again. Outdated comments can still be sent; `formatReview` warns the agent that the line numbers may be stale. Limits: only lines visible in the diff are in the snapshot, so a comment on a line whose context scrolls out of the diff goes outdated; a file rename also goes outdated.

**Round-to-round diff (built):** when a review is sent, the main process records the whole working tree (untracked files included, ignored ones not) as a git tree object, using a throwaway index like the Changes view (`snapshotTree` in `src/main/git.ts`, IPC `repo:snapshot`), before the message reaches the agent. That tree id is the baseline for the round; it is held in memory with the comments and replaced by each later send. When a baseline exists, the Changes toolbar offers "All changes" and "Since last review": the latter diffs the current working tree against the baseline (`getChangesSince`, IPC `repo:since`, refreshed by the same watcher and focus triggers) and is read-only, with no staging buttons or comments. It reflects the user's own edits as well as the agent's, since both are revisions. It is a diff of the tree as a whole, so it ignores staging. The baseline tree is unreferenced, so git could prune it after its two-week grace period; that is fine while comments live only in memory, and persisting them (5.8) would need a hidden ref as in 5.4.

### 5.2 Manual edits made in the external editor

Because editing happens outside the app, the Changes view must update when files change on disk (built: file watcher, plus refresh on window focus).

Decided: the app does **not** track which lines or files were edited externally. Telling the agent about external edits and special-casing comments on externally edited lines were dropped: attributing an edit to the user or the agent is not trivial and the benefit is small. Comments already go outdated through snapshot re-anchoring (5.1), whoever made the edit, and the agent sees the current files on disk when it reads them.

### 5.3 Open in external editor

- Configurable command template, for example `code {project} -g {file}:{line}` (`{project}` makes the editor open the project folder too).
- Clicking from the diff should land on the exact line. Built: each file header in the Changes view has a 3-dot menu (right of Stage/Unstage) whose first item is "Open in editor", landing on the file's first added line (per-line opening may follow). The template is split into argv and `{file}`/`{line}`/`{project}` substituted per argument, then spawned detached without a shell (`buildEditorCommand`, IPC `editor:open`); the path must stay inside the project. Disabled for deleted files; an unset command tells the user to choose one in Settings.
- Offer presets for common editors plus a custom command.
- Decided: the command lives in a general **Settings** view (a Settings item at the bottom of the activity bar, which opens a view in the main area). Settings are stored host-side in `settings.json` under Electron's `userData`, validated on load (`src/shared/settings.ts`, `src/main/settingsStore.ts`, IPC `settings:get`/`settings:set`). The theme and API key keep their existing stores. New preferences should be added there rather than getting their own files.

### 5.4 Checkpoints and undo

Users need to roll back a bad agent turn without losing their own uncommitted work. Options include a snapshot per turn (for example a hidden ref or stash-like commit) that can be restored. Decided: deferred past the MVP; for now the user manages commits and repo state themselves. The session model was reviewed and needs no preparation, because adding checkpoints later is purely additive: a turn number and an optional `checkpoint` ref on user transcript items, plus a host-side snapshot taken in the adapter before each `start` and `send` (no runner protocol change). Prefer a git-based snapshot in a hidden ref, stored per project on the host so it outlives the session (a fatal error ends the session and a project switch resets the transcript). It should not rely on the SDK's own file checkpointing, which would miss Bash-made changes and the user's hand edits.

### 5.5 Command safety

The agent has full permissions, so the Docker sandbox is the safety boundary. Only the project directory is mounted, no git credentials are present, `.git` is read-only, and commit and push stay with the user. Network access is open for now, and the API key is visible to the agent inside the container (see the Sandbox bullet in section 4). Approval prompts with a user-extendable allowlist, restricted networking, and a host-side API proxy are later work.

### 5.6 Diff baseline

"What has changed" needs a defined baseline. Decided: working tree against `HEAD` (including untracked files), since that matches what will be committed. A possible later addition is "changes since this session started" or "changes since the last review round".

### 5.7 Shipping

Code signing, auto-update, cross-platform quirks, and performance on very large diffs (plus binary files and renames) are the long tail. Defer until the core loop works.

Linting and formatting use Biome (one tool for both, config in `biome.json`, matching the existing single-quote, no-semicolon style). Tests use Vitest (config in `vitest.config.ts`, files named `*.test.ts` next to the code); CI enforces 100% statement, branch, function and line coverage per file (`perFile` thresholds, V8 provider) on every logic module. Untested files count as 0% because `coverage.include` lists all of `src`. Thin glue with no extractable logic is excluded: `src/main/index.ts` (Electron bootstrap), `src/preload/index.ts` and `src/renderer/src/**` (React UI). Keep that glue thin and move logic into testable modules; revisit the renderer exclusion when component tests (jsdom and Testing Library) are added. CI runs on Forgejo Actions (`.forgejo/workflows/ci.yml`): install, typecheck, lint, format check, test with coverage and build, on pushes to `main` and on pull requests. As a desktop app there are no Docker images to build, and nothing is published yet, so there is no release job. Packaging and publishing get added with the shipping work.

### 5.8 Session persistence and resume

Decided: deferred past the MVP. The core loop (prompt, watch, review, comment, repeat) works within one live session, and review comments are in memory only, so a restart loses them anyway. The Agent SDK supports resuming (`resume: '<sessionId>'`, `continue`, `forkSession`, `resumeSessionAt`, plus `listSessions` and `getSessionMessages` to read a transcript back), but it cannot work today: the SDK writes transcripts under `~/.claude/projects/` inside the container, and the container runs with `--rm`, so they are deleted with it. To add it later: (1) persist a per-project transcript store on the host (under Electron's `userData`) and mount it as the container's Claude config dir, never the host's real `~/.claude`, which would expose host settings and credentials; (2) have the runner report the SDK `session_id` and have the main process remember it per project; (3) add an optional `resume` field to the `init` command that the runner passes to `query()`; (4) rebuild the transcript in the Session view from `getSessionMessages` or saved events. This would also let a session that ended unexpectedly be resumed with its history, and it pairs naturally with persisting review comments and with checkpoints (5.4).


## 6. Suggested build order

Each milestone should be usable on its own.

1. **Shell.** Electron app, open a project folder, prompt input in the Session view, basic VS Code-style layout.
2. **Changes view.** File list plus diff of working tree against `HEAD`. Unified and split views. Built before the agent session so diffs from an agent run externally (for example Claude Code in a terminal) can be reviewed straight away.
3. **Agent session.** Build the sandbox image and container lifecycle. Wrap the Agent SDK behind the adapter, running in the container. Stream its output, show tool calls and file edits, and handle interrupt.
4. **Commit and push.** Brought forward ahead of line comments because it is small, well defined and completes a usable loop (prompt, watch, review, commit, push). Changes view split into collapsible Staged and Unstaged accordions; file-level stage and unstage only (no hunk staging); commit message input and button; push to `origin`. All user-initiated. See section 6.1.
5. **Line comments.** Add comments on lines or ranges, batch them, send to the agent as a structured message (file, line range, quoted code, comment text). Handle a second round with outdated-comment logic.
6. **External editor.** "Open in editor" from the diff with jump-to-line, and file watching.

### 6.1 Commit and push design

Decided:

- **Staging:** file-level only. Hunk staging is out of scope for now (it needs patch building for `git apply --cached`).
- **Layout:** the Changes view has two accordions, Staged changes and Unstaged changes, so every change is still visible at once with a clear separation of what is where. Each file is listed under the section matching its index state; a partially staged file appears in both.
- **Stage and unstage (built):** `stageFiles` and `unstageFiles` in `src/main/git.ts`, over IPC (`repo:stage`, `repo:unstage`). Staging runs `git add --all -- <paths>` (so deletions and untracked files work) and unstaging runs `git reset -q -- <paths>` (`restore --staged` fails before the first commit). Both use `--literal-pathspecs` so names with glob characters are safe, and a renamed file is moved by passing both its old and new path. They resolve to null or git's message. Each file in the Changes side panel also has a `+` (stage) or `−` (unstage) button beside its name, in addition to the button on its diff header. The Changes view has per-file and per-group ("Stage all" / "Unstage all") buttons inside `<details>` accordions.
- **Commit (built):** a textarea and Commit button at the top of the Changes side panel (`commitStaged` in `src/main/git.ts`, IPC `repo:commit`, runs `git commit -m`). Ctrl+Enter submits, like the prompt input. The message clears only on success; failures appear as notifications. The user types the message and the host `git commit` runs over the staged files only. While it runs, the message input is greyed out and disabled with a spinner beside it.
- **Push:** the status bar branch and `↑ahead`/`↓behind` area, clickable only when ahead of the upstream, which runs `git push` to the branch's upstream using the host `git`, with prompting disabled (`GIT_TERMINAL_PROMPT=0`, no askpass, SSH `BatchMode`). If any input would be required (passphrase, credentials) the push fails and the user is notified. Interactive auth is later work. The area shows the same busy state (disabled with a spinner) while pushing.
- **Errors (built):** any failure (hooks, auth, rejected push, nothing staged) is shown as a dismissable popup in the top-right of the window with git's output (`Notifications.tsx`, `useNotify()`). Popups stay until dismissed. The message input keeps its text so nothing is lost.
- **Publish (built):** when the branch has no upstream, the status bar shows a "Publish Branch" button, as VS Code does. With one remote it runs `git push -u <remote> HEAD` straight away, with several it shows a picker (`origin` first), and with none it shows a notification (`listRemotes` and `publishBranch` in `src/main/git.ts`, IPC `repo:remotes` and `repo:publish`). It is hidden when HEAD is detached. Same no-prompt environment and busy state as push.

Later:

- Interactive git authentication for push
- Hunk-level staging
- Checkpoints and undo per agent turn
- Resume sessions and recover transcripts (needs a persisted transcript store; see 5.8)
- Approval prompts, allowlist and restricted sandbox networking
- Host-side API proxy so the real key never enters the container
- Multiple parallel agents using worktrees
- Additional agent adapters
- Packaging, signing, and auto-update

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

- **Cross-platform.** Conductor is Mac only; Electron can run on Linux (first target), Windows and macOS.
- **Review experience.** The comment-on-diff loop exists in several tools, so it needs to be noticeably better here: comment anchoring, round-to-round diffs, and a fast path from comment to fix.
- **A native app, not a browser tab.** The small review tools all run as a local server plus a browser tab.
- **Strict scope.** No editor, by design.

## 8. Decisions on former open questions

1. ~~**Which agent to wrap first?**~~ Decided: Claude Code via the Claude Agent SDK, running inside the sandbox container. The main process talks to a runner in the container over stdio (JSON lines), behind the adapter interface. Only one adapter in the MVP.
2. ~~**Multi-agent in the MVP?**~~ Decided: single agent. Multi-agent and worktrees are deferred.
3. ~~**Comment rule after manual edits.**~~ Decided: mark as outdated (collapsed, not re-anchored by guessing). No special tracking of external edits: snapshot re-anchoring handles it regardless of who edited.
4. ~~**Diff baseline.**~~ Decided: working tree against `HEAD`, including untracked files.
5. ~~**Permission model.**~~ Decided: agents get full permissions inside a Docker sandbox, so there are no approval prompts in the MVP. Docker is a hard dependency. The project is bind-mounted into the container, run with the host uid/gid, with the API key sent to the runner over stdin and open network access for now. The container has no git credentials and `.git` is mounted read-only (so hooks and config cannot be tampered with). Commit and push are strictly user-initiated, via buttons in the UI, and the user writes the commit message. Approval prompts, an allowlist, restricted networking and a host-side API proxy are later work.
6. ~~**Platform priority.**~~ Decided: Linux first.
7. ~~**Name.**~~ Decided: **agentide**.
