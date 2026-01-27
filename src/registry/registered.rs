//! Registered tool wrapper with source tracking.

use serde::{Deserialize, Serialize};
use umf::InternalTool;

use super::ToolSource;

/// A tool with source tracking information.
///
/// This struct wraps an [`InternalTool`] with metadata about where
/// it came from, enabling source-aware tool management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredTool {
    /// The wrapped internal tool.
    tool: InternalTool,

    /// Source of this tool (Native, Mcp, A2a).
    source: ToolSource,

    /// Origin identifier (e.g., MCP server name, A2A agent ID).
    /// None for native tools.
    origin: Option<String>,
}

impl RegisteredTool {
    /// Create a new registered tool.
    pub fn new(tool: InternalTool, source: ToolSource, origin: Option<String>) -> Self {
        Self {
            tool,
            source,
            origin,
        }
    }

    /// Create a registered native tool.
    pub fn native(tool: InternalTool) -> Self {
        Self::new(tool, ToolSource::Native, None)
    }

    /// Create a registered MCP tool.
    pub fn mcp(tool: InternalTool, server: impl Into<String>) -> Self {
        Self::new(tool, ToolSource::Mcp, Some(server.into()))
    }

    /// Get a reference to the wrapped tool.
    pub fn tool(&self) -> &InternalTool {
        &self.tool
    }

    /// Consume self and return the wrapped tool.
    pub fn into_tool(self) -> InternalTool {
        self.tool
    }

    /// Get the source of this tool.
    pub fn source(&self) -> ToolSource {
        self.source
    }

    /// Get the origin identifier, if any.
    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    /// Get the tool name.
    pub fn name(&self) -> &str {
        &self.tool.name
    }

    /// Get the tool description.
    pub fn description(&self) -> &str {
        &self.tool.description
    }

    /// Check if this tool is from a native source.
    pub fn is_native(&self) -> bool {
        self.source.is_native()
    }

    /// Check if this tool is from an MCP source.
    pub fn is_mcp(&self) -> bool {
        self.source.is_mcp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool() -> InternalTool {
        InternalTool::new(
            "test_tool",
            "A test tool",
            json!({
                "type": "object",
                "properties": {}
            }),
        )
    }

    #[test]
    fn test_native_tool() {
        let tool = RegisteredTool::native(sample_tool());

        assert_eq!(tool.name(), "test_tool");
        assert_eq!(tool.source(), ToolSource::Native);
        assert!(tool.origin().is_none());
        assert!(tool.is_native());
        assert!(!tool.is_mcp());
    }

    #[test]
    fn test_mcp_tool() {
        let tool = RegisteredTool::mcp(sample_tool(), "weather-server");

        assert_eq!(tool.name(), "test_tool");
        assert_eq!(tool.source(), ToolSource::Mcp);
        assert_eq!(tool.origin(), Some("weather-server"));
        assert!(tool.is_mcp());
        assert!(!tool.is_native());
    }

    #[test]
    fn test_into_tool() {
        let tool = RegisteredTool::native(sample_tool());
        let internal = tool.into_tool();

        assert_eq!(internal.name, "test_tool");
    }

    #[test]
    fn test_serialization() {
        let tool = RegisteredTool::mcp(sample_tool(), "my-server");
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: RegisteredTool = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name(), "test_tool");
        assert_eq!(parsed.source(), ToolSource::Mcp);
        assert_eq!(parsed.origin(), Some("my-server"));
    }
}
