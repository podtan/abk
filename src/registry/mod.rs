//! Tool Registry Module
//!
//! Provides multi-source tool aggregation with source tracking.
//! Tools from Native, MCP, and A2A sources are stored and converted
//! to `InternalTool` format for LLM consumption.
//!
//! ## Architecture
//!
//! The registry uses a **Unified ToolSource Architecture**:
//!
//! - `ToolSourceProvider` trait - abstraction for tool providers (cats, MCP, WASM, etc.)
//! - `UnifiedRegistry` - aggregates all tool sources into a single interface
//! - `NativeToolSource` - wraps cats::ToolRegistry for in-process tools
//! - `McpToolSource` - wraps MCP server connections for remote tools
//!
//! ## Features
//!
//! - `registry` - Core registry functionality with native tool support
//! - `registry-mcp` - Additional support for MCP tool registration
//!
//! ## Usage
//!
//! ```rust,ignore
//! use abk::registry::{UnifiedRegistry, NativeToolSource, ToolSourceProvider};
//!
//! // Create unified registry
//! let mut registry = UnifiedRegistry::new();
//!
//! // Add native tools (from cats)
//! let native = NativeToolSource::new("opencode")?;
//! registry.add_source(Box::new(native));
//!
//! // Get all tools for LLM
//! let tools = registry.all_schemas();
//!
//! // Execute a tool (routes to correct source automatically)
//! let result = registry.execute("bash", json!({"command": "ls"})).await?;
//! ```

mod error;
mod registered;
mod registry;
mod source_kind;
mod provider;
mod unified;
mod factory;

pub use error::{RegistryError, RegistryResult};
pub use registered::RegisteredTool;
pub use registry::ToolRegistry;
pub use source_kind::ToolSource;
pub use provider::{ToolSourceProvider, ToolDescriptor, ToolResult, BoxedToolSource};
pub use unified::UnifiedRegistry;
pub use factory::build_registry_from_config;

// MCP support (requires registry-mcp feature)
#[cfg(feature = "registry-mcp")]
mod mcp;

// MCP client (requires registry-mcp feature)
#[cfg(feature = "registry-mcp")]
mod client;
#[cfg(feature = "registry-mcp")]
pub use client::{McpClient, McpServerConfig, McpToolCallResult};

// Native tool source (requires cats feature)
#[cfg(feature = "agent")]
mod native_source;
#[cfg(feature = "agent")]
pub use native_source::NativeToolSource;

// MCP tool source (requires registry-mcp feature)
#[cfg(feature = "registry-mcp")]
mod mcp_source;
#[cfg(feature = "registry-mcp")]
pub use mcp_source::McpToolSource;
