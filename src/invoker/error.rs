//! Error types for the invoker module.

use crate::invoker::InvokerSource;
use thiserror::Error;

/// Errors that can occur during invoker operations.
///
/// This enum covers all error conditions in the invoker module,
/// from registration failures to execution errors.
///
/// # Example
///
/// ```
/// use abk::invoker::{InvokerError, InvokerSource};
///
/// let error = InvokerError::NotFound {
///     name: "unknown_tool".to_string(),
/// };
/// assert!(error.to_string().contains("unknown_tool"));
/// ```
#[derive(Debug, Error)]
pub enum InvokerError {
	/// The requested invoker was not found in the registry.
	#[error("invoker not found: {name}")]
	NotFound {
		/// Name of the invoker that was not found.
		name: String,
	},

	/// An invoker with the same name is already registered.
	#[error("invoker already registered: {name}")]
	DuplicateName {
		/// Name of the duplicate invoker.
		name: String,
	},

	/// Execution of the invoker failed.
	#[error("execution failed for {name}: {message}")]
	ExecutionFailed {
		/// Name of the invoker that failed.
		name: String,
		/// Description of the failure.
		message: String,
	},

	/// The arguments provided to the invoker were invalid.
	#[error("invalid arguments for {name}: {message}")]
	InvalidArguments {
		/// Name of the invoker with invalid arguments.
		name: String,
		/// Description of the validation failure.
		message: String,
	},

	/// The source for the invoker is not available.
	#[error("source unavailable: {0}")]
	SourceUnavailable(InvokerSource),

	/// An error occurred in an adapter while discovering or registering invokers.
	#[error("adapter error: {message}")]
	AdapterError {
		/// Description of the adapter error.
		message: String,
	},

	/// A serialization or deserialization error occurred.
	#[error("serialization error: {message}")]
	SerializationError {
		/// Description of the serialization error.
		message: String,
	},
}

impl InvokerError {
	/// Create a NotFound error for the given invoker name.
	pub fn not_found(name: impl Into<String>) -> Self {
		Self::NotFound { name: name.into() }
	}

	/// Create a DuplicateName error for the given invoker name.
	pub fn duplicate_name(name: impl Into<String>) -> Self {
		Self::DuplicateName { name: name.into() }
	}

	/// Create an ExecutionFailed error.
	pub fn execution_failed(name: impl Into<String>, message: impl Into<String>) -> Self {
		Self::ExecutionFailed {
			name: name.into(),
			message: message.into(),
		}
	}

	/// Create an InvalidArguments error.
	pub fn invalid_arguments(name: impl Into<String>, message: impl Into<String>) -> Self {
		Self::InvalidArguments {
			name: name.into(),
			message: message.into(),
		}
	}

	/// Create a SourceUnavailable error.
	pub fn source_unavailable(source: InvokerSource) -> Self {
		Self::SourceUnavailable(source)
	}

	/// Create an AdapterError.
	pub fn adapter_error(message: impl Into<String>) -> Self {
		Self::AdapterError {
			message: message.into(),
		}
	}

	/// Create a SerializationError.
	pub fn serialization_error(message: impl Into<String>) -> Self {
		Self::SerializationError {
			message: message.into(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_not_found_error() {
		let error = InvokerError::not_found("my_tool");
		assert!(error.to_string().contains("my_tool"));
		assert!(error.to_string().contains("not found"));
	}

	#[test]
	fn test_duplicate_name_error() {
		let error = InvokerError::duplicate_name("existing_tool");
		assert!(error.to_string().contains("existing_tool"));
		assert!(error.to_string().contains("already registered"));
	}

	#[test]
	fn test_execution_failed_error() {
		let error = InvokerError::execution_failed("failing_tool", "connection timeout");
		assert!(error.to_string().contains("failing_tool"));
		assert!(error.to_string().contains("connection timeout"));
	}

	#[test]
	fn test_invalid_arguments_error() {
		let error = InvokerError::invalid_arguments("strict_tool", "missing required field 'path'");
		assert!(error.to_string().contains("strict_tool"));
		assert!(error.to_string().contains("missing required field"));
	}

	#[test]
	fn test_source_unavailable_error() {
		let error = InvokerError::source_unavailable(InvokerSource::Mcp);
		assert!(error.to_string().contains("mcp"));
		assert!(error.to_string().contains("unavailable"));
	}

	#[test]
	fn test_adapter_error() {
		let error = InvokerError::adapter_error("failed to connect to MCP server");
		assert!(error.to_string().contains("failed to connect"));
	}

	#[test]
	fn test_serialization_error() {
		let error = InvokerError::serialization_error("invalid JSON");
		assert!(error.to_string().contains("invalid JSON"));
	}

	#[test]
	fn test_error_is_send_sync() {
		fn assert_send_sync<T: Send + Sync>() {}
		assert_send_sync::<InvokerError>();
	}
}
