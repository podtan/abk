//! Adapter auto-instantiation
//!
//! This module handles automatic creation of adapter instances based on
//! configuration. Adapters are traits that provide context-specific
//! functionality (e.g., SimpatioderContext, CheckpointAccess).

use super::config::AdapterConfig;
use super::error::{DeclarativeError, DeclarativeResult};
use super::router::AdapterRegistry;

/// Auto-instantiate adapters based on configuration
pub struct AdapterFactory;

impl AdapterFactory {
    /// Create adapter instances from configuration
    ///
    /// This is a placeholder for now. In the full implementation,
    /// this would use dynamic dispatch or trait objects to create
    /// the appropriate adapter instances based on the config.
    ///
    /// For example:
    /// - context = "SimpatioderContext" -> create SimpatioderContext instance
    /// - checkpoint = "SimpatioderCheckpointAccess" -> create CheckpointAccess instance
    pub fn create_adapters(_config: &AdapterConfig) -> DeclarativeResult<AdapterRegistry> {
        let registry = AdapterRegistry::new();
        
        // Note: This is a placeholder implementation.
        // The actual implementation will need to:
        // 1. Import the adapter types (e.g., from simpaticoder or other crates)
        // 2. Instantiate them based on config
        // 3. Store them in the registry with type erasure
        //
        // For now, we just create an empty registry.
        // The consuming application (simpaticoder) will populate it.
        
        Ok(registry)
    }
    
    /// Validate adapter configuration
    pub fn validate_config(config: &AdapterConfig) -> DeclarativeResult<()> {
        // Validate that adapter names are recognized
        let known_adapters = vec![
            "SimpatioderContext",
            "SimpatioderCheckpointAccess",
            "SimpatioderRestorationAccess",
            "SimpatioderStorageAccess",
        ];
        
        if let Some(context) = &config.context {
            if !known_adapters.contains(&context.as_str()) {
                eprintln!("⚠️  Unknown context adapter: {}", context);
            }
        }
        
        if let Some(checkpoint) = &config.checkpoint {
            if !known_adapters.contains(&checkpoint.as_str()) {
                eprintln!("⚠️  Unknown checkpoint adapter: {}", checkpoint);
            }
        }
        
        // Custom adapters are always valid (we can't know all possible types)
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    
    #[test]
    fn test_create_empty_adapters() {
        let config = AdapterConfig {
            context: None,
            checkpoint: None,
            restoration: None,
            storage: None,
            custom: HashMap::new(),
        };
        
        let registry = AdapterFactory::create_adapters(&config).unwrap();
        // Empty registry for now
    }
    
    #[test]
    fn test_validate_known_adapter() {
        let config = AdapterConfig {
            context: Some("SimpatioderContext".to_string()),
            checkpoint: Some("SimpatioderCheckpointAccess".to_string()),
            restoration: None,
            storage: None,
            custom: HashMap::new(),
        };
        
        let result = AdapterFactory::validate_config(&config);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_validate_unknown_adapter() {
        let config = AdapterConfig {
            context: Some("UnknownAdapter".to_string()),
            checkpoint: None,
            restoration: None,
            storage: None,
            custom: HashMap::new(),
        };
        
        // Should not fail, just warn
        let result = AdapterFactory::validate_config(&config);
        assert!(result.is_ok());
    }
}
