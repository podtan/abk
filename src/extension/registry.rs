//! Extension registry
//!
//! Manages discovered and loaded extensions with capability-based indexing.

use super::bindings::{ExtensionInstance, ProviderExtensionInstance};
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

    /// Instantiated extensions indexed by ID (full extension world)
    instances: HashMap<String, ExtensionInstance>,

    /// Instantiated provider-only extensions indexed by ID
    provider_instances: HashMap<String, ProviderExtensionInstance>,

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
            instances: HashMap::new(),
            provider_instances: HashMap::new(),
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

    /// Check if an extension is loaded (WASM bytes loaded)
    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded.contains_key(id)
    }

    /// Check if an extension is instantiated (ready to call)
    pub fn is_instantiated(&self, id: &str) -> bool {
        self.instances.contains_key(id)
    }

    /// Load an extension WASM by ID (lazy loading)
    ///
    /// This loads the WASM bytes but does not instantiate the component.
    /// Call `instantiate()` to create a callable instance.
    ///
    /// # Arguments
    /// * `id` - Extension ID
    /// * `loader` - WASM loader to use
    ///
    /// # Returns
    /// * `ExtensionResult<()>` - Success if loaded
    pub fn load_wasm(
        &mut self,
        id: &str,
        loader: &ExtensionLoader,
    ) -> ExtensionResult<()> {
        // Check if already loaded
        if self.loaded.contains_key(id) {
            return Ok(());
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

        Ok(())
    }

    /// Instantiate an extension by ID
    ///
    /// Creates a callable instance of the extension. Automatically loads
    /// the WASM if not already loaded.
    ///
    /// # Arguments
    /// * `id` - Extension ID
    /// * `loader` - WASM loader to use
    ///
    /// # Returns
    /// * `ExtensionResult<&mut ExtensionInstance>` - Mutable reference to instance
    pub fn instantiate(
        &mut self,
        id: &str,
        loader: &ExtensionLoader,
    ) -> ExtensionResult<&mut ExtensionInstance> {
        // Load WASM if needed
        self.load_wasm(id, loader)?;

        // Check if already instantiated
        if !self.instances.contains_key(id) {
            // Get loaded WASM
            let loaded_wasm = self.loaded.get(id).ok_or_else(|| {
                ExtensionError::NotLoaded(format!("Extension '{}' not loaded", id))
            })?;

            // Create instance
            let instance =
                ExtensionInstance::new(loaded_wasm.engine(), loaded_wasm.component())?;

            self.instances.insert(id.to_string(), instance);
        }

        // Return mutable reference to instance
        self.instances.get_mut(id).ok_or_else(|| {
            ExtensionError::NotLoaded(format!("Extension '{}' instantiation failed", id))
        })
    }

    /// Get a mutable reference to an instantiated extension
    ///
    /// Returns None if not instantiated. Use `instantiate()` first.
    pub fn get_instance_mut(&mut self, id: &str) -> Option<&mut ExtensionInstance> {
        self.instances.get_mut(id)
    }

    /// Instantiate a provider-only extension by ID
    ///
    /// Creates a callable instance of a provider-only extension.
    /// Use this for extensions that only have provider capability (no lifecycle).
    ///
    /// # Arguments
    /// * `id` - Extension ID
    /// * `loader` - WASM loader to use
    ///
    /// # Returns
    /// * `ExtensionResult<&mut ProviderExtensionInstance>` - Mutable reference to instance
    pub async fn instantiate_provider(
        &mut self,
        id: &str,
        loader: &ExtensionLoader,
    ) -> ExtensionResult<&mut ProviderExtensionInstance> {
        // Load WASM if needed
        self.load_wasm(id, loader)?;

        // Check if already instantiated
        if !self.provider_instances.contains_key(id) {
            // Get loaded WASM
            let loaded_wasm = self.loaded.get(id).ok_or_else(|| {
                ExtensionError::NotLoaded(format!("Extension '{}' not loaded", id))
            })?;

            // Create provider-only instance (async)
            let instance =
                ProviderExtensionInstance::new(loaded_wasm.engine(), loaded_wasm.component()).await?;

            self.provider_instances.insert(id.to_string(), instance);
        }

        // Return mutable reference to instance
        self.provider_instances.get_mut(id).ok_or_else(|| {
            ExtensionError::NotLoaded(format!("Extension '{}' instantiation failed", id))
        })
    }

    /// Get a mutable reference to an instantiated provider-only extension
    ///
    /// Returns None if not instantiated. Use `instantiate_provider()` first.
    pub fn get_provider_instance_mut(&mut self, id: &str) -> Option<&mut ProviderExtensionInstance> {
        self.provider_instances.get_mut(id)
    }

    /// Get loaded WASM component by ID
    pub fn get_wasm(&self, id: &str) -> Option<&LoadedWasm> {
        self.loaded.get(id)
    }

    /// Get number of registered extensions
    pub fn count(&self) -> usize {
        self.manifests.len()
    }

    /// Get number of loaded extensions (WASM loaded)
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Get number of instantiated extensions (ready to call)
    pub fn instantiated_count(&self) -> usize {
        self.instances.len()
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
