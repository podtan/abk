//! Adapter traits for CLI commands
//!
//! These traits define minimal, stable interfaces between ABK CLI commands
//! and host applications. Implementing these adapters allows your agent to
//! use ABK's CLI commands without tight coupling.

pub mod context;
pub mod checkpoint;
pub mod provider;
pub mod tools;

pub use context::CommandContext;
pub use checkpoint::{
    CheckpointAccess,
    RestorationAccess,
    ProjectMetadata,
    SessionMetadata,
    SessionStatus,
    CheckpointMetadata,
    CheckpointData,
    CheckpointDiff,
    RestoredCheckpoint,
    AgentResult,
    ResumeContext,
    AgentStateData,
    ConversationStateData,
    FileSystemStateData,
    ToolStateData,
    RestorationMetadata,
};
pub use provider::{ProviderFactory, ProviderInfo, ProviderConfig};
pub use tools::{ToolRegistryAdapter, ToolInfo, ToolExecutionResult};
