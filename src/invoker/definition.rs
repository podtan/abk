//! Invoker definition type for representing invocable operations.

use crate::invoker::InvokerSource;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// A canonical representation of an invocable operation.
///
/// `InvokerDefinition` is the unified type for representing tools, skills,
/// or any other callable operation regardless of its source (CATS, MCP, A2A).
///
/// # Example
///
/// ```
/// use abk::invoker::{InvokerDefinition, InvokerSource};
/// use serde_json::json;
///
/// let def = InvokerDefinition::new(
///     "read_file",
///     "Read the contents of a file",
///     json!({
///         "type": "object",
///         "properties": {
///             "path": { "type": "string", "description": "File path to read" }
///         },
///         "required": ["path"]
///     }),
///     InvokerSource::Native,
/// );
///
/// // Generate OpenAI-compatible function schema
/// let schema = def.to_openai_function();
/// assert_eq!(schema["function"]["name"], "read_file");
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvokerDefinition {
	/// Unique identifier for this invoker.
	///
	/// This name is used for lookup in the registry and must be unique
	/// across all registered invokers.
	pub name: String,

	/// Human-readable description for LLM consumption.
	///
	/// This should clearly explain what the invoker does and when to use it.
	pub description: String,

	/// JSON Schema describing the parameters this invoker accepts.
	///
	/// This should be a valid JSON Schema object compatible with
	/// OpenAI's function calling format.
	pub parameters: Value,

	/// The origin of this invoker (Native, MCP, or A2A).
	pub source: InvokerSource,

	/// Optional metadata for routing, filtering, or other purposes.
	///
	/// This can store source-specific information like MCP server URL,
	/// A2A agent ID, or other routing hints.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub metadata: Option<HashMap<String, Value>>,
}

impl InvokerDefinition {
	/// Create a new invoker definition.
	///
	/// # Arguments
	///
	/// * `name` - Unique identifier for the invoker
	/// * `description` - Human-readable description
	/// * `parameters` - JSON Schema for the parameters
	/// * `source` - Origin of this invoker
	///
	/// # Example
	///
	/// ```
	/// use abk::invoker::{InvokerDefinition, InvokerSource};
	/// use serde_json::json;
	///
	/// let def = InvokerDefinition::new(
	///     "search",
	///     "Search for files",
	///     json!({"type": "object", "properties": {}}),
	///     InvokerSource::Native,
	/// );
	/// ```
	pub fn new(
		name: impl Into<String>,
		description: impl Into<String>,
		parameters: Value,
		source: InvokerSource,
	) -> Self {
		Self {
			name: name.into(),
			description: description.into(),
			parameters,
			source,
			metadata: None,
		}
	}

	/// Create a new invoker definition with default empty parameters.
	///
	/// # Example
	///
	/// ```
	/// use abk::invoker::{InvokerDefinition, InvokerSource};
	///
	/// let def = InvokerDefinition::new_simple(
	///     "ping",
	///     "Check if the service is alive",
	///     InvokerSource::Mcp,
	/// );
	/// ```
	pub fn new_simple(
		name: impl Into<String>,
		description: impl Into<String>,
		source: InvokerSource,
	) -> Self {
		Self::new(
			name,
			description,
			json!({
				"type": "object",
				"properties": {}
			}),
			source,
		)
	}

