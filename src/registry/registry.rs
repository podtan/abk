//! Thread-safe tool registry for multi-source tool aggregation.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use umf::InternalTool;

use super::{RegisteredTool, RegistryError, RegistryResult, ToolSource};

/// Internal state of the registry.
#[derive(Debug, Default)]
pub(crate) struct RegistryInner {
    /// All registered tools.
    pub(crate) tools: Vec<RegisteredTool>,

    /// Index from tool name to position in tools vector.
    pub(crate) name_index: HashMap<String, usize>,
}

/// Thread-safe registry for aggregating tools from multiple sources.
///
/// The `ToolRegistry` provides a central storage for tools from different
/// sources (Native, MCP, A2A) with source tracking and name conflict detection.
///
/// # Thread Safety
///
/// The registry is wrapped in `Arc<RwLock<...>>` internally, making it safe
/// to clone and use across threads.
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::ToolRegistry;
/// use umf::InternalTool;
/// use serde_json::json;
///
/// let registry = ToolRegistry::new();
///
/// // Register a native tool
/// let tool = InternalTool::new("search", "Search files", json!({"type": "object"}));
/// registry.register_native(tool)?;
///
/// // Get all tools for LLM consumption
/// let tools = registry.to_internal_tools();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a tool name.
    ///
    /// Tool names must be non-empty and contain only alphanumeric
    /// characters, underscores, or hyphens.
    pub(crate) fn validate_name(name: &str) -> RegistryResult<()> {
        if name.is_empty() {
            return Err(RegistryError::InvalidName(name.to_string()));
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(RegistryError::InvalidName(name.to_string()));
        }

        Ok(())
    }

    /// Register a native tool.
    ///
    /// Returns an error if a tool with the same name already exists.
    pub fn register_native(&self, tool: InternalTool) -> RegistryResult<()> {
        Self::validate_name(&tool.name)?;

        let mut inner = self.inner.write().unwrap();

        if let Some(&idx) = inner.name_index.get(&tool.name) {
            return Err(RegistryError::Conflict {
                name: tool.name,
                existing_source: inner.tools[idx].source(),
            });
        }

        let idx = inner.tools.len();
        inner.name_index.insert(tool.name.clone(), idx);
        inner.tools.push(RegisteredTool::native(tool));

        Ok(())
    }

    /// Register multiple native tools.
    ///
    /// Stops on first error and returns the count of successfully registered tools.
    pub fn register_native_batch(&self, tools: Vec<InternalTool>) -> RegistryResult<usize> {
        let mut count = 0;
        for tool in tools {
            self.register_native(tool)?;
            count += 1;
        }
        Ok(count)
    }

    /// Find a tool by name.
    ///
    /// Returns `None` if the tool is not found.
    pub fn find(&self, name: &str) -> Option<RegisteredTool> {
        let inner = self.inner.read().unwrap();
        inner
            .name_index
            .get(name)
            .map(|&idx| inner.tools[idx].clone())
    }

    /// Get a tool by name, returning an error if not found.
    pub fn get(&self, name: &str) -> RegistryResult<RegisteredTool> {
        self.find(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))
    }

    /// Get all tools as `InternalTool` for LLM consumption.
    ///
    /// This returns clones of all tools in the registry, suitable for
    /// passing to an LLM provider.
    pub fn to_internal_tools(&self) -> Vec<InternalTool> {
        let inner = self.inner.read().unwrap();
        inner.tools.iter().map(|rt| rt.tool().clone()).collect()
    }

    /// List tools filtered by source.
    pub fn list_by_source(&self, source: ToolSource) -> Vec<RegisteredTool> {
        let inner = self.inner.read().unwrap();
        inner
            .tools
            .iter()
            .filter(|rt| rt.source() == source)
            .cloned()
            .collect()
    }

    /// List all native tools.
    pub fn native_tools(&self) -> Vec<RegisteredTool> {
        self.list_by_source(ToolSource::Native)
    }

    /// List all MCP tools.
    pub fn mcp_tools(&self) -> Vec<RegisteredTool> {
        self.list_by_source(ToolSource::Mcp)
    }

    /// Get the total number of registered tools.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().tools.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// List all tool names.
    pub fn tool_names(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.name_index.keys().cloned().collect()
    }

    /// Check if a tool with the given name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.inner.read().unwrap().name_index.contains_key(name)
    }

    /// Remove a tool by name.
    ///
    /// Returns the removed tool, or an error if not found.
    pub fn remove(&self, name: &str) -> RegistryResult<RegisteredTool> {
        let mut inner = self.inner.write().unwrap();

        let idx = inner
            .name_index
            .remove(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))?;

        // Remove the tool and update indices
        let tool = inner.tools.remove(idx);

        // Update all indices that were shifted
        for (_, index) in inner.name_index.iter_mut() {
            if *index > idx {
                *index -= 1;
            }
        }

        Ok(tool)
    }

    /// Clear all tools from the registry.
    pub fn clear(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.tools.clear();
        inner.name_index.clear();
    }

    /// Get access to the internal state (for MCP module).
    #[cfg(feature = "registry-mcp")]
    pub(crate) fn inner(&self) -> &Arc<RwLock<RegistryInner>> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool(name: &str) -> InternalTool {
        InternalTool::new(
            name,
            format!("Tool: {}", name),
            json!({
                "type": "object",
                "properties": {}
            }),
        )
    }

    #[test]
    fn test_register_native() {
        let registry = ToolRegistry::new();

        registry.register_native(sample_tool("tool_a")).unwrap();
        registry.register_native(sample_tool("tool_b")).unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("tool_a"));
        assert!(registry.contains("tool_b"));
    }

    #[test]
    fn test_conflict_detection() {
        let registry = ToolRegistry::new();

        registry.register_native(sample_tool("duplicate")).unwrap();
        let err = registry.register_native(sample_tool("duplicate")).unwrap_err();

        assert!(matches!(err, RegistryError::Conflict { .. }));
    }

    #[test]
    fn test_invalid_name() {
        let registry = ToolRegistry::new();

        // Empty name
        let err = registry
            .register_native(InternalTool::new("", "Empty", json!({})))
            .unwrap_err();
        assert!(matches!(err, RegistryError::InvalidName(_)));

        // Name with spaces
        let err = registry
            .register_native(InternalTool::new("bad name", "Spaces", json!({})))
            .unwrap_err();
        assert!(matches!(err, RegistryError::InvalidName(_)));
    }

    #[test]
    fn test_find_and_get() {
        let registry = ToolRegistry::new();
        registry.register_native(sample_tool("findme")).unwrap();

        assert!(registry.find("findme").is_some());
        assert!(registry.find("notfound").is_none());

        assert!(registry.get("findme").is_ok());
        assert!(matches!(
            registry.get("notfound"),
            Err(RegistryError::NotFound(_))
        ));
    }

    #[test]
    fn test_to_internal_tools() {
        let registry = ToolRegistry::new();
        registry.register_native(sample_tool("a")).unwrap();
        registry.register_native(sample_tool("b")).unwrap();

        let tools = registry.to_internal_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_list_by_source() {
        let registry = ToolRegistry::new();
        registry.register_native(sample_tool("native_1")).unwrap();
        registry.register_native(sample_tool("native_2")).unwrap();

        let native = registry.native_tools();
        assert_eq!(native.len(), 2);

        let mcp = registry.mcp_tools();
        assert_eq!(mcp.len(), 0);
    }

    #[test]
    fn test_remove() {
        let registry = ToolRegistry::new();
        registry.register_native(sample_tool("a")).unwrap();
        registry.register_native(sample_tool("b")).unwrap();
        registry.register_native(sample_tool("c")).unwrap();

        let removed = registry.remove("b").unwrap();
        assert_eq!(removed.name(), "b");

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("a"));
        assert!(!registry.contains("b"));
        assert!(registry.contains("c"));
    }

    #[test]
    fn test_clear() {
        let registry = ToolRegistry::new();
        registry.register_native(sample_tool("a")).unwrap();
        registry.register_native(sample_tool("b")).unwrap();

        registry.clear();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_batch_register() {
        let registry = ToolRegistry::new();

        let tools = vec![sample_tool("x"), sample_tool("y"), sample_tool("z")];

        let count = registry.register_native_batch(tools).unwrap();
        assert_eq!(count, 3);
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let registry = ToolRegistry::new();
        let registry_clone = registry.clone();

        let handle = thread::spawn(move || {
            registry_clone.register_native(sample_tool("from_thread")).unwrap();
        });

        registry.register_native(sample_tool("from_main")).unwrap();

        handle.join().unwrap();

        assert_eq!(registry.len(), 2);
    }
}
