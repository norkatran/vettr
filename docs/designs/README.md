# Design documents

Every feature that adds something new to vettr, or improves an existing part, gets a design document in this folder before or as the work starts.

- **Name:** `NNNN-short-title.md`, numbered in sequence (the next free number).
- **Contents:** status (`Proposed`, `In progress` or `Fulfilled`), the problem, the decision and approach, scope and out of scope, and a to-do list ticked off as work lands.
- **Lifecycle:** when the work is complete, mark the document `Fulfilled`. Do not delete it: fulfilled designs stay here as context for future agent runs.
- **Relationship to the brief:** [../PROJECT_BRIEF.md](../PROJECT_BRIEF.md) describes the current state of the system. When a design lands, fold its decisions into the brief.

## Index

| # | Design | Status |
| --- | --- | --- |
| 0001 | [MVP](0001-mvp.md) | Fulfilled (release checklist open) |
| 0002 | [Agent lifecycle and readiness](0002-agent-lifecycle.md) | Fulfilled |
| 0003 | [Slash command discovery](0003-slash-commands.md) | Fulfilled |
| 0004 | [Structured review comments and agent replies](0004-structured-review.md) | Fulfilled |
| 0005 | [File watcher respects .gitignore](0005-watcher-respects-gitignore.md) | Fulfilled |
| 0006 | [Credential profiles](0006-credential-profiles.md) | Fulfilled |
