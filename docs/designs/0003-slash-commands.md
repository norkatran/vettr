# 0003: Slash command discovery

Status: Fulfilled

## Problem

The agent is prewarmed and the composer is disabled until the sandbox is ready, but the user has no way to see which slash commands (built-ins, skills, project and plugin commands) the agent offers.

## Decision

- The runner asks the idle SDK query for `supportedCommands()` right after creating it (it resolves before any prompt) and emits a `commands` event with the full list.
- The SDK pushes the whole list again as a `commands_changed` system message when it changes mid-session (for example skills discovered in a subdirectory). The `Translator` turns it into the same `commands` event. Clients replace their list, never merge.
- Only `name`, `description`, `argumentHint` and `aliases` cross the protocol (`SlashCommandInfo`).
- `AgentManager` caches the latest list (cleared when the agent exits) and serves it through IPC `agent:commands`, so a renderer that loads after the event still gets it. The renderer also follows `commands` and `exited` events (`useSlashCommands`).
- The composer shows a menu above the textarea while the text is `/` followed by non-space characters (`slashQuery`). Matching is prefix first, then substring, on names and aliases (`filterCommands`). Arrow keys move, Tab or Enter completes to `/name `, Escape dismisses, and clicking works. Ctrl+Enter still sends.

## Out of scope

Running commands through any UI other than typing them; commands in the palette; showing arguments help beyond the hint.

## To do

- [x] `commands` event, runner query and `commands_changed` translation
- [x] Main-process cache and IPC
- [x] Composer menu
- [x] Tests for filtering and translation
- [x] Update the brief
