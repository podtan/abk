//! Unified Registry for aggregating multiple tool sources.
//!
//! This module provides `UnifiedRegistry` which aggregates tools from multiple
//! `ToolSourceProvider` implementations and provides a single interface for
//! listing and executing tools.

use super::{BoxedToolSource, ToolDescriptor, ToolResult};

/// A unified registry that aggregates tools from multiple sources.
///
/// The registry maintains a list of tool source providers and provides:
/// - Aggregated tool schemas for LLM consumption
/// - Automatic routing of tool execution to the correct source
/// - Source tracking for debugging and logging
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::{UnifiedRegistry, NativeToolSource, ToolSourceProvider};
///
/// let mut registry = UnifiedRegistry::new();
///
/// // Add sources
/// let native = NativeToolSource::new("opencode")?;
/// registry.add_source(Arc::new(native));
///
/// // Get all tool schemas for LLM
/// let tools = registry.all_schemas();
///
/// // Execute a tool (routes to correct source)
/// let result = registry.execute("bash", json!({"command": "ls"})).await?;
/// ```
#[derive(Default)]
pub struct UnifiedRegistry {
    /// List of tool sources, in priority order.
    sources: Vec<BoxedToolSource>,
}

impl UnifiedRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tool source to the registry.
    ///
    /// Sources are added in priority order - when multiple sources
    /// provide a tool with the same name, the first match wins.
    pub fn add_source(&mut self, source: BoxedToolSource) {
        self.sources.push(source);
    }

    /// Add multiple tool sources to the registry.
    pub fn add_sources(&mut self, sources: Vec<BoxedToolSource>) {
        self.sources.extend(sources);
    }

    /// Get all tool descriptors from all sources.
    ///
    /// This returns a flat list suitable for sending to an LLM.
    /// The source field in each descriptor indicates the origin.
    pub fn all_schemas(&self) -> Vec<ToolDescriptor> {
        self.sources
            .iter()
            .flat_map(|s| s.tool_descriptors())
            .collect()
    }

    /// Get all tool schemas in OpenAI function format.
    ///
    /// This is a convenience method for preparing tools to send to
    /// OpenAI-compatible LLM providers.
    pub fn to_openai_schemas(&self) -> Vec<serde_json::Value> {
        self.all_schemas()
            .into_iter()
            .map(|d| d.to_openai_schema())
            .collect()
    }

    /// Execute a tool by routing to the appropriate source.
    ///
    /// This method searches sources in priority order and executes
    /// the tool on the first source that owns it.
    ///
    /// # Arguments
    /// * `tool_name` - The name of the tool to execute
    /// * `args` - The arguments as a JSON value
    ///
    /// # Returns
    /// The result of the tool execution, or an error if no source
    /// provides the tool or execution failed.
    ///
    /// # Errors
    /// Returns an error if no source provides the tool.
    pub async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        for source in &self.sources {
            if source.has_tool(tool_name) {
                return source.execute(tool_name, args).await;
            }
        }

        Err(anyhow::anyhow!(
            "Tool '{}' not found in any source",
            tool_name
        ))
    }

    /// Check if any source provides a tool with the given name.
    pub fn has_tool(&self, name: &str) -> bool {
        self.sources.iter().any(|s| s.has_tool(name))
    }

    /// Get the source that provides a tool.
    ///
    /// Returns the name of the source that provides the tool,
    /// or None if no source provides it.
    pub fn get_tool_source(&self, name: &str) -> Option<&str> {
        self.sources
            .iter()
            .find(|s| s.has_tool(name))
            .map(|s| s.name())
    }

    /// Get the total number of tools across all sources.
    pub fn tool_count(&self) -> usize {
        self.all_schemas().len()
    }

    /// Get the number of registered sources.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Get the names of all tools.
    pub fn tool_names(&self) -> Vec<String> {
        self.all_schemas().iter().map(|d| d.name.clone()).collect()
    }

    /// Clear all sources from the registry.
    pub fn clear(&mut self) {
        self.sources.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use crate::registry::ToolSourceProvider;

    struct MockSource {
        name: String,
        tools: Vec<ToolDescriptor>,
    }

    impl MockSource {
        fn new(name: &str, tools: Vec<&str>) -> Self {
            let descriptors = tools
                .into_iter()
                .map(|t| {
                    ToolDescriptor::new(
                        t,
                        format!("Tool: {}", t),
                        serde_json::json!({"type": "object"}),
                        name,
                    )
                })
                .collect();
            Self {
                name: name.to_string(),
                tools: descriptors,
            }
        }
    }

    #[async_trait]
    impl ToolSourceProvider for MockSource {
        fn name(&self) -> &str {
            &self.name
        }

        fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
            self.tools.clone()
        }

        async fn execute(
            &self,
            tool_name: &str,
            _args: serde_json::Value,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success(format!(
                "Executed {} from {}",
                tool_name, self.name
            )))
        }

        fn has_tool(&self, name: &str) -> bool {
            self.tools.iter().any(|t| t.name == name)
        }
    }

    #[test]
    fn test_empty_registry() {
        let registry = UnifiedRegistry::new();
        assert_eq!(registry.tool_count(), 0);
        assert_eq!(registry.source_count(), 0);
        assert!(!registry.has_tool("any"));
    }

    #[test]
    fn test_add_source() {
        let mut registry = UnifiedRegistry::new();
        let source = Arc::new(MockSource::new("test", vec!["tool_a", "tool_b"]));
        registry.add_source(source);

        assert_eq!(registry.source_count(), 1);
        assert_eq!(registry.tool_count(), 2);
        assert!(registry.has_tool("tool_a"));
        assert!(registry.has_tool("tool_b"));
    }

    #[test]
    fn test_multiple_sources() {
        let mut registry = UnifiedRegistry::new();
        registry.add_source(Arc::new(MockSource::new("source-a", vec!["tool_a"])));
        registry.add_source(Arc::new(MockSource::new("source-b", vec!["tool_b"])));

        assert_eq!(registry.source_count(), 2);
        assert_eq!(registry.tool_count(), 2);
        assert_eq!(registry.get_tool_source("tool_a"), Some("source-a"));
        assert_eq!(registry.get_tool_source("tool_b"), Some("source-b"));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let mut registry = UnifiedRegistry::new();
        registry.add_source(Arc::new(MockSource::new("test", vec!["bash"])));

        let result = registry
            .execute("bash", serde_json::json!({"command": "ls"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.content.contains("bash"));
    }

    #[tokio::test]
    async fn test_execute_missing_tool() {
        let registry = UnifiedRegistry::new();

        let result = registry
            .execute("nonexistent", serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_priority_routing() {
        let mut registry = UnifiedRegistry::new();
        // First source has priority
        registry.add_source(Arc::new(MockSource::new("priority", vec!["tool"])));
        registry.add_source(Arc::new(MockSource::new("fallback", vec!["tool"])));

        // Should route to first source
        assert_eq!(registry.get_tool_source("tool"), Some("priority"));
    }

    #[test]
    fn test_to_openai_schemas() {
        let mut registry = UnifiedRegistry::new();
        registry.add_source(Arc::new(MockSource::new("test", vec!["bash"])));

        let schemas = registry.to_openai_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["type"], "function");
        assert_eq!(schemas[0]["function"]["name"], "bash");
    }
}
