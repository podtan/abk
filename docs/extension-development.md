# ABK Extension Development Guide

This guide explains how to create WASM extensions for ABK (Agent Builder Kit). Extensions allow you to add new capabilities to ABK-based agents without modifying the core ABK code.

## Overview

ABK uses a **WASM Component Model** based extension system. Extensions are WebAssembly modules that implement specific interfaces defined using WIT (WebAssembly Interface Types).

### Extension Worlds

ABK defines three WIT worlds for different extension types:

| World | Required Interfaces | Use Case |
|-------|---------------------|----------|
| `extension` | `core` + `lifecycle` + `provider` | Full-featured extensions |
| `lifecycle-extension` | `core` + `lifecycle` | Lifecycle-only extensions (templates, classification) |
| `provider-extension` | `core` + `provider` | LLM provider extensions |

### Extension Capabilities

Extensions can provide one or more capabilities:

| Capability | Description |
|------------|-------------|
| `lifecycle` | Template management, task classification, workflow control |
| `provider` | LLM API communication |
| `tools` | Custom tools for agents (future) |
| `context` | Context providers (future) |

## Quick Start

### 1. Create Extension Structure

```
my-extension/
├── extension.toml     # Extension manifest (required)
├── src/
│   └── lib.rs         # Rust implementation
├── Cargo.toml         # Rust project config
├── build.sh           # Build script
└── wit/               # WIT interface definitions (optional)
```

### 2. Create `extension.toml` Manifest

```toml
[extension]
id = "my-extension"
name = "My Extension"
version = "0.1.0"
api_version = "0.3.0"
description = "Description of what your extension does"
authors = ["Your Name"]
repository = "https://github.com/your/repo"

[lib]
kind = "rust"
path = "my_extension.wasm"

[capabilities]
lifecycle = true    # Enable lifecycle capability
provider = false    # Disable provider capability
tools = false       # Disable tools capability
context = false     # Disable context capability

# Lifecycle-specific configuration (if lifecycle = true)
[lifecycle]
supported_task_types = ["bug_fix", "feature", "maintenance", "query", "fallback"]
templates = [
    "system",
    "task/bug_fix",
    "task/feature"
]
```

### 3. Set Up Cargo.toml

```toml
[package]
name = "my-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = { version = "0.36", default-features = false, features = ["realloc"] }

[profile.release]
opt-level = "s"
lto = true
strip = true
```

### 4. Implement the Extension

For lifecycle-only extensions, use the `lifecycle-extension` world:

```rust
// src/lib.rs

// Generate bindings from WIT interface
// Use "lifecycle-extension" world for lifecycle-only extensions
wit_bindgen::generate!({
    world: "lifecycle-extension",
    path: "../abk/wit/extension",
});

// Export the extension implementation
export!(MyExtension);

struct MyExtension;

impl Guest for MyExtension {
    // === Core Interface (Required) ===
    
    fn get_metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            id: "my-extension".to_string(),
            name: "My Extension".to_string(),
            version: "0.1.0".to_string(),
            api_version: "0.3.0".to_string(),
            description: "My custom extension".to_string(),
        }
    }
    
    fn list_capabilities() -> Vec<String> {
        vec!["lifecycle".to_string()]
    }
    
    fn init() -> Result<(), String> {
        // Initialize extension state if needed
        Ok(())
    }
}

// Lifecycle implementation (if capability enabled)
impl exports::abk::extension::lifecycle::Guest for MyExtension {
    fn load_template(template_name: String) -> Result<String, LifecycleError> {
        match template_name.as_str() {
            "system" => Ok(include_str!("../templates/system.md").to_string()),
            "task/bug_fix" => Ok(include_str!("../templates/task_bug_fix.md").to_string()),
            _ => Err(LifecycleError {
                message: format!("Template not found: {}", template_name),
                code: Some("NOT_FOUND".to_string()),
            }),
        }
    }
    
    fn render_template(content: String, variables: Vec<TemplateVariable>) -> String {
        let mut result = content;
        for var in variables {
            result = result.replace(&format!("{{{}}}", var.key), &var.value);
        }
        result
    }
    
    fn extract_sections(content: String) -> Vec<TemplateSection> {
        // Parse markdown sections
        vec![]
    }
    
    fn validate_template_variables(content: String) -> Vec<String> {
        // Extract {variable} placeholders
        vec![]
    }
    
    fn get_task_template_name(task_type: String) -> String {
        format!("task/{}", task_type)
    }
    
    fn classify_task(task_description: String) -> Result<TaskClassification, LifecycleError> {
        // Simple keyword-based classification
        let task_type = if task_description.contains("bug") || task_description.contains("fix") {
            "bug_fix"
        } else if task_description.contains("add") || task_description.contains("feature") {
            "feature"
        } else {
            "query"
        };
        
        Ok(TaskClassification {
            task_type: task_type.to_string(),
            confidence: 0.8,
        })
    }
    
    fn load_useful_commands() -> Result<String, LifecycleError> {
        Ok("# Available Commands\n...".to_string())
    }
    
    fn get_system_info_variables() -> Vec<TemplateVariable> {
        vec![
            TemplateVariable {
                key: "os".to_string(),
                value: std::env::consts::OS.to_string(),
            },
            TemplateVariable {
                key: "arch".to_string(),
                value: std::env::consts::ARCH.to_string(),
            },
        ]
    }
}
```

