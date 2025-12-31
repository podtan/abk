//! Extension registry
//!
//! Manages discovered and loaded extensions with capability-based indexing.

use super::error::{ExtensionError, ExtensionResult};
use super::loader::{ExtensionLoader, LoadedWasm};
use super::manifest::ExtensionManifest;
use std::collections::HashMap;
use std::path::PathBuf;

/// Registry of discovered and loaded extensions
pub struct ExtensionRegistry {
    /// Manifests indexed by extension ID
    manifests: HashMap<String, ExtensionManifest>,

    /// Extension directory paths indexed by ID
    extension_paths: HashMap<String, PathBuf>,

    /// Loaded WASM components indexed by ID (lazy loaded)
    loaded: HashMap<String, LoadedWasm>,

    /// Capability index: capability -> list of extension IDs
    capability_index: HashMap<String, Vec<String>>,
}

impl ExtensionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
            extension_paths: HashMap::new(),
            loaded: HashMap::new(),
            capability_index: HashMap::new(),
        }
    }

    /// Register a discovered extension
    ///
    /// # Arguments
    /// * `manifest` - Parsed extension manifest
    /// * `extension_dir` - Directory containing the extension
    pub fn register(&mut self, manifest: ExtensionManifest, extension_dir: PathBuf) {
        let id = manifest.extension.id.clone();

        // Index by capabilities
        for capability in manifest.list_capabilities() {
            self.capability_index
                .entry(capability)
                .or_default()
                .push(id.clone());
        }

        self.extension_paths.insert(id.clone(), extension_dir);
        self.manifests.insert(id, manifest);
    }

    /// Get extension manifest by ID
    pub fn get_manifest(&self, id: &str) -> Option<&ExtensionManifest> {
        self.manifests.get(id)
    }

    /// Get extension directory path by ID
    pub fn get_path(&self, id: &str) -> Option<&PathBuf> {
        self.extension_paths.get(id)
    }

    /// Get extensions by capability
    ///
    /// # Arguments
    /// * `capability` - Capability name (e.g., "lifecycle", "provider")
    ///
    /// # Returns
    /// * `Vec<&ExtensionManifest>` - Manifests of extensions with the capability
    pub fn get_by_capability(&self, capability: &str) -> Vec<&ExtensionManifest> {
        self.capability_index
            .get(capability)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.manifests.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all registered extensions
    pub fn list_all(&self) -> Vec<&ExtensionManifest> {
        self.manifests.values().collect()
    }

    /// List all extension IDs
    pub fn list_ids(&self) -> Vec<&String> {
        self.manifests.keys().collect()
    }

    /// Check if an extension is loaded
    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded.contains_key(id)
    }

    /// Load an extension by ID
    ///
    /// # Arguments
    /// * `id` - Extension ID
    /// * `loader` - WASM loader to use
    ///
    /// # Returns
    /// * `ExtensionResult<&LoadedExtension>` - Reference to loaded extension
    pub async fn load(
        &mut self,
        id: &str,
        loader: &mut ExtensionLoader,
    ) -> ExtensionResult<&LoadedExtension> {
        // Check if already loaded
        if self.loaded.contains_key(id) {
            // Return a LoadedExtension view
            return self.get_loaded(id);
        }

        // Get manifest and path
        let manifest = self.manifests.get(id).ok_or_else(|| {
            ExtensionError::ExtensionNotFound(format!("Extension '{}' not found in registry", id))
        })?;

        let extension_dir = self.extension_paths.get(id).ok_or_else(|| {
            ExtensionError::ExtensionNotFound(format!("Extension path for '{}' not found", id))
        })?;

        // Build WASM path
        let wasm_path = extension_dir.join(&manifest.lib.path);

        // Load WASM
        let loaded_wasm = loader.load_wasm(&wasm_path)?;

        // Store loaded component
        self.loaded.insert(id.to_string(), loaded_wasm);

        self.get_loaded(id)
    }

    /// Get a loaded extension
    fn get_loaded(&self, id: &str) -> ExtensionResult<&LoadedExtension> {
        // This is a bit awkward due to Rust's borrow checker
        // We need to return a view that combines manifest + loaded wasm
        if !self.loaded.contains_key(id) {
            return Err(ExtensionError::NotLoaded(id.to_string()));
        }

        // For now, we'll use a workaround with static lifetime
        // In a real implementation, you might want to use interior mutability
        // or return owned data
        Err(ExtensionError::NotLoaded(format!(
            "Use get_loaded_components() to access loaded extension '{}'",
            id
        )))
    }

    /// Get loaded WASM component by ID
    pub fn get_wasm(&self, id: &str) -> Option<&LoadedWasm> {
        self.loaded.get(id)
    }

    /// Get number of registered extensions
    pub fn count(&self) -> usize {
        self.manifests.len()
    }

    /// Get number of loaded extensions
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A loaded extension with manifest and WASM component
pub struct LoadedExtension {
    /// Extension manifest
    pub manifest: ExtensionManifest,
    /// Extension directory path
    pub path: PathBuf,
    /// Loaded WASM (if loaded)
    pub wasm: Option<LoadedWasm>,
}

