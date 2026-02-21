//! Factory for creating UnifiedRegistry from configuration.
//!
//! This module provides a factory function that creates a `UnifiedRegistry`
//! from the configuration's `[[tool_sources]]` section.

use std::sync::Arc;

use crate::config::ToolSourceConfig;
use super::{BoxedToolSource, UnifiedRegistry, NativeToolSource, McpToolSource, McpServerConfig};

/// Build a UnifiedRegistry from configuration.
///
/// This function reads the `[[tool_sources]]` configuration and creates
/// the appropriate tool source providers, then aggregates them into
/// a unified registry.
///
/// # Arguments
/// * `configs` - List of tool source configurations
/// * `open_window_size` - Window size for native file reading tools
///
/// # Returns
/// A UnifiedRegistry with all configured tool sources, or an error.
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::build_registry_from_config;
/// use abk::config::Configuration;
///
/// let config = Configuration::default();
/// let registry = build_registry_from_config(&config.tool_sources, 2000).await?;
///
/// // Get all tools
/// let tools = registry.all_schemas();
/// ```
pub async fn build_registry_from_config(
    configs: &[ToolSourceConfig],
    open_window_size: usize,
) -> anyhow::Result<UnifiedRegistry> {
    let mut registry = UnifiedRegistry::new();
    
    for config in configs {
        let source = build_source_from_config(config, open_window_size).await?;
        registry.add_source(source);
    }
    
    // If no sources configured, add default native source
    if registry.source_count() == 0 {
        let default_source = Arc::new(NativeToolSource::new("opencode", open_window_size));
        registry.add_source(default_source);
    }
    
    Ok(registry)
}

/// Build a single tool source from configuration.
async fn build_source_from_config(
    config: &ToolSourceConfig,
    open_window_size: usize,
) -> anyhow::Result<BoxedToolSource> {
    match config {
        ToolSourceConfig::Native { toolset } => {
            let source = NativeToolSource::new(toolset, open_window_size);
            Ok(Arc::new(source))
        }
        ToolSourceConfig::Mcp { name, url, auth_token, auto_init } => {
            let mut server_config = McpServerConfig::new(name, url);
            if let Some(token) = auth_token {
                // Resolve environment variable references
                let resolved = resolve_env_var(token);
                server_config = server_config.with_auth(resolved);
            }
            
            let source = McpToolSource::new(server_config, *auto_init).await?;
            Ok(Arc::new(source))
        }
    }
}

/// Resolve environment variable references in a string.
///
/// Supports patterns like `${VAR_NAME}` and replaces them with
/// the corresponding environment variable value.
fn resolve_env_var(value: &str) -> String {
    if value.starts_with("${") && value.ends_with('}') {
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        std::env::set_var("TEST_TOOL_VAR", "test_value");

        assert_eq!(resolve_env_var("${TEST_TOOL_VAR}"), "test_value");
        assert_eq!(resolve_env_var("plain_value"), "plain_value");
        assert_eq!(
            resolve_env_var("${NONEXISTENT_VAR}"),
            "${NONEXISTENT_VAR}"
        );

        std::env::remove_var("TEST_TOOL_VAR");
    }

    #[tokio::test]
    async fn test_build_registry_empty() {
        let registry = build_registry_from_config(&[], 2000).await.unwrap();
        
        // Should have default native source
        assert_eq!(registry.source_count(), 1);
        assert!(registry.has_tool("bash"));
    }

    #[tokio::test]
    async fn test_build_registry_native() {
        let configs = vec![ToolSourceConfig::Native {
            toolset: "opencode".to_string(),
        }];
        
        let registry = build_registry_from_config(&configs, 2000).await.unwrap();
        
        assert_eq!(registry.source_count(), 1);
        assert!(registry.has_tool("bash"));
        assert!(registry.has_tool("read"));
    }
}
