//! ABK Invoker Module - Unified abstraction for invocable operations.
//!
//! The invoker module provides a source-agnostic way to manage and invoke
//! tools, skills, and other callable operations. It supports multiple
//! sources including native tools (CATS), MCP servers, and A2A peer agents.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
//! │ CATS Tools  │   │ MCP Server  │   │ A2A Agent   │
//! └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
//!        │                 │                 │
//!        ▼                 ▼                 ▼
//! ┌─────────────────────────────────────────────────┐
//! │              InvokerAdapter trait               │
//! └─────────────────────────┬───────────────────────┘
//!                           │
//!                           ▼
//! ┌─────────────────────────────────────────────────┐
//! │              InvokerRegistry trait              │
//! │         (DefaultInvokerRegistry impl)           │
//! └─────────────────────────┬───────────────────────┘
//!                           │
//!                           ▼
//! ┌─────────────────────────────────────────────────┐
//! │              InvokerDefinition                  │
//! │    - name, description, parameters, source      │
//! │    - to_openai_function() for LLM integration  │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ```
//! use abk::invoker::{
//!     InvokerDefinition, InvokerSource, InvokerRegistry,
//!     DefaultInvokerRegistry, generate_openai_tools,
//! };
//! use serde_json::json;
//!
//! // Create a registry
//! let mut registry = DefaultInvokerRegistry::new();
//!
//! // Register some invokers
//! registry.register(InvokerDefinition::new(
//!     "read_file",
//!     "Read the contents of a file",
//!     json!({
//!         "type": "object",
//!         "properties": {
//!             "path": { "type": "string", "description": "File path" }
//!         },
//!         "required": ["path"]
//!     }),
//!     InvokerSource::Native,
//! )).unwrap();
//!
//! // Generate OpenAI-compatible tool schemas
//! let tools = generate_openai_tools(&registry);
//! assert_eq!(tools.len(), 1);
//! assert_eq!(tools[0]["function"]["name"], "read_file");
//! ```
//!
//! # Using Adapters
//!
//! ```
//! use abk::invoker::{
//!     InvokerAdapter, StaticAdapter, InvokerDefinition,
//!     InvokerSource, InvokerRegistry, DefaultInvokerRegistry,
//! };
//!
//! // Create an adapter with predefined tools
//! let adapter = StaticAdapter::new(
//!     InvokerSource::Native,
//!     vec![
//!         InvokerDefinition::new_simple("ping", "Check connectivity", InvokerSource::Native),
//!         InvokerDefinition::new_simple("status", "Get status", InvokerSource::Native),
//!     ],
//! );
//!
//! // Register all tools from the adapter
//! let mut registry = DefaultInvokerRegistry::new();
//! let count = adapter.register_all(&mut registry).unwrap();
//! assert_eq!(count, 2);
//! ```
//!
//! # Feature Gate
//!
//! This module is gated behind the `invoker` feature:
//!
//! ```toml
//! [dependencies]
//! abk = { version = "0.2", features = ["invoker"] }
//! ```

mod adapter;
mod definition;
mod error;
mod registry;
mod source;

pub use adapter::{InvokerAdapter, StaticAdapter};
pub use definition::InvokerDefinition;
pub use error::InvokerError;
pub use registry::{DefaultInvokerRegistry, InvokerRegistry};
pub use source::InvokerSource;

/// Result type for invoker operations.
pub type InvokerResult<T> = Result<T, InvokerError>;

/// Create a new empty invoker registry.
///
/// This is a convenience function for creating a `DefaultInvokerRegistry`.
///
/// # Example
///
/// ```
/// use abk::invoker::{create_registry, InvokerRegistry};
///
/// let registry = create_registry();
/// assert!(registry.is_empty());
/// ```
pub fn create_registry() -> DefaultInvokerRegistry {
	DefaultInvokerRegistry::new()
}

