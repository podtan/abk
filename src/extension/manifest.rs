//! Extension manifest parsing
//!
//! Handles parsing of `extension.toml` manifest files that describe extensions.

use super::error::{ExtensionError, ExtensionResult};
use serde::Deserialize;
use std::path::Path;

/// Extension manifest (`extension.toml`)
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionManifest {
    /// Core extension metadata
    pub extension: ExtensionInfo,

    /// Library/binary information
    pub lib: LibInfo,

    /// Capabilities this extension provides
    #[serde(default)]
    pub capabilities: Capabilities,

    /// Lifecycle-specific configuration (if lifecycle capability)
    #[serde(default)]
    pub lifecycle: Option<LifecycleConfig>,

    /// Provider-specific configuration (if provider capability)
    #[serde(default)]
    pub provider: Option<ProviderConfig>,

    /// Extension-specific settings (arbitrary TOML)
    #[serde(default = "default_settings")]
    pub settings: toml::Value,
}

/// Default settings value (empty table)
fn default_settings() -> toml::Value {
    toml::Value::Table(Default::default())
}

/// Core extension information
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionInfo {
    /// Unique extension identifier (e.g., "coder-lifecycle")
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Extension version (semver)
    pub version: String,

    /// API version this extension targets
    pub api_version: String,

    /// Description of what the extension does
    pub description: String,

    /// Extension authors
    #[serde(default)]
    pub authors: Vec<String>,

    /// Repository URL
    #[serde(default)]
    pub repository: Option<String>,
}

/// Library/binary information
#[derive(Debug, Clone, Deserialize)]
pub struct LibInfo {
    /// Kind of extension (e.g., "rust")
    pub kind: String,

    /// Path to the WASM file (relative to extension directory)
    pub path: String,
}

/// Capabilities the extension provides
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Capabilities {
    /// Provides lifecycle functionality (templates, classification)
    #[serde(default)]
    pub lifecycle: bool,

    /// Provides LLM provider functionality
    #[serde(default)]
    pub provider: bool,

    /// Provides tools functionality (future)
    #[serde(default)]
    pub tools: bool,

    /// Provides context functionality (future)
    #[serde(default)]
    pub context: bool,
}

/// Lifecycle-specific configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LifecycleConfig {
    /// Task types this lifecycle supports
    #[serde(default)]
    pub supported_task_types: Vec<String>,

    /// Templates provided by this lifecycle
    #[serde(default)]
    pub templates: Vec<String>,
}

/// Provider-specific configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderConfig {
    /// Backends this provider supports
    #[serde(default)]
    pub supported_backends: Vec<String>,

    /// Models this provider supports
    #[serde(default)]
    pub supported_models: Vec<String>,
}

impl ExtensionManifest {
    /// Parse a manifest from a file
    ///
    /// # Arguments
    /// * `path` - Path to the `extension.toml` file
    ///
    /// # Returns
    /// * `ExtensionResult<Self>` - Parsed manifest or error
    pub fn from_file(path: &Path) -> ExtensionResult<Self> {
        if !path.exists() {
            return Err(ExtensionError::ManifestNotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            ExtensionError::IoError(format!("Failed to read manifest {:?}: {}", path, e))
        })?;

        Self::from_str(&content)
    }

