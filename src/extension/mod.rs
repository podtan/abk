//! Extension System for ABK
//!
//! This module provides a unified extension system for loading WASM-based
//! extensions that can provide various capabilities (lifecycle, provider, etc.).
//!
//! # Overview
//!
//! The extension system uses:
//! - **`extension.toml`** - Manifest file describing the extension
//! - **WASM Component Model** - Extensions are WASM components implementing WIT interfaces
//! - **Capability-based architecture** - Extensions declare what capabilities they provide
//!
//! # Example
//!
//! ```ignore
//! use abk::extension::{ExtensionManager, ExtensionManifest};
//! use std::path::Path;
//!
//! // Create extension manager
//! let mut manager = ExtensionManager::new(Path::new("extensions")).await?;
//!
//! // Discover all extensions
//! let manifests = manager.discover().await?;
//!
//! // Get extensions by capability
//! let lifecycles = manager.get_by_capability("lifecycle");
//! ```

mod error;
mod loader;
mod manifest;
mod registry;
mod bindings;

pub use error::{ExtensionError, ExtensionResult};
pub use loader::ExtensionLoader;
pub use manifest::{Capabilities, ExtensionInfo, ExtensionManifest, LibInfo};
pub use registry::{ExtensionRegistry, LoadedExtension};
pub use bindings::{ExtensionInstance, ExtensionState, ProviderExtensionInstance, LifecycleExtensionInstance};
// Re-export generated WIT types for external use
pub use bindings::{core, lifecycle, provider, provider_only, lifecycle_only};

use std::path::{Path, PathBuf};

/// Main extension manager for discovering and loading extensions
pub struct ExtensionManager {
    /// Directory containing extensions
    extensions_dir: PathBuf,
    /// Registry of discovered/loaded extensions
    registry: ExtensionRegistry,
    /// WASM loader
    loader: ExtensionLoader,
}

impl ExtensionManager {
    /// Create a new ExtensionManager for the given extensions directory
    ///
    /// # Arguments
    /// * `extensions_dir` - Path to directory containing extension subdirectories
    ///
    /// # Returns
    /// * `ExtensionResult<Self>` - The manager or an error
    pub async fn new(extensions_dir: impl AsRef<Path>) -> ExtensionResult<Self> {
        let extensions_dir = extensions_dir.as_ref().to_path_buf();
        let loader = ExtensionLoader::new()?;
        let registry = ExtensionRegistry::new();

        Ok(Self {
            extensions_dir,
            registry,
            loader,
        })
    }

    /// Discover all extensions in the extensions directory
    ///
    /// Scans for subdirectories containing `extension.toml` files and
    /// parses their manifests.
    ///
    /// # Returns
    /// * `ExtensionResult<Vec<ExtensionManifest>>` - List of discovered manifests
    pub async fn discover(&mut self) -> ExtensionResult<Vec<ExtensionManifest>> {
        let mut manifests = Vec::new();

        // Check if directory exists
        if !self.extensions_dir.exists() {
            return Ok(manifests);
        }

        // Read directory entries
        let entries = std::fs::read_dir(&self.extensions_dir).map_err(|e| {
            ExtensionError::IoError(format!(
                "Failed to read extensions directory {:?}: {}",
                self.extensions_dir, e
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                ExtensionError::IoError(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("extension.toml");
                if manifest_path.exists() {
                    match ExtensionManifest::from_file(&manifest_path) {
                        Ok(manifest) => {
                            // Register in registry
                            self.registry.register(manifest.clone(), path.clone());
                            manifests.push(manifest);
                        }
                        Err(e) => {
                            // Log warning but continue discovering other extensions
                            eprintln!(
                                "[WARN] Failed to parse extension manifest {:?}: {}",
                                manifest_path, e
                            );
                        }
                    }
                }
            }
        }

        Ok(manifests)
    }

    /// Instantiate a specific extension by ID
    ///
    /// Creates a callable instance of the extension. Automatically loads
    /// the WASM if not already loaded.
    ///
    /// # Arguments
    /// * `id` - Extension ID from manifest
    ///
    /// # Returns
    /// * `ExtensionResult<&mut ExtensionInstance>` - Mutable reference to instance
    pub fn instantiate(&mut self, id: &str) -> ExtensionResult<&mut ExtensionInstance> {
        self.registry.instantiate(id, &self.loader)
    }

    /// Get a mutable reference to an instantiated extension
    ///
    /// Returns None if not instantiated. Use `instantiate()` first.
    pub fn get_instance_mut(&mut self, id: &str) -> Option<&mut ExtensionInstance> {
        self.registry.get_instance_mut(id)
    }

    /// Instantiate a provider-only extension by ID
    ///
    /// Creates a callable instance of a provider-only extension.
    /// Use this for extensions that only have provider capability (no lifecycle).
    ///
    /// # Arguments
    /// * `id` - Extension ID from manifest
    ///
    /// # Returns
    /// * `ExtensionResult<&mut ProviderExtensionInstance>` - Mutable reference to instance
    pub async fn instantiate_provider(&mut self, id: &str) -> ExtensionResult<&mut ProviderExtensionInstance> {
        self.registry.instantiate_provider(id, &self.loader).await
    }

    /// Get a mutable reference to an instantiated provider-only extension
    ///
    /// Returns None if not instantiated. Use `instantiate_provider()` first.
    pub fn get_provider_instance_mut(&mut self, id: &str) -> Option<&mut ProviderExtensionInstance> {
        self.registry.get_provider_instance_mut(id)
    }

    /// Get extensions by capability
    ///
    /// # Arguments
    /// * `capability` - Capability name (e.g., "lifecycle", "provider")
    ///
    /// # Returns
    /// * `Vec<&ExtensionManifest>` - Extensions providing the capability
    pub fn get_by_capability(&self, capability: &str) -> Vec<&ExtensionManifest> {
        self.registry.get_by_capability(capability)
    }

    /// Get all extensions with lifecycle capability
    pub fn get_lifecycles(&self) -> Vec<&ExtensionManifest> {
        self.get_by_capability("lifecycle")
    }

    /// Get all extensions with provider capability
    pub fn get_providers(&self) -> Vec<&ExtensionManifest> {
        self.get_by_capability("provider")
    }

    /// Get all discovered extension manifests
    pub fn list_all(&self) -> Vec<&ExtensionManifest> {
        self.registry.list_all()
    }

    /// Get the extensions directory path
    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_discover_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = ExtensionManager::new(temp_dir.path()).await.unwrap();
        let manifests = manager.discover().await.unwrap();
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_discover_nonexistent_directory() {
        let mut manager = ExtensionManager::new("/nonexistent/path").await.unwrap();
        let manifests = manager.discover().await.unwrap();
        assert!(manifests.is_empty());
    }

    #[tokio::test]
    async fn test_discover_extension() {
        let temp_dir = TempDir::new().unwrap();

        // Create extension directory
        let ext_dir = temp_dir.path().join("test-extension");
        std::fs::create_dir(&ext_dir).unwrap();

        // Create manifest
        let manifest_content = r#"
[extension]
id = "test-extension"
name = "Test Extension"
version = "0.1.0"
api_version = "0.3.0"
description = "A test extension"

[lib]
kind = "rust"
path = "extension.wasm"

[capabilities]
lifecycle = true
"#;
        std::fs::write(ext_dir.join("extension.toml"), manifest_content).unwrap();

        // Discover
        let mut manager = ExtensionManager::new(temp_dir.path()).await.unwrap();
        let manifests = manager.discover().await.unwrap();

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].extension.id, "test-extension");
        assert!(manifests[0].capabilities.lifecycle);
    }
}
