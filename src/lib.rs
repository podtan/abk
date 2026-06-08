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

/// Get the home directory path (infallible).
///
/// Falls back through HOME → USERPROFILE → "." to support
/// Linux/macOS, Windows SSH, and Windows direct terminal.
pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string())
}

/// Strip the Windows UNC prefix `\\?\` from a path.
///
/// On Windows, `std::fs::canonicalize()` returns paths with a `\\?\` prefix
/// (e.g., `\\?\C:\Projects\Foo`). This prefix is the Windows extended-length path
/// notation that bypasses the 260-character MAX_PATH limit. While functionally
/// equivalent to the unprefixed path, it causes string comparisons to fail.
///
/// This function strips the prefix so paths are stored and compared consistently.
/// On non-Windows platforms, returns the path unchanged.
pub fn strip_unc_prefix(path: &std::path::Path) -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(s) = path.to_str() {
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return std::path::PathBuf::from(stripped);
            }
        }
    }
    path.to_path_buf()
}

/// Get the home directory path (fallible).
///
/// Falls back through HOME → USERPROFILE.
/// Returns an error if neither environment variable is set.
pub fn get_home_dir() -> Result<String, String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Could not determine home directory".to_string())
}

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

/// Extension system (enabled with the `extension` feature)
#[cfg(feature = "extension")]
pub mod extension;

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

/// Tool Registry for multi-source tool aggregation (enabled with the `registry` feature)
#[cfg(feature = "registry")]
pub mod registry;

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

    #[cfg(feature = "registry")]
    pub use crate::registry::{
        RegisteredTool, RegistryError, RegistryResult, ToolRegistry, ToolSource,
    };
}
