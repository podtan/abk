//! ToolRegistryAdapter trait
//!
//! Provides access to tool registry operations.

use crate::cli::error::CliResult;
use serde_json::Value;
use std::collections::HashMap;

/// Information about a tool
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// JSON schema for tool parameters
    pub schema: Value,
    /// Tool category (if any)
    pub category: Option<String>,
}

/// Result of tool execution
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Output from tool execution
    pub output: String,
    /// Error message if failed
    pub error: Option<String>,
}

/// Adapter for interacting with tool registry
///
/// This trait abstracts tool registry operations (typically backed by
/// `cats::ToolRegistry`) without creating a dependency.
///
/// # Example
///
/// ```rust,ignore
/// use abk::cli::ToolRegistryAdapter;
///
/// struct MyToolAdapter {
///     // ... fields
/// }
///
/// impl ToolRegistryAdapter for MyToolAdapter {
///     fn list_tool_schemas(&self) -> CliResult<Vec<ToolInfo>> {
///         // Return tool schemas from cats registry
///         Ok(vec![])
///     }
///
///     // ... implement remaining methods
/// }
/// ```
pub trait ToolRegistryAdapter {
    /// Get schemas for all registered tools
    fn list_tool_schemas(&self) -> CliResult<Vec<ToolInfo>>;

    /// Get schema for a specific tool
    fn get_tool_schema(&self, tool_name: &str) -> CliResult<ToolInfo>;

    /// Validate tool arguments against schema
    ///
    /// Returns true if arguments are valid, error otherwise
    fn validate_tool_args(&self, tool_name: &str, args: &HashMap<String, Value>) -> CliResult<bool>;

    /// Execute a tool with given arguments
    ///
    /// Note: This is primarily for testing/debugging tools from CLI
    fn execute_tool(&self, tool_name: &str, args: HashMap<String, Value>) -> CliResult<ToolExecutionResult>;

    /// Get tool categories
    ///
    /// Returns map of category -> list of tool names
    fn get_tool_categories(&self) -> CliResult<HashMap<String, Vec<String>>>;
}
