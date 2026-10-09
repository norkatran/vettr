# 0002 - Agent lifecycle and readiness

> Note: since design 0007 the app is Rust + egui (it was Electron + TypeScript + React). Outdated references below are ~~struck through~~ and followed by the current equivalent.

**Status:** Fulfilled

## Problem

The sandbox agent (Docker container) is currently started on demand. The user waits for container start (and, on first run, image build) after sending a prompt. Nothing stops the user writing prompts or comments when no Claude key is configured, and there is no consistent behaviour when Docker is missing, the agent crashes, or the project changes.

## Decision

The agent is **prewarmed** as soon as the app can run it, and every input that directs to the agent is gated on a single readiness state owned by the ~~main process~~ Rust host (`src/host/agent_manager.rs`).

### Lifecycle

1. **App launch:** check Docker is available and a Claude key (API key or OAuth token) exists in the app settings. Keys come only from the app settings, as today, with the existing secret storage flow.
2. **Both present:** start (prewarm) the sandbox agent for the loaded project.
3. **Docker missing:** error notification. Agent inputs stay disabled.
4. **No key:** the session prompt input is replaced by the existing key entry input, with a short explanation. Comment threads on the changes page are disabled and no comments can be written or saved. When the key is saved, the agent starts and prewarms.
5. **Key changed:** always restart the agent. **Key removed:** stop the agent. Add a tooltip on the settings key editor stating this. If the agent is working on a turn, ask for confirmation first, as for a project switch (the key is validated before the dialog, so a bad key is reported without one).
6. **Project opened:** if it is the same project, do nothing. Otherwise, if an agent is running a task, show a confirmation dialog warning that work in progress will be lost. On confirm, kill the agent, then once the new project is loaded and the app is ready, run the lifecycle from step 1.
7. **Agent crash after warm:** restart it, resume the session in the new sandbox using the stored session id, and notify the user.

### Readiness state

A single state machine ~~in the main process, pushed to the renderer~~ in the Rust host, pushed to the UI over a channel:

`status`: `idle | starting | ready | stopping | error`
`reason` (when not ready): `no-key | docker-unavailable | building-image | starting | crashed-restarting | offline | error`

- The prompt input, comment threads and any other agent-directed input derive their enabled state **only** from this. Inputs are disabled until the sandbox is running (`ready`).
- The ~~main process~~ host also enforces it: ~~IPC handlers~~ `Backend` methods that send to the agent or save comments reject when not `ready`. UI gating alone is not enough.
- Comments exist to be sent to the agent, so they are gated the same as the prompt. Existing threads stay visible but read-only.
- A half-written comment draft is kept while comments are disabled (it lives in the Changes view, so it is not kept across views or projects; see Per-project state).
- Transitions run ~~in the main process (single threaded)~~ in the host's agent manager. Start and stop are serialised through one queue so a start never overlaps a stop. A start for a superseded project is cancelled or discarded on completion (project generation check).

### Notifications

Image builds: when the sandbox image is missing the manager sets `building-image`, streaming the latest build line as the readiness message, then warms the agent. It needs the bundled runner (`runner/dist/runner.mjs`, from `npm run build:sandbox` in `runner/`; ~~`sandbox/dist/runner.mjs`, from `npm run build:runner`~~) next to the app, so it works from source today; shipping a packaged app needs the runner and Dockerfile bundled as resources (shipping work, not this design).

Extend the notification system with an `info` level alongside `error`, used for e.g. "Building the sandbox image…", "Agent restarted, session resumed". Offline vs auth failures get distinct messages.

### Key validation

API keys are validated on save with a cheap API call; an invalid key is rejected with an error and not stored as usable. OAuth tokens: a spike showed the models endpoint answers a bad bearer token with a distinctive 401 ("OAuth access token is invalid"). Decision: reject a token only on that 401 (message says invalid, expired or revoked), and accept it for any other outcome, because a valid long-lived token is scoped for inference and its response from the models endpoint has not been confirmed. First-use failure handling remains the backstop.

### Orphaned containers

Label containers (e.g. with an app/session label). Sweep stale labelled containers at launch, and stop the agent on `before-quit`.

### Per-project state

**Dropped from this design.** The manager already resets its session id on a project switch, and stored sessions are per project on disk. Keying prompt drafts and comment drafts by project (held in app state so they survive view and project switches) was considered and left out as not belonging to this work. Known gap: the comment draft is local to the Changes view, so it is lost when leaving the view and is not cleared on a project switch. Revisit as its own design if it matters.

## Scope

- Startup checks (Docker, key), prewarm, readiness state machine and ~~renderer~~ UI gating
- Key-save and key-removal hooks, settings tooltip
- Project switch teardown with confirmation, same-project no-op, rapid-switch cancellation
- Crash detection, restart and session resume with notification
- `info` notifications
- Orphan container sweep
- Main-process enforcement of gating

## Out of scope

- Handling key expiry or revocation mid-session (a future goal, not handled today either)
- Multiple key sources (settings only)
- Idle timeouts or opt-out of prewarming (developer tool, overhead is accepted)
- Local-only comments that do not go to the agent (not a product concept)

## Dependent future work

- **Slash-command discovery:** with a warm agent the runner can call `Query.supportedCommands()` so the prompt input can hint at available commands as the user starts typing one. Needs its own design; this design only keeps the warm query available for it.

## To-do

