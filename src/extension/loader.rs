//! Extension WASM loader
//!
//! Handles loading WASM extension binaries using wasmtime component model.

use super::error::{ExtensionError, ExtensionResult};
use std::path::Path;
use std::sync::Arc;
use wasmtime::component::Component;
use wasmtime::Engine;

/// Current host API version
pub const HOST_API_VERSION: &str = "0.3.0";

/// WASM extension loader using wasmtime
pub struct ExtensionLoader {
    /// Wasmtime engine (shared across all extensions)
    engine: Arc<Engine>,
}

impl ExtensionLoader {
    /// Create a new extension loader
    pub fn new() -> ExtensionResult<Self> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);

        let engine = Engine::new(&config).map_err(|e| {
            ExtensionError::WasmLoadError(format!("Failed to create WASM engine: {}", e))
        })?;

        Ok(Self {
            engine: Arc::new(engine),
        })
    }

    /// Load a WASM component from a file
    ///
    /// # Arguments
    /// * `wasm_path` - Path to the WASM file
    ///
    /// # Returns
    /// * `ExtensionResult<LoadedWasm>` - Loaded WASM component
    pub fn load_wasm(&self, wasm_path: &Path) -> ExtensionResult<LoadedWasm> {
        if !wasm_path.exists() {
            return Err(ExtensionError::WasmLoadError(format!(
                "WASM file not found: {:?}",
                wasm_path
            )));
        }

        let wasm_bytes = std::fs::read(wasm_path).map_err(|e| {
            ExtensionError::WasmLoadError(format!("Failed to read WASM file {:?}: {}", wasm_path, e))
        })?;

        self.load_wasm_bytes(&wasm_bytes, wasm_path)
    }

    /// Load a WASM component from bytes
    ///
    /// # Arguments
    /// * `wasm_bytes` - Raw WASM bytes
    /// * `source_path` - Source path for error messages
    ///
    /// # Returns
    /// * `ExtensionResult<LoadedWasm>` - Loaded WASM component
    pub fn load_wasm_bytes(
        &self,
        wasm_bytes: &[u8],
        source_path: &Path,
    ) -> ExtensionResult<LoadedWasm> {
        // Parse API version from custom section (if present)
        let api_version = parse_api_version(wasm_bytes);

        // Check version compatibility
        if let Some(ref version) = api_version {
            if !check_version_compatibility(version, HOST_API_VERSION) {
                return Err(ExtensionError::IncompatibleVersion {
                    extension_version: version.clone(),
                    host_version: HOST_API_VERSION.to_string(),
                });
            }
        }

        // Load as component
        let component = Component::from_binary(&self.engine, wasm_bytes).map_err(|e| {
            ExtensionError::WasmLoadError(format!(
                "Failed to load WASM component from {:?}: {}",
                source_path, e
            ))
        })?;

        Ok(LoadedWasm {
            engine: Arc::clone(&self.engine),
            component,
            api_version,
        })
    }

    /// Get the wasmtime engine
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }
}

/// A loaded WASM component ready for instantiation
pub struct LoadedWasm {
    /// Wasmtime engine
    pub engine: Arc<Engine>,
    /// Loaded component
    pub component: Component,
    /// API version from WASM custom section (if present)
    pub api_version: Option<String>,
}

impl LoadedWasm {
    /// Get the component
    pub fn component(&self) -> &Component {
        &self.component
    }

    /// Get the engine
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }
}

/// Parse API version from WASM custom section
///
/// Looks for a custom section named "abk:api-version" containing the version string.
fn parse_api_version(wasm_bytes: &[u8]) -> Option<String> {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        if let Ok(Payload::CustomSection(section)) = payload {
            if section.name() == "abk:api-version" {
                if let Ok(version_str) = std::str::from_utf8(section.data()) {
                    return Some(version_str.trim().to_string());
                }
            }
        }
    }
    None
}

/// Check if extension version is compatible with host version
///
/// Uses semver compatibility rules:
/// - Major version must match
/// - Extension minor version must be <= host minor version
fn check_version_compatibility(extension_version: &str, host_version: &str) -> bool {
    let ext_parts: Vec<u32> = extension_version
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let host_parts: Vec<u32> = host_version
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    if ext_parts.len() < 2 || host_parts.len() < 2 {
        // Can't parse versions, allow
        return true;
    }

    let ext_major = ext_parts[0];
    let ext_minor = ext_parts[1];
    let host_major = host_parts[0];
    let host_minor = host_parts[1];

    // Major must match, minor must be compatible
    ext_major == host_major && ext_minor <= host_minor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_compatibility_same_version() {
        assert!(check_version_compatibility("0.3.0", "0.3.0"));
    }

    #[test]
    fn test_version_compatibility_older_minor() {
        assert!(check_version_compatibility("0.2.0", "0.3.0"));
    }

    #[test]
    fn test_version_compatibility_newer_minor() {
        assert!(!check_version_compatibility("0.4.0", "0.3.0"));
    }

    #[test]
    fn test_version_compatibility_different_major() {
        assert!(!check_version_compatibility("1.0.0", "0.3.0"));
    }

    #[test]
    fn test_version_compatibility_patch_ignored() {
        assert!(check_version_compatibility("0.3.5", "0.3.0"));
        assert!(check_version_compatibility("0.3.0", "0.3.5"));
    }

    #[test]
    fn test_loader_creation() {
        let loader = ExtensionLoader::new();
        assert!(loader.is_ok());
    }
}
