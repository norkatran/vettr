//! Host services: git, file watching, sandbox, agent lifecycle and persistent stores.

pub mod agent_manager;
pub mod api_key_check;
pub mod busy_guard;
pub mod claude_adapter;
pub mod git;
pub mod profile_store;
pub mod project_store;
pub mod resolved_store;
pub mod sandbox_image;
pub mod sandbox_runtime;
pub mod secret;
pub mod sessions;
pub mod settings_store;
pub mod transcripts;
pub mod translate;
pub mod watcher;
