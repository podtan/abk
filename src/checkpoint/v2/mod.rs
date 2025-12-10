//! V2 Checkpoint Storage Format
//!
//! This module implements the new split-file checkpoint format (v2.0).
//! Each checkpoint creates multiple focused files instead of one monolithic file:
//!
//! - `{NNN}_metadata.json` - Checkpoint metadata (small, queryable)
//! - `{NNN}_agent.json` - Agent state snapshot
//! - `{NNN}_conversation.json` - Conversation events (UMF types)
//! - `events.jsonl` - Append-only event log (session-level)
//!
//! ## Breaking Change
//!
//! This format is incompatible with v1.0 checkpoints. Old checkpoints should be
//! deleted before upgrading to ABK 0.2.0.

pub mod schemas;
pub mod storage_v2;
pub mod events_log;

pub use schemas::*;
pub use storage_v2::*;
pub use events_log::*;
