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
- [ ] Rest of the VS Code / Atom-style layout (status bar, theme)
- [ ] Navigation between the Session and Changes surfaces (the activity bar switches views; remaining work is the real Session and Changes content, and a "New session" action that returns to the prompt)

### 2. Agent session

- [ ] Define the agent adapter interface (`start`, `send`, `interrupt`, event stream; keep `respondToApproval` in the interface for later)
- [ ] Build the sandbox image (Node, Claude Agent SDK, runner script that speaks JSON lines over stdio)
- [ ] Start and stop the container from the main process: bind-mount the project, host uid/gid, send the API key to the runner over stdin, check Docker is available and report clearly if not
- [ ] Implement the first adapter in the main process, behind the interface, driving the in-container runner
- [ ] Prevent the agent from committing, pushing or tampering with git: mount `.git` read-only (so no commits, and no edits to hooks or config that would later run on the host with the user's credentials), no git credentials in the container, and handle worktree/submodule layouts where `.git` is a file or lives elsewhere
- [ ] Stream agent events to the renderer over typed IPC
- [ ] Session view: streamed assistant text
- [ ] Session view: tool calls (started and finished) and file edits
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
- [ ] Commit message input and a commit button (the user writes the message; the agent never commits)
- [ ] Push button, using the host's installed `git`, credentials and config (the agent never pushes)
- [ ] Show git errors (hooks, auth, rejected pushes) in the UI

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

- [ ] Checkpoints and undo per agent turn
- [ ] Approval prompts and a user-extendable allowlist for shell commands
- [ ] Restricted network access for the sandbox container
- [ ] Host-side API proxy that injects the key, so the container only gets a placeholder token and `ANTHROPIC_BASE_URL` (verify the SDK honours the base URL override; bind the proxy to the Docker bridge only)
- [ ] Per-project sandbox image override
- [ ] Multiple parallel agents using git worktrees
- [ ] Additional agent adapters
- [ ] Packaging, code signing and auto-update
- [ ] Windows and Linux (or macOS) coverage beyond the first target OS
