//! Tool source tracking for the registry.
//!
//! This module defines the different sources from which tools can be
//! registered, enabling source-aware tool management.

use serde::{Deserialize, Serialize};

/// Source of a registered tool.
///
/// Tools in the registry can come from different sources, and this enum
/// tracks the origin for debugging, logging, and policy enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    /// Native tool from CATS or similar local tool library.
    Native,

    /// Tool from an MCP (Model Context Protocol) server.
    Mcp,

    /// Tool from an A2A (Agent-to-Agent) protocol agent.
    /// Reserved for future use.
    A2a,
}

impl ToolSource {
    /// Get the string representation of the source.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolSource::Native => "native",
            ToolSource::Mcp => "mcp",
            ToolSource::A2a => "a2a",
        }
    }

    /// Check if this is a native tool source.
    pub fn is_native(&self) -> bool {
        matches!(self, ToolSource::Native)
    }

    /// Check if this is an MCP tool source.
    pub fn is_mcp(&self) -> bool {
        matches!(self, ToolSource::Mcp)
    }
}

impl std::fmt::Display for ToolSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_source_display() {
        assert_eq!(ToolSource::Native.to_string(), "native");
        assert_eq!(ToolSource::Mcp.to_string(), "mcp");
        assert_eq!(ToolSource::A2a.to_string(), "a2a");
    }

    #[test]
    fn test_tool_source_checks() {
        assert!(ToolSource::Native.is_native());
        assert!(!ToolSource::Native.is_mcp());
        assert!(ToolSource::Mcp.is_mcp());
        assert!(!ToolSource::Mcp.is_native());
    }

    #[test]
    fn test_tool_source_serialization() {
        let json = serde_json::to_string(&ToolSource::Native).unwrap();
        assert_eq!(json, "\"native\"");

        let parsed: ToolSource = serde_json::from_str("\"mcp\"").unwrap();
        assert_eq!(parsed, ToolSource::Mcp);
    }
}
