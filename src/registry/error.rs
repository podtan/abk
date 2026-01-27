//! Error types for the tool registry.

use thiserror::Error;

use super::ToolSource;

/// Errors that can occur during registry operations.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// A tool with the same name already exists from a different source.
    #[error("Tool '{name}' already registered from {existing_source}")]
    Conflict {
        /// Name of the conflicting tool.
        name: String,
        /// Source of the existing tool.
        existing_source: ToolSource,
    },

    /// The tool name is invalid (empty or contains invalid characters).
    #[error("Invalid tool name '{0}': must be non-empty and contain only alphanumeric characters, underscores, or hyphens")]
    InvalidName(String),

    /// The requested tool was not found.
    #[error("Tool '{0}' not found")]
    NotFound(String),

    /// Error during type conversion.
    #[error("Conversion error: {0}")]
    ConversionError(String),
}

/// Result type for registry operations.
pub type RegistryResult<T> = Result<T, RegistryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RegistryError::Conflict {
            name: "test_tool".to_string(),
            existing_source: ToolSource::Native,
        };
        assert!(err.to_string().contains("test_tool"));
        assert!(err.to_string().contains("native"));

        let err = RegistryError::InvalidName("bad name!".to_string());
        assert!(err.to_string().contains("bad name!"));

        let err = RegistryError::NotFound("missing".to_string());
        assert!(err.to_string().contains("missing"));
    }
}