- [x] Review the current agent, session, key storage, notification and project-open code against this design
- [x] Readiness state machine and queue in the ~~main process (`src/main/agentManager.ts`), with IPC push (`getReadiness`, `onReadiness`)~~ Rust host (`src/host/agent_manager.rs`), with readiness events pushed over the `Backend` channel and the generation check
- [x] Adapter `warm` and prompt-only `start` (a matching warm agent is reused; resume or another project restarts it)
- [x] Docker availability check at launch, with error notification
- [x] Key presence check at launch, and prewarm
- [x] `info` notification level (auto-dismisses after 6 s; also used for crash restart messages)
- [x] Image build: the app builds a missing image itself when `runner/dist/runner.mjs` and the SDK version are available (otherwise it keeps the "run `npm run build:sandbox`" message), with live progress in the readiness message and info notifications (~~`src/main/sandboxImage.ts`~~ `src/host/sandbox_image.rs`)
- [x] Replace prompt input with key entry when no key; disable comment threads and saving (existing threads stay visible, drafts are kept, Send is blocked with the reason)
- [x] ~~Main-process enforcement on agent-directed IPC~~ Host enforcement on agent-directed `Backend` calls (start, send and interrupt reject unless `ready`). Comments are ~~renderer-only~~ UI-only state with no backend call of their own, so the only way they reach the agent is through those gated calls; there is nothing further to gate until comments are persisted by the host (its own design)
- [x] Send review only marks comments as sent once the agent has taken the message (`sendReviewRound`), so a failed delivery leaves them pending
- [x] Key save: API key validation (existing), restart agent; key removal (`apikey:clear`): stop agent. The key is managed at the top of the Settings view (change or remove, with a two-step remove); the Session view only shows the entry form when no key is saved
- [x] Tooltips on the key editor's Save, Change and Remove buttons stating that the agent restarts or stops
- [x] Project switch: confirmation when running, teardown, same-project no-op, rapid switch handling
- [-] Per-project keying of drafts and session ids (dropped, see Per-project state)
- [x] Crash detection, restart, session resume and notification
- [x] Orphan container labelling (`vettr.app=1`, `vettr.pid=<owner pid>`), background launch sweep of containers whose owner is gone, stop on quit (via the manager `shutdown`)
- [x] Investigate OAuth token validation: spiked against the real API (a bad token sent as a bearer token gets a 401 "OAuth access token is invalid"); implemented as a narrow check (`checkOAuthToken`) that rejects only that 401 and fails open otherwise. Still unconfirmed with a real valid token
- [x] Tests for state transitions and gating (~~`agentManager.test.ts`, `claudeAdapter.test.ts`, `readiness.test.ts`, `sandbox*.test.ts`, `apiKeyCheck.test.ts`, `reviewSend.test.ts`, `busyGuard.test.ts`; the renderer has no component tests~~ now inline `#[cfg(test)]` modules in the matching Rust files, plus headless egui frame tests in `src/ui/app.rs`)
- [x] Update `docs/PROJECT_BRIEF.md` and mark this design Fulfilled

## Review of the current code (findings)

- **Session and container are one thing today.** `ClaudeAdapter.start(prompt, cwd, resume)` checks Docker, reads the key, starts the container and writes `init` then the first `prompt`. Prewarming needs this split: a `warm(project)` that starts the container and sends `init`, and a `start` that only sends the first prompt.
- **Runner protocol.** The runner creates the SDK `query()` at `init`, as today, so a warm agent is a live, idle `Query`. Verified with a spike (SDK 0.3.288): `supportedCommands()` resolves in about 0.6 s before any prompt is sent, so the warm query can answer such requests (needed by the future slash-command discovery feature). Creating the query only on the first prompt was rejected because it would rule this out. `resume` is fixed when the query is created and is only known when the user opens a stored session, so resuming is a re-init: the runner closes the idle query and creates a new one with `resume` (or the container is restarted). The SDK's alpha `prewarm()` is a possible later alternative, not relied on.
- **Resuming after the agent exits** is done ~~in the renderer (`useAgentSession.send`: `agentStop` then `agentStart(message, id)`)~~ in the UI (stop, then start with the message and session id). It moves into the ~~main process~~ host's manager, which restarts and warms a new container.
- **Project switch** in `activateProject` calls `agent.stop()` unconditionally, with no confirmation, no same-project check and no generation check. `open-project` runs in three places (picker, recents, launch).
- **Key flow.** ~~`hasApiKey`/`setApiKey` IPC~~ `hasApiKey`/`setApiKey` calls exist, and `saveApiKey` validates API keys (not OAuth tokens). Neither restarts or stops the agent. The key form lives in ~~`Session.tsx`~~ `src/ui/api_key_form.rs` and the ~~renderer~~ UI decides `hasKey` itself, so readiness replaces it.
- **Gating is UI only today** (~~`disabled={hasKey !== true}`~~ disabling the inputs when there is no key). Agent send, start and the comment flow are not checked in the ~~main process~~ host. Review comments live in the UI state (~~`useReviewComments`~~ `src/comments.rs`) with no persistence.
- **Notifications** have only the error style (`useNotify(title, detail)`); an `info` level needs a `level` field and styling.
- **Containers** are named `vettr-<uuid8>` but unlabelled, and only the `will-quit` handler stops the agent.

## Implementation order

1. ~~`src/shared/readiness.ts`~~ `src/readiness.rs` (types, `canDirectAgent`, block reason text). **Done.**
2. Adapter `warm` (container, `init`, idle query) and `start` (prompt only), plus re-init for resume.
3. ~~Main-process manager~~ Host manager (state machine, serial queue, project generation) and ~~IPC push~~ channel events.
4. ~~Renderer~~ UI gating, `info` notifications, key form and tooltip.
5. Project switch, crash restart, orphan sweep, then tests and docs.