impl LoadedExtension {
    /// Get the extension ID
    pub fn id(&self) -> &str {
        &self.manifest.extension.id
    }

    /// Get the extension name
    pub fn name(&self) -> &str {
        &self.manifest.extension.name
    }

    /// Get the extension version
    pub fn version(&self) -> &str {
        &self.manifest.extension.version
    }

    /// Check if the extension is loaded
    pub fn is_loaded(&self) -> bool {
        self.wasm.is_some()
    }

    /// Get capabilities
    pub fn capabilities(&self) -> Vec<String> {
        self.manifest.list_capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::manifest::*;

    fn create_test_manifest(id: &str, lifecycle: bool, provider: bool) -> ExtensionManifest {
        ExtensionManifest {
            extension: ExtensionInfo {
                id: id.to_string(),
                name: format!("{} Extension", id),
                version: "0.1.0".to_string(),
                api_version: "0.3.0".to_string(),
                description: "Test".to_string(),
                authors: vec![],
                repository: None,
            },
            lib: LibInfo {
                kind: "rust".to_string(),
                path: "extension.wasm".to_string(),
            },
            capabilities: Capabilities {
                lifecycle,
                provider,
                tools: false,
                context: false,
            },
            lifecycle: None,
            provider: None,
            settings: toml::Value::Table(Default::default()),
        }
    }

    #[test]
    fn test_registry_register() {
        let mut registry = ExtensionRegistry::new();
        let manifest = create_test_manifest("test", true, false);

        registry.register(manifest, PathBuf::from("/extensions/test"));

        assert_eq!(registry.count(), 1);
        assert!(registry.get_manifest("test").is_some());
    }

    #[test]
    fn test_registry_capability_index() {
        let mut registry = ExtensionRegistry::new();

        let lifecycle_ext = create_test_manifest("lifecycle-ext", true, false);
        let provider_ext = create_test_manifest("provider-ext", false, true);
        let both_ext = create_test_manifest("both-ext", true, true);

        registry.register(lifecycle_ext, PathBuf::from("/ext/lifecycle"));
        registry.register(provider_ext, PathBuf::from("/ext/provider"));
        registry.register(both_ext, PathBuf::from("/ext/both"));

        let lifecycles = registry.get_by_capability("lifecycle");
        assert_eq!(lifecycles.len(), 2);

        let providers = registry.get_by_capability("provider");
        assert_eq!(providers.len(), 2);

        let tools = registry.get_by_capability("tools");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_registry_list_all() {
        let mut registry = ExtensionRegistry::new();

        registry.register(
            create_test_manifest("ext1", true, false),
            PathBuf::from("/ext1"),
        );
        registry.register(
            create_test_manifest("ext2", false, true),
            PathBuf::from("/ext2"),
        );

        let all = registry.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_registry_default() {
        let registry = ExtensionRegistry::default();
        assert_eq!(registry.count(), 0);
    }
}
