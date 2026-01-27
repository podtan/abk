//! Tool Registry Module
//!
//! Provides multi-source tool aggregation with source tracking.
//! Tools from Native, MCP, and A2A sources are stored and converted
//! to `InternalTool` format for LLM consumption.
//!
//! ## Features
//!
//! - `registry` - Core registry functionality with native tool support
//! - `registry-mcp` - Additional support for MCP tool registration
//!
//! ## Usage
//!
//! ```rust,ignore
//! use abk::registry::ToolRegistry;
//! use umf::InternalTool;
//! use serde_json::json;
//!
//! // Create a new registry
//! let registry = ToolRegistry::new();
//!
//! // Register native tools
//! let tool = InternalTool::new("search", "Search files", json!({"type": "object"}));
//! registry.register_native(tool)?;
//!
//! // With registry-mcp feature, register MCP tools
//! #[cfg(feature = "registry-mcp")]
//! {
//!     use umf::McpTool;
//!     let mcp_tool: McpTool = /* from server */;
//!     registry.register_mcp(mcp_tool, "weather-server")?;
//! }
//!
//! // Get all tools for LLM
//! let tools = registry.to_internal_tools();
//! ```

mod error;
mod registered;
mod registry;
mod source;

pub use error::{RegistryError, RegistryResult};
pub use registered::RegisteredTool;
pub use registry::ToolRegistry;
pub use source::ToolSource;

// MCP support (requires registry-mcp feature)
#[cfg(feature = "registry-mcp")]
mod mcp;
