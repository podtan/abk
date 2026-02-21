//! Native Tool Source - wraps cats::ToolRegistry.
//!
//! This module provides `NativeToolSource` which implements `ToolSourceProvider`
//! by wrapping the cats tool registry for in-process tool execution.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{ToolDescriptor, ToolSourceProvider, ToolResult};
use crate::registry::provider::ToolResult as ProviderResult;

/// A tool source that wraps the native cats tool registry.
///
/// This provides tools from the cats crate (Code Agent Tool System) for
/// in-process execution. The toolset is selected at compile time via
/// cargo features (`opencode`, `old`, etc.).
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::{NativeToolSource, ToolSourceProvider};
///
/// let source = NativeToolSource::new("opencode", 2000);
///
/// // Get all tool descriptors
/// let tools = source.tool_descriptors();
///
/// // Execute a tool
/// let result = source.execute("bash", json!({"command": "ls"})).await?;
/// ```
pub struct NativeToolSource {
    /// Source identifier (e.g., "cats-opencode")
    name: String,
    /// The wrapped cats tool registry
    registry: Arc<Mutex<cats::ToolRegistry>>,
    /// Cached tool descriptors
    descriptors: Vec<ToolDescriptor>,
}

impl NativeToolSource {
    /// Create a new native tool source with the specified toolset.
    ///
    /// # Arguments
    /// * `toolset` - The toolset identifier (e.g., "opencode", "old")
    /// * `open_window_size` - Window size for file reading tools
    ///
    /// # Returns
    /// A new NativeToolSource with all tools from the selected toolset.
    pub fn new(toolset: &str, open_window_size: usize) -> Self {
        let registry = cats::create_tool_registry_with_open_window_size(Some(open_window_size));
        Self::from_registry(toolset, registry)
    }

    /// Create a native tool source from an existing registry.
    ///
    /// This is useful when you need to customize the registry before
    /// wrapping it as a tool source.
    pub fn from_registry(toolset: &str, registry: cats::ToolRegistry) -> Self {
        let name = format!("cats-{}", toolset);
        let descriptors = Self::build_descriptors(&registry, &name);
        Self {
            name,
            registry: Arc::new(Mutex::new(registry)),
            descriptors,
        }
    }

    /// Build tool descriptors from the registry.
    fn build_descriptors(registry: &cats::ToolRegistry, source_name: &str) -> Vec<ToolDescriptor> {
        registry
            .list_tools()
            .into_iter()
            .filter_map(|tool_name| {
                let tool = registry.get_tool(&tool_name)?;
                Some(ToolDescriptor::new(
                    tool.name(),
                    tool.description(),
                    tool.get_parameters_schema(),
                    source_name,
                ))
            })
            .collect()
    }

    /// Get the underlying cats registry (for advanced use cases).
    pub fn registry(&self) -> Arc<Mutex<cats::ToolRegistry>> {
        Arc::clone(&self.registry)
    }
}

#[async_trait]
impl ToolSourceProvider for NativeToolSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
        self.descriptors.clone()
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<ProviderResult> {
        let mut registry = self.registry.lock().map_err(|e| {
            anyhow::anyhow!("Failed to acquire registry lock: {}", e)
        })?;

        // Convert JSON args to cats ToolArgs
        let cats_args = json_to_cats_args(args);

        match registry.execute_tool(tool_name, &cats_args) {
            Ok(result) => Ok(ProviderResult {
                success: result.success,
                content: result.message,
                data: result.data,
            }),
            Err(e) => Ok(ProviderResult::failure(e.to_string())),
        }
    }

    fn has_tool(&self, name: &str) -> bool {
        self.descriptors.iter().any(|d| d.name == name)
    }
}

/// Convert a JSON value to cats ToolArgs.
///
/// This handles the conversion from LLM-provided JSON to the
/// cats tool argument format.
fn json_to_cats_args(args: serde_json::Value) -> cats::ToolArgs {
    use std::collections::HashMap;

    match args {
        serde_json::Value::Object(map) => {
            let named_args: HashMap<String, String> = map
                .into_iter()
                .filter_map(|(k, v)| {
                    let value = match v {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Array(arr) => {
                            serde_json::to_string(&arr).unwrap_or_default()
                        }
                        serde_json::Value::Object(obj) => {
                            serde_json::to_string(&obj).unwrap_or_default()
                        }
                        serde_json::Value::Null => String::new(),
                    };
                    Some((k, value))
                })
                .collect();

            cats::ToolArgs::with_named_args(vec![], named_args)
        }
        serde_json::Value::Array(arr) => {
            let args: Vec<String> = arr
                .into_iter()
                .filter_map(|v| match v {
                    serde_json::Value::String(s) => Some(s),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    serde_json::Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect();
            cats::ToolArgs::with_named_args(args, HashMap::new())
        }
        _ => cats::ToolArgs::with_named_args(vec![], HashMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_source_creation() {
        let source = NativeToolSource::new("opencode", 2000);

        assert_eq!(source.name(), "cats-opencode");
        assert!(!source.tool_descriptors().is_empty());
    }

    #[test]
    fn test_has_tool() {
        let source = NativeToolSource::new("opencode", 2000);

        // These tools should exist in opencode toolset
        assert!(source.has_tool("bash"));
        assert!(source.has_tool("read"));
        assert!(source.has_tool("write"));
    }

    #[test]
    fn test_tool_descriptors() {
        let source = NativeToolSource::new("opencode", 2000);
        let descriptors = source.tool_descriptors();

        // All descriptors should have the correct source
        for desc in &descriptors {
            assert_eq!(desc.source, "cats-opencode");
            assert!(!desc.name.is_empty());
            assert!(!desc.description.is_empty());
        }
    }

    #[tokio::test]
    async fn test_execute_missing_tool() {
        let source = NativeToolSource::new("opencode", 2000);

        let result = source
            .execute("nonexistent_tool", serde_json::json!({}))
            .await;

        // Should return Ok with failure status (not an error)
        assert!(result.is_ok());
        assert!(!result.unwrap().success);
    }
}
