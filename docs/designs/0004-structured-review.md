# 0004: Structured review comments and agent replies

Status: Fulfilled

## Problem

Review comments reach the agent as prose (`formatReview`: file, lines, quoted code, text). That has three weaknesses:

- Prose is hard for the agent to parse reliably and hard for vettr to recognise again, so a sent review shows up in the Session page as an ordinary user message.
- Comments are renderer-only, in-memory state. Reloading or resuming a session loses them, and nothing in the transcript lets us rebuild them.
- The agent has no way to answer a specific comment. Replies are free text in the transcript, so they cannot be attached to a thread.

## Decision

### 1. Structured review format

A review round is sent as one XML block instead of prose. It is distinctive enough to stand out to the agent and cannot plausibly be typed by accident.

```
<vettr-review round="2">
  <comment id="c7e1…" file="src/foo.ts" side="new" lines="12-14">
    <code>…snapshot…</code>
    <note>…comment…</note>
  </comment>
</vettr-review>
```

- **Ids:** every comment has a unique id (UUID), assigned when the comment is created and kept for its whole life, including across rounds. A comment re-sent in a later round keeps its id.
- **Attributes:** `file`, `side` (`old` or `new`) and `lines` (`12` or `12-14`) are the anchor. `round` is on the root. An outdated comment (see brief 5.1) carries `outdated="true"` so the agent knows the lines may have moved.
- **Escaping:** `<code>` and `<note>` content must be escaped (or wrapped in CDATA, splitting any `]]>`), and attribute values escaped. The format must round-trip losslessly, since the transcript is the source of truth for rebuilding.
- **Preamble:** a short fixed sentence before the block explains the format and the reply tool (section 2). Any free text the user adds goes outside the block.
- **Rebuilding from the transcript:** the Session page recognises a user message containing a `<vettr-review>` block (`parseReview`, the inverse of `formatReview`, in `src/shared/comments.ts`) and renders it as comment cards, in the same way tool use is rendered from the stream, instead of raw text. The parsed comments are enough to recreate threads, so reviews are no longer lost when a session is reloaded or resumed, even where they cannot be shown on the Changes page (for example the file or lines no longer exist).
- **Changes page:** when resuming, comments parsed from the transcript are rehydrated into the review state and re-anchored by snapshot as today (brief 5.1). Those that no longer match show as outdated. This supersedes "comments stay in memory" (brief 5.8) for sent comments; unsent drafts remain in memory only.

### 2. A tool for replying to comments

vettr provides a small in-process MCP-style tool, for example `respond_to_comment`, with:

| Parameter | Meaning |
| --- | --- |
| `comment_id` | Id of the comment being answered (must match one sent). |
| `message` | The agent's reply. |
| `kind` | Optional: `question` (the agent needs more from the user) or `resolved` (the agent believes the issue is fixed). Defaults to a plain reply. |

- **No-op by design:** the handler does nothing except return a short acknowledgement (and an error for an unknown id so the agent can correct itself). The value is in the call itself: vettr reads `tool_use` for it from the JSON stream, as it does for other tools, and builds the UI from that. No new persistence is needed, and replies survive a reload because they are in the transcript.
- **Rendering:** the Session page always shows the reply as a thread entry under its comment card. The Changes page shows it in the comment's thread when the comment is anchored there.
- **Resolution:** resolving a comment resolves its whole thread (the comment and the agent's replies), as on GitHub or GitLab, and is the user's decision alone. It is stored by the app, not in the transcript, in `<userData>/projects/<name>-<hash>/resolved-comments.json` (a list of comment ids, which are UUIDs and so unique across sessions), served over IPC (`comments:resolved`, `comments:resolve`). Resolved threads collapse in both the Session page and the Changes page, with a Reopen button. Only sent comments can be resolved (a pending draft has no thread), the agent is not told, and sent comments are never re-sent, so resolving does not change what later reviews contain.
- **`kind` is advisory:** `resolved` is displayed as "agent believes this is fixed" and never closes a thread. Only the user resolves a comment or thread. `question` marks the thread as awaiting the user.
- **Registration:** checked against the installed SDK (`@anthropic-ai/claude-agent-sdk` 0.3.288): it exports `createSdkMcpServer` and `tool` and accepts in-process servers in `options.mcpServers`, so no external process or new transport is needed. The runner (`src/runner/index.ts`) creates a server (for example named `vettr`) with the one tool and passes it when creating the query. The agent sees it as `mcp__vettr__respond_to_comment`, and that full name is what the preamble uses and what the translator matches in `tool_use`.
- **Permissions:** the runner uses `bypassPermissions`, so the tool needs no allow-list entry.
- **Dependencies:** `tool` takes a zod schema. zod is already installed as the SDK's peer dependency, but it should be declared directly in `package.json`. The runner bundle keeps both the SDK and zod external (`--external:zod`), so the SDK and the tool share one zod copy; the sandbox image already gets zod from npm installing the SDK's peer dependencies.
- **Id validation:** the runner notes the ids in every review prompt it forwards (`ReplyTracker`, using `parseReview`) and returns an error result for an unknown id. In a resumed session the runner never saw the earlier rounds, so validation is off there.

## Out of scope

- Replies to code areas that are not a previously sent comment.
- Multiple agents; sharing comments between machines. Sent comments and replies live in the transcript; only resolution is stored by the app.

## To do

- [x] Add UUIDs to `ReviewComment`
- [x] `formatReview` emits the XML format with escaping; add `parseReview` and round-trip tests
- [x] Preamble text describing the format (the reply tool is added with the tool)
- [x] Session page renders review messages as comment cards
- [x] Rehydrate sent comments from the transcript when a stored session is opened, then re-anchor
- [x] Confirm the SDK supports in-process MCP servers (it does, 0.3.288)
- [x] Reply tool in the runner (in-process MCP server), with id validation; declare zod in `package.json`
- [x] Render replies in the Session page, under their comment cards and in the flow (derived from the tool call in the transcript, no new event); `question` and `resolved` are labelled
- [x] Show replies in the Changes page threads
- [x] User-only resolution: resolving a comment resolves its thread; kept in a vettr-side per-project file (`resolved-comments.json`), shown collapsed in Session and Changes; sent comments only
- [x] Update the brief (sections 5.1, 5.8 and the review loop) and mark this design Fulfilled
