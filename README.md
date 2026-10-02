# agentide

Agent-first review IDE (pronounced like "agentic"). See [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md) for the concept, decisions and build order.

Electron + TypeScript + React, built with [electron-vite](https://electron-vite.org).

## Layout

- `src/main` - main process (filesystem, git, agent processes)
- `src/preload` - context-isolated bridge exposing `window.agentide`
- `src/renderer` - React UI
- `src/shared` - types shared across processes (typed IPC contract)

## Scripts

- `npm run dev` - run with hot reload
- `npm run build` - production build into `out/`
- `npm start` - preview the production build
- `npm run typecheck` - type-check main/preload and renderer

If the Electron binary is missing after `npm install` (install scripts disabled), run `node node_modules/electron/install.js`.

## MVP to-do list

Derived from the build order in [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md). Each milestone should be usable on its own; tick items off as they land.

### 0. Decisions to settle first (brief section 8)

- [ ] Choose the first agent to wrap (suggested: Claude Code via the Claude Agent SDK)
- [ ] Confirm single agent for the MVP, with multi-agent and worktrees deferred
- [ ] Confirm comments on manually edited lines are marked outdated
- [ ] Confirm the diff baseline is working tree against `HEAD`
- [ ] Decide the default permission model for agent shell commands
- [ ] Pick the first OS to build and test on

### 1. Shell

- [x] Electron + TypeScript + React scaffold with typed IPC
- [x] Open a project folder via a native picker
- [ ] Remember the current project and recent projects across restarts
- [ ] Verify the chosen folder is a git repository, and handle the case where it is not
- [ ] Home screen with a prompt input
- [ ] Basic VS Code / Atom-style layout (title bar, status bar, main surface, theme)
- [ ] Navigation between the Home, Session and Changes surfaces

### 2. Agent session

- [ ] Define the agent adapter interface (`start`, `send`, `interrupt`, `respondToApproval`, event stream)
- [ ] Implement the first adapter in the main process, behind the interface
- [ ] Stream agent events to the renderer over typed IPC
- [ ] Session view: streamed assistant text
- [ ] Session view: tool calls (started and finished) and file edits
- [ ] Approval prompts for shell commands, with a user-extendable allowlist
- [ ] Interrupt the current turn
- [ ] Send follow-up messages in the same session
- [ ] Surface errors and agent exit clearly
- [ ] Keep the session model open to per-turn checkpoints later

### 3. Changes view

- [ ] Run `git diff` of the working tree against `HEAD` from the main process
- [ ] Changed-file list with status (added, modified, deleted, renamed)
- [ ] Read-only diff rendering with unified and split modes
- [ ] Handle binary files, renames and very large diffs gracefully
- [ ] Single button to open the Changes view from anywhere in a session
- [ ] Watch the working tree (for example with chokidar) and refresh on change

### 4. Line comments

- [ ] Add a comment to a line or a range, on either side of the diff
- [ ] Store each comment with file, side, line range, snapshot of the lines and review round
- [ ] Batch comments and send them to the agent as a structured message (file, line range, quoted code, comment text)
- [ ] Re-anchor comments by snapshot text when a new round arrives
- [ ] Mark comments that no longer match as outdated and show them collapsed
- [ ] Round-to-round diff showing what the agent changed in response
- [ ] Edit and delete comments before sending

### 5. Commit and push

- [ ] Stage and unstage files (and ideally hunks) from the Changes view
- [ ] Commit message input and commit
- [ ] Push using the user's installed `git`, credentials and config
- [ ] Show git errors (hooks, auth, rejected pushes) in the UI

### 6. External editor

- [ ] Setting for a command template (for example `code -g {file}:{line}`) with presets and a custom option
- [ ] "Open in editor" from the diff, landing on the exact line
- [ ] Detect files changed outside the app and note them in the next message to the agent
- [ ] Mark comments on externally edited lines as outdated

### MVP release checklist

- [ ] End-to-end run of the core flow: prompt, watch, review, comment, repeat, commit, push
- [ ] Linting, formatting and automated tests for the main process and shared logic
- [ ] Out-of-scope guardrails respected (no editor, language server, debugger, extensions or primary file tree)
- [ ] Build and smoke test on the first target OS

### Later

- [ ] Checkpoints and undo per agent turn
- [ ] Multiple parallel agents using git worktrees
- [ ] Additional agent adapters
- [ ] Packaging, code signing and auto-update
- [ ] Windows and Linux (or macOS) coverage beyond the first target OS
