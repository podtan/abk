//! CLI feature - Reusable command-line interface implementations
//!
//! This module provides generic CLI command implementations that can be used
//! across different agent projects via adapter traits.
//!
//! # Features
//!
//! - **Adapter-based**: Commands operate through small, stable traits
//! - **Feature-gated**: Commands requiring specific features use cargo features
//! - **Testable**: Full unit test coverage with mock implementations
//!
//! # Architecture
//!
//! Commands interact with the host application through adapter traits:
//! - `CommandContext` - Access to config, logging, filesystem
//! - `CheckpointAccess` - Checkpoint and session management
//! - `ProviderFactory` - LLM provider creation
//! - `ToolRegistryAdapter` - Tool registry operations
//!
//! # Example
//!
//! ```rust,ignore
//! use abk::cli::{CommandContext, commands};
//!
//! // Implement adapter for your agent
//! struct MyContext { /* ... */ }
//! impl CommandContext for MyContext { /* ... */ }
//!
//! // Use ABK commands
//! let ctx = MyContext::new();
//! commands::init::run(&ctx, InitOpts::default()).await?;
//! ```

#[cfg(feature = "cli")]
pub mod error;

#[cfg(feature = "cli")]
pub mod adapters;

#[cfg(feature = "cli")]
pub mod commands;

#[cfg(feature = "cli")]
pub mod config;

#[cfg(feature = "cli")]
pub mod runner;

#[cfg(feature = "cli")]
pub mod utils;

#[cfg(all(feature = "cli", test))]
pub mod test_utils;

// Re-exports for convenience
#[cfg(feature = "cli")]
pub use error::{CliError, CliResult};

#[cfg(feature = "cli")]
pub use adapters::{CommandContext, CheckpointAccess, ProviderFactory, ToolRegistryAdapter};

#[cfg(feature = "cli")]
pub use config::*;

#[cfg(feature = "cli")]
pub use runner::{run_configured_cli, run_configured_cli_from_config, run_with_raw_config, DefaultCommandContext, RawConfigCommandContext};

#[cfg(feature = "cli")]
pub use utils::*;
