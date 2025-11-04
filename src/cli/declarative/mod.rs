//! Declarative CLI Framework
//!
//! This module provides a zero-code CLI generation system that automatically
//! routes commands to ABK adapter functions based on TOML/YAML/JSON configuration.
//!
//! ## Philosophy
//!
//! Instead of writing CLI handler code, you write a configuration file that
//! declaratively describes your CLI structure. The framework automatically:
//! - Builds the clap CLI from config
//! - Routes commands to ABK adapter functions
//! - Auto-instantiates adapters from config
//!
//! ## Usage
//!
//! ```rust,no_run
//! use abk::cli::declarative::DeclarativeCli;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     DeclarativeCli::from_file("config/cli.toml")?.execute().await?;
//!     Ok(())
//! }
//! ```
//!
//! That's it. ONE line of code. Everything else is in the config file.

pub mod config;
pub mod builder;
pub mod router;
pub mod executor;
pub mod adapters;
pub mod error;

// Re-export main types
pub use config::{CliConfig, CommandConfig, ArgumentConfig, AppConfig, AdapterConfig, AgentCliConfig};
pub use builder::CliBuilder;
pub use router::{CommandRouter, CommandHandler, ExecutionContext, AdapterRegistry};
pub use executor::DeclarativeCli;
pub use adapters::AdapterFactory;
pub use error::{DeclarativeError, DeclarativeResult};