/// Generate OpenAI-compatible tool schemas from a registry.
///
/// This function iterates over all invokers in the registry and
/// generates the corresponding OpenAI function calling schemas.
///
/// # Example
///
/// ```
/// use abk::invoker::{
///     create_registry, InvokerDefinition, InvokerSource,
///     InvokerRegistry, generate_openai_tools,
/// };
/// use serde_json::json;
///
/// let mut registry = create_registry();
/// registry.register(InvokerDefinition::new_simple(
///     "my_tool",
///     "Does something",
///     InvokerSource::Native,
/// )).unwrap();
///
/// let tools = generate_openai_tools(&registry);
/// assert_eq!(tools.len(), 1);
/// assert_eq!(tools[0]["type"], "function");
/// ```
pub fn generate_openai_tools(registry: &dyn InvokerRegistry) -> Vec<serde_json::Value> {
	registry
		.list()
		.iter()
		.map(|def| def.to_openai_function())
		.collect()
}

/// Generate OpenAI-compatible tool schemas for a specific source.
///
/// # Example
///
/// ```
/// use abk::invoker::{
///     create_registry, InvokerDefinition, InvokerSource,
///     InvokerRegistry, generate_openai_tools_by_source,
/// };
///
/// let mut registry = create_registry();
/// registry.register(InvokerDefinition::new_simple(
///     "native_tool",
///     "Native tool",
///     InvokerSource::Native,
/// )).unwrap();
/// registry.register(InvokerDefinition::new_simple(
///     "mcp_tool",
///     "MCP tool",
///     InvokerSource::Mcp,
/// )).unwrap();
///
/// let native_tools = generate_openai_tools_by_source(&registry, InvokerSource::Native);
/// assert_eq!(native_tools.len(), 1);
///
/// let mcp_tools = generate_openai_tools_by_source(&registry, InvokerSource::Mcp);
/// assert_eq!(mcp_tools.len(), 1);
/// ```
pub fn generate_openai_tools_by_source(
	registry: &dyn InvokerRegistry,
	source: InvokerSource,
) -> Vec<serde_json::Value> {
	registry
		.list_by_source(source)
		.iter()
		.map(|def| def.to_openai_function())
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn test_create_registry() {
		let registry = create_registry();
		assert!(registry.is_empty());
	}

	#[test]
	fn test_generate_openai_tools() {
		let mut registry = create_registry();

		registry
			.register(InvokerDefinition::new(
				"tool_a",
				"Tool A description",
				json!({"type": "object", "properties": {}}),
				InvokerSource::Native,
			))
			.unwrap();

		registry
			.register(InvokerDefinition::new(
				"tool_b",
				"Tool B description",
				json!({
					"type": "object",
					"properties": {
						"x": {"type": "number"}
					}
				}),
				InvokerSource::Mcp,
			))
			.unwrap();

		let tools = generate_openai_tools(&registry);
		assert_eq!(tools.len(), 2);

		for tool in &tools {
			assert_eq!(tool["type"], "function");
			assert!(tool["function"]["name"].is_string());
			assert!(tool["function"]["description"].is_string());
		}
	}

	#[test]
	fn test_generate_openai_tools_by_source() {
		let mut registry = create_registry();

		registry
			.register(InvokerDefinition::new_simple(
				"native1",
				"Native 1",
				InvokerSource::Native,
			))
			.unwrap();

		registry
			.register(InvokerDefinition::new_simple(
				"native2",
				"Native 2",
				InvokerSource::Native,
			))
			.unwrap();

		registry
			.register(InvokerDefinition::new_simple(
				"mcp1",
				"MCP 1",
				InvokerSource::Mcp,
			))
			.unwrap();

		let native_tools = generate_openai_tools_by_source(&registry, InvokerSource::Native);
		assert_eq!(native_tools.len(), 2);

		let mcp_tools = generate_openai_tools_by_source(&registry, InvokerSource::Mcp);
		assert_eq!(mcp_tools.len(), 1);

		let a2a_tools = generate_openai_tools_by_source(&registry, InvokerSource::A2a);
		assert_eq!(a2a_tools.len(), 0);
	}

	#[test]
	fn test_invoker_result_type() {
		fn returns_result() -> InvokerResult<i32> {
			Ok(42)
		}

		fn returns_error() -> InvokerResult<i32> {
			Err(InvokerError::not_found("test"))
		}

		assert_eq!(returns_result().unwrap(), 42);
		assert!(returns_error().is_err());
	}
}
