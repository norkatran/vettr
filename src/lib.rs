//! vettr: agent-first review IDE (egui). See `docs/designs/0007-rust-egui-rewrite.md`.

// Protocol and pure logic (ports of `src/shared/*.ts`).
pub mod agent;
pub mod comments;
pub mod credential;
pub mod diff;
pub mod editor;
pub mod fuzzy;
pub mod git_actions;
pub mod highlight;
pub mod markdown;
pub mod profiles;
pub mod projects;
pub mod readiness;
pub mod replies;
pub mod repo_status;
pub mod resolution;
pub mod review_send;
pub mod sandbox;
pub mod session;
pub mod sessions;
pub mod settings;
pub mod slash_commands;
pub mod theme;

// Host services (ports of `src/main/*.ts`).
pub mod host;

// Glue between the host services and the UI (replaces `src/main/index.ts` and the IPC layer).
pub mod backend;
pub mod task;

// egui user interface (ports of `src/renderer/src/*.tsx`).
pub mod ui;
