# Agent-first review IDE: project brief

Project name: **agentide** (pronounced the same way as "agentic"). This brief captures the concept, the decisions made so far, the known hard problems, and a suggested build order. It is written to be handed to a coding agent as starting context.

Items marked **Decided** came from the product owner. Items marked **Suggested** are recommendations that have not been confirmed and can be changed.

## 1. Concept

A desktop app (Electron) for working on a code repository through agents rather than through an editor. The closest mental model is **GitHub Desktop plus an agent**: it shows diffs, commits, and pushes, and it hands off to a real editor for any manual editing.

The user never browses a file tree to see what happened. They prompt, watch the agent work, review a pull-request-style diff, comment on lines, send the comments back to the agent, and commit when satisfied.

## 2. Core user flow

1. **Open a project.** The app shows a home screen with a prompt input.
2. **Enter requirements.** The prompt is passed to an agent.
3. **Watch the agent work.** The session view looks like any coding harness: streamed messages, tool calls, file edits, and approval prompts. The user stays in the loop.
4. **Open the Changes view.** A single diff/changes button shows everything that changed, laid out like a merge/pull request comparison on a git host.
5. **Comment on specific lines.** Comments are batched and sent to the agent(s) to fix.
6. **Repeat** steps 3 to 5 until the changes are right.
7. **Commit and push.** Stage the files, write a message, commit, push, as usual.

## 3. Decisions

| Topic | Status | Decision |
|---|---|---|
| Platform | Decided | Electron desktop app, not a browser tab. You select the app and you're in it. |
| Language | Decided | TypeScript throughout (strongly preferred). |
| Look and feel | Decided | VS Code / Atom-style UI. |
| Paradigm | Decided | Agent-first. The prompt home screen is the entry point. |
| Navigating changes | Decided | A diff/changes view replaces the sidebar file tree as the main way to see what happened. |
| Review loop | Decided | Line-level comments on the diff, sent back to the agent(s). |
| Manual editing | Decided | Out of scope. "Edit manually" opens the file in the user's own editor/IDE, which is user-configurable. |
| Git | Decided | Stage, commit, and push from inside the app. |
| Agent implementation | Suggested | Wrap an existing agent SDK or CLI rather than writing a harness. See open questions. |

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
- **UI:** React, or whichever framework the builder prefers. Three main surfaces: Home (prompt), Session (agent activity), Changes (diff, comments, commit).
- **Agent:** Wrap an existing agent and render its event stream. For example, Claude Code through the Claude Agent SDK. Keep the agent behind an adapter interface so that other agents can be added later.
- **Diff rendering:** Monaco's diff editor in read-only mode, or a dedicated diff library, fed by `git diff`. Support both unified and split views.
- **Git:** Shell out to the user's installed `git`. Their existing credentials, SSH keys, and config then work with no extra setup.
- **File watching:** Watch the working tree (for example with chokidar) so the Changes view refreshes when files change on disk.
- **Multiple agents:** One git worktree per agent, so parallel agents cannot overwrite each other.

### Agent adapter interface (sketch)

The UI should depend only on a small interface, roughly:

- `start(prompt, cwd)` begins a session
- `send(message)` sends a follow-up, including batched review comments
- `interrupt()` stops the current turn
- An event stream covering: assistant text, tool call started/finished, file edited, approval requested, turn finished, error
- `respondToApproval(id, allow)`

## 5. Hard problems and how to approach them

### 5.1 Comment anchoring

Once the agent rewrites a file, the lines that comments point at move or disappear. Git hosts handle this by marking comments "outdated".

Suggested approach:

- Store each comment with: file path, diff side (old/new), line range, a snapshot of the commented lines, and the review round it was made in.
- When a new round arrives, try to re-anchor by matching the snapshot text. If it no longer matches, mark the comment **outdated** and keep it visible in a collapsed state.
- Show a round-to-round diff so the user can see what the agent changed in response.

### 5.2 Manual edits made in the external editor

Because editing happens outside the app, three things are needed:

- **Refresh:** The Changes view must update when files change on disk.
- **Tell the agent:** If the user hand-edits a file mid-session, the agent's view of it is stale and it may overwrite the change. The next message to the agent should include a note listing files changed externally.
- **Comments:** Suggested rule: treat a manual edit like an agent revision, and mark comments on affected lines as outdated.

