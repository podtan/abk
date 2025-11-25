//! Agent Builder Kit (ABK) - Modular utilities for building LLM agents
//!
//! ABK provides a set of feature-gated modules for building LLM-based agents:
//!
//! - **`config`** - Configuration and environment loading
//! - **`observability`** - Structured logging, metrics, and tracing
//! - **`cli`** - CLI display utilities and formatting helpers
//! - **`checkpoint`** - Session persistence and checkpoint management
//!
//! # Features
//!
//! Enable the features you need in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! abk = { version = "0.1", features = ["config"] }
//! # Or enable multiple features:
//! abk = { version = "0.1", features = ["config", "observability"] }
//! # Or enable everything:
//! abk = { version = "0.1", features = ["all"] }
//! ```
//!
//! # Example: Using the config feature
//!
//! ```ignore
//! use abk::config::{ConfigurationLoader, EnvironmentLoader};
//! use std::path::Path;
//!
//! // Load environment variables
//! let env = EnvironmentLoader::new(None);
//!
//! // Load configuration from TOML
//! let config_loader = ConfigurationLoader::new(Some(Path::new("config/agent.toml"))).unwrap();
//! let config = &config_loader.config;
//!
//! // Access configuration
//! println!("Max iterations: {}", config.execution.max_iterations);
//! println!("LLM provider: {:?}", env.llm_provider());
//! ```
//!
//! # Example: Using the observability feature
//!
//! ```ignore
//! use abk::observability::Logger;
//! use std::collections::HashMap;
//!
//! // Create a logger
//! let logger = Logger::new(None, Some("DEBUG")).unwrap();
//!
//! // Log a session start
//! let config = HashMap::new();
//! logger.log_session_start("auto", &config).unwrap();
//!
//! // Log an LLM interaction
//! let messages = vec![];
//! logger.log_llm_interaction(&messages, "Hello, world!", "gpt-4").unwrap();
//! ```
//!
//! # Example: Using the checkpoint feature
//!
//! ```ignore
//! use abk::checkpoint::{get_storage_manager, CheckpointResult};
//! use std::path::Path;
//!
//! async fn example() -> CheckpointResult<()> {
//!     let manager = get_storage_manager()?;
//!     let project_path = Path::new(".");
//!     let project_storage = manager.get_project_storage(project_path).await?;
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]

/// Configuration management (enabled with the `config` feature)
#[cfg(feature = "config")]
pub mod config;

/// Observability utilities (enabled with the `observability` feature)
#[cfg(feature = "observability")]
pub mod observability;

/// CLI display utilities (enabled with the `cli` feature)
#[cfg(feature = "cli")]
pub mod cli;

/// Checkpoint and session management (enabled with the `checkpoint` feature)
#[cfg(feature = "checkpoint")]
pub mod checkpoint;

/// LLM Provider abstraction (enabled with the `provider` feature)
#[cfg(feature = "provider")]
pub mod provider;

/// Agent orchestration (enabled with the `orchestration` feature)
#[cfg(feature = "orchestration")]
pub mod orchestration;

/// Agent implementation (enabled with the `agent` feature)
#[cfg(feature = "agent")]
pub mod agent;

/// Executor implementation (enabled with the `executor` feature)
#[cfg(feature = "executor")]
pub mod executor;

/// Lifecycle implementation (enabled with the `agent` feature)
#[cfg(feature = "agent")]
pub mod lifecycle;

/// Prelude module for convenient imports
pub mod prelude {
    #[cfg(feature = "config")]
    pub use crate::config::{Configuration, ConfigurationLoader, EnvironmentLoader};

    #[cfg(feature = "observability")]
    pub use crate::observability::Logger;

    #[cfg(feature = "checkpoint")]
    pub use crate::checkpoint::{
        get_storage_manager, Checkpoint, CheckpointError, CheckpointResult,
        CheckpointStorageManager, ProjectStorage, SessionStorage,
    };

    #[cfg(feature = "provider")]
    pub use crate::provider::{
        LlmProvider, GenerateResponse, ToolInvocation, ProviderFactory,
        InternalMessage, GenerateConfig, InternalToolDefinition,
    };

    #[cfg(feature = "orchestration")]
    pub use crate::orchestration::{
        AgentRuntime, RuntimeConfig, WorkflowCoordinator, WorkflowStep, WorkflowStatus,
        ToolCoordinator, ToolExecutionResult, ExecutionResult, ExecutionMode, AgentMode,
    };
}
