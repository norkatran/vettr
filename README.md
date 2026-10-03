# agentide

Agent-first review IDE (pronounced like "agentic"). See [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md) for the concept, decisions and build order.

Electron + TypeScript + React, built with [electron-vite](https://electron-vite.org).

## Layout

- `src/main` - main process (filesystem, git, agent processes)
- `src/preload` - context-isolated bridge exposing `window.agentide`
- `src/renderer` - React UI
- `src/runner` - the runner that executes inside the sandbox container and drives the Agent SDK (bundled by esbuild, not part of the Electron app)
- `src/shared` - types shared across processes (typed IPC contract, agent protocol)
- `sandbox` - Dockerfile for the agent sandbox image

## Scripts

- `npm run dev` - run with hot reload
- `npm run build` - production build into `out/`
- `npm start` - preview the production build
- `npm run typecheck` - type-check main/preload and renderer
- `npm run build:sandbox` - bundle the runner and build the `agentide-sandbox` Docker image (needs Docker)
- `npm run lint` - lint with [Biome](https://biomejs.dev)
- `npm test` - run the [Vitest](https://vitest.dev) suite (`*.test.ts` files under `src/`)
- `npm run test:coverage` - run the suite with V8 coverage; fails unless every logic file is at 100% (see `vitest.config.ts` for the excluded glue)
- `npm run format` - format with Biome (`npm run format:check` verifies formatting and lint without writing)

## CI

A Forgejo Actions workflow (`.forgejo/workflows/ci.yml`) runs on pushes to `main` and on pull requests: install, typecheck, lint, format check, test (with the 100% coverage gate) and build. There are no Docker images or packages to publish.

If the Electron binary is missing after `npm install` (install scripts disabled), run `node node_modules/electron/install.js`.

## MVP to-do list

Derived from the build order in [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md). Each milestone should be usable on its own; tick items off as they land.

### 0. Decisions to settle first (brief section 8)

- [x] First agent: Claude Code via the Claude Agent SDK, running inside the sandbox container behind the adapter interface (main process talks to a runner in the container over stdio)
- [x] Single agent for the MVP, with multi-agent and worktrees deferred
- [x] Comments on manually edited lines are marked outdated (collapsed, not re-anchored by guessing)
- [x] Diff baseline is the working tree against `HEAD`, including untracked files
- [x] Permission model: agents get full permissions inside a Docker sandbox, so there are no approval prompts in the MVP. Docker is a hard dependency
- [x] First OS: Linux
- [x] Sandbox: project is bind-mounted into the container, run with the host uid/gid, API key sent to the runner over stdin (never an env var, `--env-file` or file, so it is not visible in `docker inspect`) and stored on the host with Electron `safeStorage`, open network access for now
- [x] Commit and push are strictly user-initiated (buttons in the UI, user-typed commit message). The container has no git credentials, and `.git` is mounted read-only so the agent cannot change history, hooks or config (which would otherwise run on the host with the user's credentials)

### 1. Shell

- [x] Electron + TypeScript + React scaffold with typed IPC
- [x] Open a project folder via a native picker, from File > Open Project (Ctrl+O) in the application menu
- [x] Remember the current project and recent projects across restarts (File > Recent Projects, last 10)
- [x] Verify the chosen folder is a git repository (a subfolder resolves to the repo root); otherwise show an error and do not open it
- [x] Prompt input as the empty state of the Session view (Ctrl+Enter or Start submits; disabled until a project is open; submission is a stub until the agent adapter exists)
- [x] VS Code-style sidebar: activity bar (Session, Changes) plus a collapsible side panel (click the active icon or Ctrl+B to toggle). Session and Changes are placeholders until those surfaces exist
- [x] Status bar: repository name, branch (or short SHA when detached), `↓behind ↑ahead` against the upstream (hidden when zero, "no upstream" when none is set), and changed-file count. Refreshes on window focus
- [x] Light and dark themes via CSS variables: follows the OS by default, with a status bar toggle that remembers the choice
- [x] Navigation between the Session and Changes surfaces: the activity bar switches views, and a "New session" button in the Session side panel returns to the empty prompt (the real Session and Changes content lands in milestones 2 and 3)

### 2. Changes view

- [x] Run `git diff` of the working tree against `HEAD` from the main process (untracked files included, via a throwaway copy of the index)
- [x] Changed-file list with status (added, modified, deleted, renamed)
- [x] Read-only diff rendering with unified and split modes (plain table rendering; no syntax highlighting yet)
- [x] Handle binary files, renames and very large diffs gracefully (files over 5000 changed lines are listed with counts but not rendered)
- [x] Single button to open the Changes view from anywhere in a session (titlebar button with the changed-file count)
- [x] Notification-style blue badge on the Changes activity bar icon with the changed-file count (`99+` above 99, hidden when zero)
- [x] Watch the working tree with chokidar and refresh on change (debounced; the status bar refreshes too)

### 3. Agent session

- [x] Define the agent adapter interface (`start`, `send`, `interrupt`, event stream; keep `respondToApproval` in the interface for later), plus the JSON-lines protocol shared with the runner (`src/shared/agent.ts`)
- [x] Build the sandbox image (Node, Claude Agent SDK, runner script that speaks JSON lines over stdio): `sandbox/Dockerfile` and `src/runner`, built with `npm run build:sandbox`
- [x] Start and stop the container from the main process (`src/main/sandbox.ts`, `src/shared/sandbox.ts`): bind-mount the project, host uid/gid, check Docker and the image are available and report clearly if not
- [x] Credential: an API key or a Claude OAuth token (`claude setup-token`) entered in the Session view; API keys are checked against the API when saved, stored with Electron `safeStorage` and sent to the runner over stdin in `init`
- [x] Implement the first adapter in the main process, behind the interface, driving the in-container runner (`src/main/claudeAdapter.ts`)
- [x] Prevent the agent from committing, pushing or tampering with git: mount `.git` read-only (so no commits, and no edits to hooks or config that would later run on the host with the user's credentials), no git credentials in the container, and handle worktree/submodule layouts where `.git` is a file or lives elsewhere
- [x] Stream agent events to the renderer over typed IPC (`agent:event`, plus start, send, interrupt and stop calls on `window.agentide`)
- [x] Session view: streamed assistant text
- [x] Session view: tool calls (started and finished) and file edits
- [x] Interrupt the current turn (Stop button; see the real-API check below)
- [x] Send follow-up messages in the same session
- [x] Surface errors and agent exit clearly
- [x] Verify a full turn against the real API with a real credential (confirmed working in the real app with an OAuth token)

### 4. Commit and push

Brought forward ahead of line comments (see brief section 6.1). File-level staging only; no hunk staging.

- [x] Stage and unstage whole files from the Changes view (per-file and "all" buttons in the diff, plus a `+`/`−` button beside each file in the sidebar list; `git add --all` / `git reset` with literal pathspecs, errors shown inline until the notification system lands)
- [x] Split the Changes view into collapsible Staged and Unstaged accordions (a partially staged file appears in both)
- [x] Commit message input and a commit button, at the top of the Changes side panel (Ctrl+Enter commits; the user writes the message; the agent never commits; errors shown inline for now)
- [x] While committing, grey out the message input and show a spinner in the button beside "Committing"
- [x] The status bar's branch and `↑ahead`/`↓behind` area is itself the push control (clickable only when there are commits to push): runs `git push` to the branch's upstream using the host's installed `git`, credentials and config (the agent never pushes); prompting is disabled, so it fails if any input is needed. Failures show inline in the status bar until the notification system lands
- [x] Same busy state (disabled with a spinner, "Pushing") while pushing
- [ ] Notification system for git errors (hooks, auth, rejected pushes, nothing staged), showing git's output and keeping the typed commit message
- [ ] Decide how to handle push when no upstream is set (`push -u`)

### 5. Line comments

- [ ] Add a comment to a line or a range, on either side of the diff
- [ ] Store each comment with file, side, line range, snapshot of the lines and review round
- [ ] Batch comments and send them to the agent as a structured message (file, line range, quoted code, comment text)
- [ ] Re-anchor comments by snapshot text when a new round arrives
- [ ] Mark comments that no longer match as outdated and show them collapsed
- [ ] Round-to-round diff showing what the agent changed in response
- [ ] Edit and delete comments before sending

### 6. External editor

- [ ] Setting for a command template (for example `code -g {file}:{line}`) with presets and a custom option
- [ ] "Open in editor" from the diff, landing on the exact line
- [ ] Detect files changed outside the app and note them in the next message to the agent
- [ ] Mark comments on externally edited lines as outdated

### MVP release checklist

- [ ] End-to-end run of the core flow: prompt, watch, review, comment, repeat, commit, push
- [ ] Linting, formatting and automated tests for the main process and shared logic (Biome and Vitest are set up and run in CI, with a 100% per-file coverage gate on logic modules; new logic needs full tests as it lands)
- [ ] Out-of-scope guardrails respected (no editor, language server, debugger, extensions or primary file tree)
- [ ] Build and smoke test on the first target OS

### Later

- [ ] Interactive git authentication for push (passphrase and credential prompts)
- [ ] Hunk-level staging
- [ ] Checkpoints and undo per agent turn (the session model can take them additively: see brief section 5.4)
- [ ] Approval prompts and a user-extendable allowlist for shell commands
- [ ] Restricted network access for the sandbox container
- [ ] Host-side API proxy that injects the key, so the container only gets a placeholder token and `ANTHROPIC_BASE_URL` (verify the SDK honours the base URL override; bind the proxy to the Docker bridge only)
- [ ] Per-project sandbox image override
- [ ] Multiple parallel agents using git worktrees
- [ ] Additional agent adapters
- [ ] User-customisable or importable themes (beyond the built-in light and dark)
- [ ] Packaging, code signing and auto-update
- [ ] Windows and Linux (or macOS) coverage beyond the first target OS