### 5.3 Open in external editor

- Configurable command template, for example `code -g {file}:{line}`.
- Clicking from the diff should land on the exact line.
- Offer presets for common editors plus a custom command.

### 5.4 Checkpoints and undo

Users need to roll back a bad agent turn without losing their own uncommitted work. Options include a snapshot per turn (for example a hidden ref or stash-like commit) that can be restored. Not needed for the first milestone, but the session model should leave room for it.

### 5.5 Command safety

Decide what the agent can run without asking. At minimum: an approval prompt for shell commands, with an allowlist the user can extend. Whatever agent is wrapped will likely have its own permission model; surface it rather than reinventing it.

### 5.6 Diff baseline

"What has changed" needs a defined baseline. Suggested default: working tree against `HEAD`, since that matches what will be committed. A possible later addition is "changes since this session started" or "changes since the last review round".

### 5.7 Shipping

Code signing, auto-update, cross-platform quirks, and performance on very large diffs (plus binary files and renames) are the long tail. Defer until the core loop works.

## 6. Suggested build order

Each milestone should be usable on its own.

1. **Shell.** Electron app, open a project folder, home screen with a prompt input, basic VS Code-style layout.
2. **Agent session.** Wrap one agent behind the adapter. Stream its output, show tool calls and file edits, handle approvals and interrupt.
3. **Changes view.** File list plus diff of working tree against `HEAD`. Unified and split views.
4. **Line comments.** Add comments on lines or ranges, batch them, send to the agent as a structured message (file, line range, quoted code, comment text). Handle a second round with outdated-comment logic.
5. **Commit and push.** Stage files, commit message, commit, push.
6. **External editor.** "Open in editor" from the diff with jump-to-line, file watching, and the note to the agent about external changes.

Later:

- Checkpoints and undo per agent turn
- Multiple parallel agents using worktrees
- Additional agent adapters
- Packaging, signing, and auto-update

Rough effort for a solo experienced developer: a prototype in 1 to 2 weeks, a usable MVP in 4 to 8 weeks, a polished product in 6 months or more.

## 7. Competitive landscape

Researched on 2 October 2026. This space moves quickly, so recheck before relying on it.

### Agent-first apps with diff review (closest)

- **Conductor** (Melty Labs): Mac-only app that runs Claude Code and Codex in isolated git worktrees, with in-app diff review and PR handoff. Can open a workspace in an external IDE for editing. https://conductor.build
- **Warp:** Code review panel with inline comments on agent diffs, batched and sent back to the agent. https://docs.warp.dev/agent-platform/local-agents/interactive-code-review/
- **Codex app** (OpenAI): Agent available as an app, CLI, editor integrations, and cloud environments.

### Small review-loop tools

These implement the line-comment loop as a standalone local web UI.

- **diffx:** https://github.com/wong2/diffx
- **CodeChat:** https://github.com/alexmx/codechat
- **Crit:** shows a diff between review rounds. https://sharedcontext.ai/plugins/external/tomasz-tomczyk/crit

### Full AI IDEs

These are what this project deliberately simplifies away from.

- **Cursor:** VS Code fork with an agent mode.
- **Devin Desktop** (formerly Windsurf): full IDE with agents and review tools.
- **GitHub Copilot:** agent mode with reviewable diffs.

### Agents to wrap rather than compete with

Claude Code, Codex CLI, OpenCode.

### Where this project can differ

- **Cross-platform.** Conductor is Mac only; Electron gives Windows and Linux.
- **Review experience.** The comment-on-diff loop exists in several tools, so it needs to be noticeably better here: comment anchoring, round-to-round diffs, and a fast path from comment to fix.
- **A native app, not a browser tab.** The small review tools all run as a local server plus a browser tab.
- **Strict scope.** No editor, by design.

## 8. Open questions

1. **Which agent to wrap first?** And should the adapter support more than one from the start?
2. **Multi-agent in the MVP, or later?** The brief mentions "agent(s)". Worktrees add real complexity.
3. **Comment rule after manual edits.** Confirm "mark as outdated".
4. **Diff baseline.** Confirm working tree against `HEAD` as the default.
5. **Permission model.** How much can the agent run without approval?
6. **Platform priority.** Which OS to build and test on first.
7. ~~**Name.**~~ Decided: **agentide**.