    /// Parse a manifest from a string
    ///
    /// # Arguments
    /// * `content` - TOML content
    ///
    /// # Returns
    /// * `ExtensionResult<Self>` - Parsed manifest or error
    pub fn from_str(content: &str) -> ExtensionResult<Self> {
        let manifest: ExtensionManifest = toml::from_str(content)?;

        // Validate required fields
        if manifest.extension.id.is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "extension.id is required".to_string(),
            ));
        }
        if manifest.extension.name.is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "extension.name is required".to_string(),
            ));
        }
        if manifest.extension.version.is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "extension.version is required".to_string(),
            ));
        }
        if manifest.extension.api_version.is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "extension.api_version is required".to_string(),
            ));
        }
        if manifest.lib.path.is_empty() {
            return Err(ExtensionError::InvalidManifest(
                "lib.path is required".to_string(),
            ));
        }

        Ok(manifest)
    }

    /// List capabilities this extension provides
    ///
    /// # Returns
    /// * `Vec<String>` - List of capability names
    pub fn list_capabilities(&self) -> Vec<String> {
        let mut caps = Vec::new();
        if self.capabilities.lifecycle {
            caps.push("lifecycle".to_string());
        }
        if self.capabilities.provider {
            caps.push("provider".to_string());
        }
        if self.capabilities.tools {
            caps.push("tools".to_string());
        }
        if self.capabilities.context {
            caps.push("context".to_string());
        }
        caps
    }

    /// Check if extension has a specific capability
    ///
    /// # Arguments
    /// * `capability` - Capability name to check
    ///
    /// # Returns
    /// * `bool` - True if extension has the capability
    pub fn has_capability(&self, capability: &str) -> bool {
        match capability {
            "lifecycle" => self.capabilities.lifecycle,
            "provider" => self.capabilities.provider,
            "tools" => self.capabilities.tools,
            "context" => self.capabilities.context,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = r#"
[extension]
id = "test-extension"
name = "Test Extension"
version = "0.1.0"
api_version = "0.3.0"
description = "A test extension"
authors = ["Test Author"]
repository = "https://github.com/test/extension"

[lib]
kind = "rust"
path = "extension.wasm"

[capabilities]
lifecycle = true
provider = false

[lifecycle]
supported_task_types = ["bug_fix", "feature"]
templates = ["system", "task/*"]

[settings]
custom_setting = "value"
"#;

    #[test]
    fn test_parse_valid_manifest() {
        let manifest = ExtensionManifest::from_str(VALID_MANIFEST).unwrap();

        assert_eq!(manifest.extension.id, "test-extension");
        assert_eq!(manifest.extension.name, "Test Extension");
        assert_eq!(manifest.extension.version, "0.1.0");
        assert_eq!(manifest.extension.api_version, "0.3.0");
        assert_eq!(manifest.extension.description, "A test extension");
        assert_eq!(manifest.extension.authors, vec!["Test Author"]);
        assert_eq!(
            manifest.extension.repository,
            Some("https://github.com/test/extension".to_string())
        );

        assert_eq!(manifest.lib.kind, "rust");
        assert_eq!(manifest.lib.path, "extension.wasm");

        assert!(manifest.capabilities.lifecycle);
        assert!(!manifest.capabilities.provider);
    }

    #[test]
    fn test_list_capabilities() {
        let manifest = ExtensionManifest::from_str(VALID_MANIFEST).unwrap();
        let caps = manifest.list_capabilities();

        assert_eq!(caps, vec!["lifecycle"]);
    }

    #[test]
    fn test_has_capability() {
        let manifest = ExtensionManifest::from_str(VALID_MANIFEST).unwrap();

        assert!(manifest.has_capability("lifecycle"));
        assert!(!manifest.has_capability("provider"));
        assert!(!manifest.has_capability("unknown"));
    }

    #[test]
    fn test_parse_missing_required_field() {
        let invalid = r#"
[extension]
name = "Test"
version = "0.1.0"
api_version = "0.3.0"
description = "Missing id"

[lib]
kind = "rust"
path = "extension.wasm"
"#;

        let result = ExtensionManifest::from_str(invalid);
        assert!(result.is_err());

        if let Err(ExtensionError::InvalidManifest(msg)) = result {
            assert!(msg.contains("id"));
        } else {
            panic!("Expected InvalidManifest error");
        }
    }

    #[test]
    fn test_parse_lifecycle_config() {
        let manifest = ExtensionManifest::from_str(VALID_MANIFEST).unwrap();

        let lifecycle = manifest.lifecycle.unwrap();
        assert_eq!(lifecycle.supported_task_types, vec!["bug_fix", "feature"]);
        assert_eq!(lifecycle.templates, vec!["system", "task/*"]);
    }

    #[test]
    fn test_parse_minimal_manifest() {
        let minimal = r#"
[extension]
id = "minimal"
name = "Minimal Extension"
version = "0.1.0"
api_version = "0.3.0"
description = "Minimal"

[lib]
kind = "rust"
path = "extension.wasm"
"#;

        let manifest = ExtensionManifest::from_str(minimal).unwrap();
        assert_eq!(manifest.extension.id, "minimal");
        assert!(manifest.list_capabilities().is_empty());
    }

    #[test]
    fn test_default_capabilities() {
        let minimal = r#"
[extension]
id = "minimal"
name = "Minimal"
version = "0.1.0"
api_version = "0.3.0"
description = "Minimal"

[lib]
kind = "rust"
path = "extension.wasm"
"#;

        let manifest = ExtensionManifest::from_str(minimal).unwrap();

        assert!(!manifest.capabilities.lifecycle);
        assert!(!manifest.capabilities.provider);
        assert!(!manifest.capabilities.tools);
        assert!(!manifest.capabilities.context);
    }
}
