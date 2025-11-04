//! Error types for declarative CLI framework

use thiserror::Error;

/// Result type for declarative CLI operations
pub type DeclarativeResult<T> = Result<T, DeclarativeError>;

/// Errors that can occur in the declarative CLI framework
#[derive(Debug, Error)]
pub enum DeclarativeError {
    /// Config file errors
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    /// Command routing errors
    #[error("Routing error: {0}")]
    RoutingError(String),
    
    /// Adapter instantiation errors
    #[error("Adapter error: {0}")]
    AdapterError(String),
    
    /// Command execution errors
    #[error("Execution error: {0}")]
    ExecutionError(String),
    
    /// Invalid argument type or value
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    
    /// Command not found in ABK registry
    #[error("Command not found: {0}")]
    CommandNotFound(String),
    
    /// Special handler not implemented
    #[error("Special handler not found: {0}")]
    HandlerNotFound(String),
    
    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Other errors
    #[error("{0}")]
    Other(String),
}

impl DeclarativeError {
    /// Create a config error with context
    pub fn config<S: Into<String>>(msg: S) -> Self {
        Self::ConfigError(msg.into())
    }
    
    /// Create a routing error with context
    pub fn routing<S: Into<String>>(msg: S) -> Self {
        Self::RoutingError(msg.into())
    }
    
    /// Create an adapter error with context
    pub fn adapter<S: Into<String>>(msg: S) -> Self {
        Self::AdapterError(msg.into())
    }
    
    /// Create an execution error with context
    pub fn execution<S: Into<String>>(msg: S) -> Self {
        Self::ExecutionError(msg.into())
    }
}
