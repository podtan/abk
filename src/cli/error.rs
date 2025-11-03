//! Error types for CLI operations

use thiserror::Error;

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

/// Errors that can occur during CLI command execution
#[derive(Error, Debug)]
pub enum CliError {
    /// Error executing a command or operation
    #[error("Execution error: {0}")]
    ExecutionError(String),

    /// Error from adapter implementation
    #[error("Adapter error: {0}")]
    AdapterError(String),

    /// Configuration loading or validation error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Checkpoint operation error
    #[error("Checkpoint error: {0}")]
    CheckpointError(String),

    /// Provider operation error
    #[error("Provider error: {0}")]
    ProviderError(String),

    /// Tool registry error
    #[error("Tool registry error: {0}")]
    ToolError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerdeError(String),

    /// Serialization error (alternative name for compatibility)
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Invalid argument or input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),
}

// Conversions from common error types
impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        CliError::SerdeError(err.to_string())
    }
}

impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> Self {
        CliError::ExecutionError(err.to_string())
    }
}