### 5. Build the Extension

**Important**: WASM modules must be converted to WASM components using `wasm-tools`:

```bash
#!/bin/bash
# build.sh

set -e

# Build for WASM
cargo build --target wasm32-wasip1 --release

# Get WASI adapter
WASI_ADAPTER="wasi_snapshot_preview1.reactor.wasm"
if [ ! -f "$WASI_ADAPTER" ]; then
    echo "Downloading WASI adapter..."
    curl -LO "https://github.com/bytecodealliance/wasmtime/releases/download/v25.0.0/$WASI_ADAPTER"
fi

# Convert module to component
wasm-tools component new \
    target/wasm32-wasip1/release/my_extension.wasm \
    -o my_extension.wasm \
    --adapt "$WASI_ADAPTER"

echo "Built: my_extension.wasm"
```

> **Note**: ABK requires WASM **components** (not modules). The `wasm-tools component new` command converts your Rust WASM module to a proper component with WASI support.

## WIT Interface Reference

### Core Interface (Required)

All extensions must implement the `core` interface:

```wit
interface core {
    record extension-metadata {
        id: string,
        name: string,
        version: string,
        api-version: string,
        description: string,
    }
    
    get-metadata: func() -> extension-metadata;
    list-capabilities: func() -> list<string>;
    init: func() -> result<_, string>;
}
```

### Lifecycle Interface

Extensions with `lifecycle` capability implement:

```wit
interface lifecycle {
    record template-variable {
        key: string,
        value: string,
    }

    record template-section {
        name: string,
        content: string,
    }

    record lifecycle-error {
        message: string,
        code: option<string>,
    }

    record task-classification {
        task-type: string,
        confidence: f32,
    }

    load-template: func(template-name: string) -> result<string, lifecycle-error>;
    render-template: func(content: string, variables: list<template-variable>) -> string;
    extract-sections: func(content: string) -> list<template-section>;
    validate-template-variables: func(content: string) -> list<string>;
    get-task-template-name: func(task-type: string) -> string;
    classify-task: func(task-description: string) -> result<task-classification, lifecycle-error>;
    load-useful-commands: func() -> result<string, lifecycle-error>;
    get-system-info-variables: func() -> list<template-variable>;
}
```

## Installation

### User Installation

Copy the extension directory to `~/.{agent}/extensions/`:

```bash
# Example for trustee agent
cp -r my-extension ~/.trustee/extensions/
```

### Development Installation

During development, use `{agent} init --force` to copy extensions from your project's `extensions/` directory to the agent's home directory.

## Extension Discovery

ABK discovers extensions by:

1. Scanning `~/.{agent}/extensions/` directory
2. Looking for subdirectories containing `extension.toml`
3. Parsing manifest and loading WASM module on demand

## Best Practices

### 1. Keep Extensions Focused

Each extension should provide a single, well-defined capability. Don't try to do everything in one extension.

### 2. Handle Errors Gracefully

Always return meaningful error messages in `LifecycleError`:

```rust
Err(LifecycleError {
    message: format!("Failed to load template '{}': file not found", name),
    code: Some("TEMPLATE_NOT_FOUND".to_string()),
})
```

