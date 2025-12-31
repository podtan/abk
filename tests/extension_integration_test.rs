//! Extension integration tests
//!
//! These tests verify the extension system works correctly.

use abk::extension::{ExtensionManager, ExtensionManifest, ExtensionRegistry};
use std::path::PathBuf;
use tempfile::TempDir;

/// Test discovering extensions from a directory
#[tokio::test]
async fn test_discover_multiple_extensions() {
    let temp_dir = TempDir::new().unwrap();

    // Create first extension
    let ext1_dir = temp_dir.path().join("lifecycle-ext");
    std::fs::create_dir(&ext1_dir).unwrap();
    std::fs::write(
        ext1_dir.join("extension.toml"),
        r#"
[extension]
id = "lifecycle-ext"
name = "Lifecycle Extension"
version = "0.1.0"
api_version = "0.3.0"
description = "A lifecycle extension"

[lib]
kind = "rust"
path = "extension.wasm"

[capabilities]
lifecycle = true
"#,
    )
    .unwrap();

    // Create second extension
    let ext2_dir = temp_dir.path().join("provider-ext");
    std::fs::create_dir(&ext2_dir).unwrap();
    std::fs::write(
        ext2_dir.join("extension.toml"),
        r#"
[extension]
id = "provider-ext"
name = "Provider Extension"
version = "0.2.0"
api_version = "0.3.0"
description = "A provider extension"

[lib]
kind = "rust"
path = "extension.wasm"

[capabilities]
provider = true
"#,
    )
    .unwrap();

    // Discover
    let mut manager = ExtensionManager::new(temp_dir.path()).await.unwrap();
    let manifests = manager.discover().await.unwrap();

    assert_eq!(manifests.len(), 2);

    // Check lifecycle extensions
    let lifecycles = manager.get_lifecycles();
    assert_eq!(lifecycles.len(), 1);
    assert_eq!(lifecycles[0].extension.id, "lifecycle-ext");

    // Check provider extensions
    let providers = manager.get_providers();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].extension.id, "provider-ext");
}

/// Test extension manifest validation
#[test]
fn test_manifest_validation() {
    // Valid manifest
    let valid = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
api_version = "0.3.0"
description = "Test"

[lib]
kind = "rust"
path = "test.wasm"

[capabilities]
lifecycle = true
"#;

    let manifest: Result<ExtensionManifest, _> = toml::from_str(valid);
    assert!(manifest.is_ok());

    // Invalid manifest - missing id
    let invalid = r#"
[extension]
name = "Test"
version = "0.1.0"
api_version = "0.3.0"
description = "Test"

[lib]
kind = "rust"
path = "test.wasm"
"#;

    let manifest: Result<ExtensionManifest, _> = toml::from_str(invalid);
    assert!(manifest.is_err());
}

/// Test capability listing
#[test]
fn test_capability_listing() {
    let manifest_str = r#"
[extension]
id = "multi-cap"
name = "Multi Capability"
version = "0.1.0"
api_version = "0.3.0"
description = "Extension with multiple capabilities"

[lib]
kind = "rust"
path = "extension.wasm"

[capabilities]
lifecycle = true
provider = true
tools = true
"#;

    let manifest: ExtensionManifest = toml::from_str(manifest_str).unwrap();
    let caps = manifest.list_capabilities();

    assert!(caps.contains(&"lifecycle".to_string()));
    assert!(caps.contains(&"provider".to_string()));
    assert!(caps.contains(&"tools".to_string()));
    assert!(!caps.contains(&"context".to_string()));
}

/// Test registry capability indexing
#[test]
fn test_registry_capability_index() {
    let mut registry = ExtensionRegistry::new();

    // Register extensions
    let manifest1: ExtensionManifest = toml::from_str(
        r#"
[extension]
id = "ext1"
name = "Extension 1"
version = "0.1.0"
api_version = "0.3.0"
description = "First"

[lib]
kind = "rust"
path = "ext1.wasm"

[capabilities]
lifecycle = true
"#,
    )
    .unwrap();

    let manifest2: ExtensionManifest = toml::from_str(
        r#"
[extension]
id = "ext2"
name = "Extension 2"
version = "0.1.0"
api_version = "0.3.0"
description = "Second"

[lib]
kind = "rust"
path = "ext2.wasm"

[capabilities]
provider = true
"#,
    )
    .unwrap();

    let manifest3: ExtensionManifest = toml::from_str(
        r#"
[extension]
id = "ext3"
name = "Extension 3"
version = "0.1.0"
api_version = "0.3.0"
description = "Third"

[lib]
kind = "rust"
path = "ext3.wasm"

[capabilities]
lifecycle = true
provider = true
"#,
    )
    .unwrap();

    registry.register(manifest1, PathBuf::from("/ext1"));
    registry.register(manifest2, PathBuf::from("/ext2"));
    registry.register(manifest3, PathBuf::from("/ext3"));

    // Check counts
    assert_eq!(registry.count(), 3);

    // Check lifecycle extensions
    let lifecycles = registry.get_by_capability("lifecycle");
    assert_eq!(lifecycles.len(), 2);

    // Check provider extensions
    let providers = registry.get_by_capability("provider");
    assert_eq!(providers.len(), 2);

    // Check tools extensions (none)
    let tools = registry.get_by_capability("tools");
    assert!(tools.is_empty());
}

/// Test loading with fixtures
#[tokio::test]
async fn test_load_fixture_manifest() {
    // Use the test fixture
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");

    let mut manager = ExtensionManager::new(&fixture_path).await.unwrap();
    let manifests = manager.discover().await.unwrap();

    // Should find the test-extension
    assert!(!manifests.is_empty());

    let test_ext = manifests
        .iter()
        .find(|m| m.extension.id == "test-extension");
    assert!(test_ext.is_some());

    let ext = test_ext.unwrap();
    assert_eq!(ext.extension.name, "Test Extension");
    assert_eq!(ext.extension.version, "0.1.0");
    assert!(ext.capabilities.lifecycle);
    assert!(!ext.capabilities.provider);
}