	/// Add a metadata entry using builder pattern.
	///
	/// # Example
	///
	/// ```
	/// use abk::invoker::{InvokerDefinition, InvokerSource};
	/// use serde_json::json;
	///
	/// let def = InvokerDefinition::new_simple("tool", "A tool", InvokerSource::Mcp)
	///     .with_metadata("server_url", json!("http://localhost:8080"))
	///     .with_metadata("timeout_ms", json!(5000));
	/// ```
	pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
		self.metadata
			.get_or_insert_with(HashMap::new)
			.insert(key.into(), value);
		self
	}

	/// Get a metadata value by key.
	pub fn get_metadata(&self, key: &str) -> Option<&Value> {
		self.metadata.as_ref()?.get(key)
	}

	/// Generate an OpenAI-compatible function calling schema.
	///
	/// This returns a JSON object in the format expected by OpenAI's
	/// function calling API and compatible providers.
	///
	/// # Example
	///
	/// ```
	/// use abk::invoker::{InvokerDefinition, InvokerSource};
	/// use serde_json::json;
	///
	/// let def = InvokerDefinition::new(
	///     "get_weather",
	///     "Get current weather",
	///     json!({
	///         "type": "object",
	///         "properties": {
	///             "city": { "type": "string" }
	///         },
	///         "required": ["city"]
	///     }),
	///     InvokerSource::Native,
	/// );
	///
	/// let schema = def.to_openai_function();
	/// assert_eq!(schema["type"], "function");
	/// assert_eq!(schema["function"]["name"], "get_weather");
	/// ```
	pub fn to_openai_function(&self) -> Value {
		json!({
			"type": "function",
			"function": {
				"name": self.name,
				"description": self.description,
				"parameters": self.parameters,
			}
		})
	}

	/// Check if this invoker has any parameters defined.
	pub fn has_parameters(&self) -> bool {
		if let Some(props) = self.parameters.get("properties") {
			if let Some(obj) = props.as_object() {
				return !obj.is_empty();
			}
		}
		false
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_new() {
		let def = InvokerDefinition::new(
			"test_tool",
			"A test tool",
			json!({"type": "object", "properties": {"x": {"type": "number"}}}),
			InvokerSource::Native,
		);

		assert_eq!(def.name, "test_tool");
		assert_eq!(def.description, "A test tool");
		assert_eq!(def.source, InvokerSource::Native);
		assert!(def.metadata.is_none());
	}

	#[test]
	fn test_new_simple() {
		let def = InvokerDefinition::new_simple("ping", "Ping the server", InvokerSource::Mcp);

		assert_eq!(def.name, "ping");
		assert_eq!(def.parameters["type"], "object");
		assert!(def.parameters["properties"].as_object().unwrap().is_empty());
	}

	#[test]
	fn test_with_metadata() {
		let def = InvokerDefinition::new_simple("tool", "A tool", InvokerSource::A2a)
			.with_metadata("agent_id", json!("agent-123"))
			.with_metadata("priority", json!(10));

		let meta = def.metadata.as_ref().unwrap();
		assert_eq!(meta.len(), 2);
		assert_eq!(meta.get("agent_id").unwrap(), "agent-123");
		assert_eq!(meta.get("priority").unwrap(), 10);
	}

	#[test]
	fn test_get_metadata() {
		let def = InvokerDefinition::new_simple("tool", "A tool", InvokerSource::Native)
			.with_metadata("key", json!("value"));

		assert_eq!(def.get_metadata("key"), Some(&json!("value")));
		assert_eq!(def.get_metadata("missing"), None);

		// Test with no metadata
		let def2 = InvokerDefinition::new_simple("tool2", "Another tool", InvokerSource::Native);
		assert_eq!(def2.get_metadata("any"), None);
	}

	#[test]
	fn test_to_openai_function() {
		let def = InvokerDefinition::new(
			"read_file",
			"Read file contents",
			json!({
				"type": "object",
				"properties": {
					"path": {"type": "string", "description": "File path"}
				},
				"required": ["path"]
			}),
			InvokerSource::Native,
		);

		let schema = def.to_openai_function();

		assert_eq!(schema["type"], "function");
		assert_eq!(schema["function"]["name"], "read_file");
		assert_eq!(schema["function"]["description"], "Read file contents");
		assert_eq!(schema["function"]["parameters"]["type"], "object");
		assert_eq!(
			schema["function"]["parameters"]["properties"]["path"]["type"],
			"string"
		);
	}

	#[test]
	fn test_has_parameters() {
		let with_params = InvokerDefinition::new(
			"tool",
			"desc",
			json!({
				"type": "object",
				"properties": {"x": {"type": "number"}}
			}),
			InvokerSource::Native,
		);
		assert!(with_params.has_parameters());

		let without_params =
			InvokerDefinition::new_simple("tool2", "desc", InvokerSource::Native);
		assert!(!without_params.has_parameters());

		let null_params = InvokerDefinition::new("tool3", "desc", json!(null), InvokerSource::Native);
		assert!(!null_params.has_parameters());
	}

	#[test]
	fn test_serde_roundtrip() {
		let def = InvokerDefinition::new(
			"my_tool",
			"Does something",
			json!({"type": "object", "properties": {}}),
			InvokerSource::Mcp,
		)
		.with_metadata("url", json!("http://example.com"));

		let json = serde_json::to_string(&def).unwrap();
		let parsed: InvokerDefinition = serde_json::from_str(&json).unwrap();

		assert_eq!(def.name, parsed.name);
		assert_eq!(def.description, parsed.description);
		assert_eq!(def.source, parsed.source);
		assert_eq!(def.parameters, parsed.parameters);
		assert_eq!(def.metadata, parsed.metadata);
	}

	#[test]
	fn test_serde_without_metadata() {
		let def = InvokerDefinition::new_simple("tool", "A tool", InvokerSource::Native);

		let json = serde_json::to_string(&def).unwrap();
		// metadata should be skipped when None
		assert!(!json.contains("metadata"));

		let parsed: InvokerDefinition = serde_json::from_str(&json).unwrap();
		assert!(parsed.metadata.is_none());
	}

	#[test]
	fn test_clone() {
		let def = InvokerDefinition::new_simple("tool", "A tool", InvokerSource::Native)
			.with_metadata("key", json!("value"));

		let cloned = def.clone();
		assert_eq!(def.name, cloned.name);
		assert_eq!(def.metadata, cloned.metadata);
	}
}