### 3. Embed Templates at Compile Time

Use `include_str!` to embed templates in the WASM binary:

```rust
const SYSTEM_TEMPLATE: &str = include_str!("../templates/system.md");
```

### 4. Optimize WASM Size

Use release profile optimizations:

```toml
[profile.release]
opt-level = "s"      # Size optimization
lto = true           # Link-time optimization
strip = true         # Strip debug symbols
codegen-units = 1    # Better optimization
```

### 5. Version Your API

Match your `api_version` in `extension.toml` with the ABK version you're targeting:

```toml
[extension]
api_version = "0.3.0"  # Must match ABK's extension API version
```

## Example: Coder Lifecycle Extension

See the [coder-lifecycle-wasm](https://github.com/podtan/coder-lifecycle-wasm) repository for a complete example of a lifecycle extension that provides:

- Task classification (bug_fix, feature, maintenance, query)
- Template management for coding workflows
- System information variables
- Useful commands documentation

## Debugging Extensions

### Check Extension Discovery

```bash
trustee extension list
```

### Verbose Logging

Set `RUST_LOG=debug` to see extension loading logs:

```bash
RUST_LOG=debug trustee run "your task"
```

### Common Issues

| Issue | Solution |
|-------|----------|
| Extension not discovered | Check `extension.toml` exists and is valid TOML |
| WASM load failure | Ensure WASM targets `wasm32-wasip1` |
| Interface mismatch | Check `api_version` matches ABK version |
| Template not found | Verify template name matches what's registered |

## API Reference

### Instance Types

ABK provides different instance types for different extension worlds:

| Type | World | Use Case |
|------|-------|----------|
| `ExtensionInstance` | `extension` | Full extensions (all interfaces) |
| `LifecycleExtensionInstance` | `lifecycle-extension` | Lifecycle-only extensions |
| `ProviderExtensionInstance` | `provider-extension` | Provider-only extensions |

### ExtensionManager

```rust
use abk::extension::ExtensionManager;

// Create manager
let mut manager = ExtensionManager::new("~/.trustee/extensions").await?;

// Discover extensions
let manifests = manager.discover().await?;

// Get by capability
let lifecycles = manager.get_by_capability("lifecycle");

// Instantiate extension
let instance = manager.instantiate("coder-lifecycle")?;

// Call lifecycle method
let template = instance.lifecycle().load_template("system")?;
```

### LifecycleExtensionInstance (Lifecycle-only)

```rust
use abk::extension::{LifecycleExtensionInstance, ExtensionManifest};
use wasmtime::{Engine, Config};
use wasmtime::component::Component;
use std::sync::Arc;

// Create engine WITHOUT async_support (sync bindings)
let mut config = Config::new();
config.wasm_component_model(true);
// Note: Do NOT enable async_support for lifecycle extensions
let engine = Arc::new(Engine::new(&config)?);

// Load WASM component
let wasm_bytes = std::fs::read("extension.wasm")?;
let component = Component::from_binary(&engine, &wasm_bytes)?;

// Create instance
let mut instance = LifecycleExtensionInstance::new(&engine, &component)?;

// Initialize
instance.init()?;

// Use lifecycle methods
let template = instance.load_template("system")?;
let rendered = instance.render_template(&template, &[
    ("task".to_string(), "fix the bug".to_string()),
])?;
let (task_type, confidence) = instance.classify_task("fix the login bug")?;
```

### ExtensionManifest

```rust
use abk::extension::ExtensionManifest;

// Load from file
let manifest = ExtensionManifest::from_file("extension.toml")?;

// Check capabilities
if manifest.capabilities.lifecycle {
    // Extension provides lifecycle capability
}

// Get extension info
println!("Extension: {} v{}", manifest.extension.name, manifest.extension.version);
```

## Contributing

Extensions can be shared with the community:

1. Create a public repository
2. Add clear documentation and examples
3. Tag releases with semantic versions
4. Submit to the ABK extension registry (coming soon)

## See Also

- [ABK Changelog](../CHANGELOG.md) - Version history
- [Checkpoint Format](checkpoint-format.md) - Checkpoint system documentation
- [WIT Specification](https://component-model.bytecodealliance.org/design/wit.html) - WebAssembly Interface Types
