# vettr

Agent-first review IDE built in Rust with egui (eframe). The agent runner in `runner/` is TypeScript and runs inside a Docker sandbox (`sandbox/`).

## Read these first

- [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md) - concept, decisions, architecture, hard problems and the current state of the system. Read it for context before making design or implementation choices.
- [docs/designs/](docs/designs/README.md) - design documents, one per feature or improvement, including all fulfilled ones. Read the index, and any design relevant to your task, for the history and reasoning behind existing behaviour.
- [README.md](README.md) - public-facing project page (branding, how the app works, screenshots, setup). It is **not** a place for technical decisions, roadmaps or to-do lists.

## Design documents (critical)

Every feature that adds something new or improves an existing system gets a design document in `docs/designs/`, following the process in [docs/designs/README.md](docs/designs/README.md).

- Create `docs/designs/NNNN-short-title.md` (next free number) before or as the work starts, and add it to the index. It records the problem, decision, scope and a to-do list that you tick off as work lands.
- When the work is done, mark the design `Fulfilled`. **Never delete fulfilled designs**: they are kept as context for future agent runs.
- Do not track roadmaps, to-do lists or technical decisions in the README.

## Keeping the docs up to date (critical)

- **Brief:** `docs/PROJECT_BRIEF.md` describes the current state of the system. Whenever a decision is made or changed (architecture, scope, behaviour, tooling), update it in the same piece of work, and fold in the decisions of any design that lands. Do not leave it describing a design that no longer matches the code.
- **README:** update it only when user-visible behaviour, setup steps, scripts or branding change. Keep it a friendly overview, and do not add technical decision records to it.
