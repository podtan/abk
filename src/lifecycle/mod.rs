//! Lifecycle WASM Plugin Loader
//!
//! This module loads and interfaces with lifecycle extensions which handle
//! template management, task classification, and system information gathering.
//!
//! # Modes
//!
//! - **WASM Extension**: Full lifecycle with templates and task classification
//! - **Built-in Simple**: Minimal lifecycle without classification (when `lifecycle.enabled = false`)

use anyhow::{Context, Result};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;

use crate::extension::{LifecycleExtensionInstance, ExtensionManager};

macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG LIFECYCLE] {}", format!($($arg)*));
        }
    };
}

/// Lifecycle trait for polymorphic lifecycle handling
pub trait Lifecycle: Send + Sync {
    /// Load a template by name
    fn load_template(&self, name: &str) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
    
    /// Render a template with variable substitution
    fn render_template(&self, template: &str, variables: &[(String, String)]) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
    
    /// Classify a task (returns "query" for simple lifecycle)
    fn classify_task(&self, task_description: &str) -> Pin<Box<dyn Future<Output = Result<(String, f32)>> + Send + '_>>;
    
    /// Get system info variables
    fn get_system_info_variables(&self) -> Pin<Box<dyn Future<Output = Result<Vec<(String, String)>>> + Send + '_>>;
    
    /// Load useful commands
    fn load_useful_commands(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
    
    /// Get metadata
    fn get_metadata(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>>;
}

/// WASM-based lifecycle extension wrapper
pub struct WasmLifecycle {
    instance: Mutex<LifecycleExtensionInstance>,
    #[allow(dead_code)]
    extension_path: PathBuf,
}

impl WasmLifecycle {
    pub async fn new(extension_dir: PathBuf) -> Result<Self> {
        debug!("Creating lifecycle plugin from: {}", extension_dir.display());

        let mut instance = create_standalone_instance(&extension_dir)?;
        instance.init().context("Failed to initialize lifecycle extension")?;

        let caps = instance.list_capabilities()
            .context("Failed to list capabilities")?;
        if !caps.iter().any(|c| c == "lifecycle") {
            anyhow::bail!("Extension at '{}' does not have lifecycle capability", extension_dir.display());
        }

        debug!("Lifecycle plugin compiled successfully");

        Ok(Self {
            instance: Mutex::new(instance),
            extension_path: extension_dir,
        })
    }
}

impl Lifecycle for WasmLifecycle {
    fn load_template(&self, template_name: &str) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let template_name = template_name.to_string();
        Box::pin(async move {
            debug!("Loading template: {}", template_name);
            let result = self.instance.lock().unwrap().load_template(&template_name)
                .context(format!("Failed to load template: {}", template_name))?;
            debug!("Template loaded successfully: {} ({} bytes)", template_name, result.len());
            Ok(result)
        })
    }

    fn render_template(&self, template_content: &str, variables: &[(String, String)]) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let template_content = template_content.to_string();
        let variables = variables.to_vec();
        Box::pin(async move {
            debug!("Rendering template with {} variables", variables.len());
            let result = self.instance.lock().unwrap().render_template(&template_content, &variables)
                .context("Failed to render template")?;
            debug!("Template rendered successfully");
            Ok(result)
        })
    }

    fn classify_task(&self, task_description: &str) -> Pin<Box<dyn Future<Output = Result<(String, f32)>> + Send + '_>> {
        let task_description = task_description.to_string();
        Box::pin(async move {
            debug!("Classifying task: {}", task_description);
            let (task_type, confidence) = self.instance.lock().unwrap().classify_task(&task_description)
                .context("Failed to classify task")?;
            debug!("Task classified as: {} (confidence: {})", task_type, confidence);
            Ok((task_type, confidence))
        })
    }

    fn get_system_info_variables(&self) -> Pin<Box<dyn Future<Output = Result<Vec<(String, String)>>> + Send + '_>> {
        Box::pin(async move {
            debug!("Getting system info variables");
            let vars = self.instance.lock().unwrap().get_system_info_variables()
                .context("Failed to get system info variables")?;
            debug!("Retrieved {} system info variables", vars.len());
            Ok(vars)
        })
    }

    fn load_useful_commands(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        Box::pin(async move {
            debug!("Loading useful commands");
            let result = self.instance.lock().unwrap().load_useful_commands()
                .context("Failed to load useful commands")?;
            debug!("Useful commands loaded successfully ({} bytes)", result.len());
            Ok(result)
        })
    }

    fn get_metadata(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        Box::pin(async move {
            debug!("Getting lifecycle plugin metadata");
            let metadata = self.instance.lock().unwrap().get_metadata()
                .context("Failed to get metadata")?;
            let json = serde_json::json!({
                "id": metadata.id,
                "name": metadata.name,
                "version": metadata.version,
                "api_version": metadata.api_version,
                "description": metadata.description,
            });
            debug!("Metadata retrieved");
            Ok(json.to_string())
        })
    }
}

/// Simple built-in lifecycle (no classification, no templates)
pub struct SimpleLifecycle;

impl SimpleLifecycle {
    pub fn new() -> Self {
        debug!("Using simple built-in lifecycle (no WASM extension)");
        Self
    }
}

impl Default for SimpleLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

const SIMPLE_SYSTEM_TEMPLATE: &str = r#"You are a helpful AI assistant. Follow the user's instructions directly.

## Tools

You have access to tools. Use them when needed to accomplish tasks.

## Workflow

1. Understand what the user wants
2. Execute the necessary actions using available tools
3. Provide clear, helpful responses

When you have completed the task, use the `submit` tool:

```json
{"name": "submit", "arguments": {}}
```
"#;

const SIMPLE_TASK_TEMPLATE: &str = r#"## Current Task

{{task}}

{{#if additional_context}}
## Additional Context

{{additional_context}}
{{/if}}

## Instructions

Complete this task. Use available tools as needed. When done, call the `submit` tool."#;

impl Lifecycle for SimpleLifecycle {
    fn load_template(&self, name: &str) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let name = name.to_string();
        Box::pin(async move {
            match name.as_str() {
                "system" => Ok(SIMPLE_SYSTEM_TEMPLATE.to_string()),
                "task/query" | "task/feature" | "task/bug_fix" | "task/maintenance" | "task/fallback" => {
                    Ok(SIMPLE_TASK_TEMPLATE.to_string())
                }
                _ => Ok(String::new()),
            }
        })
    }

    fn render_template(&self, template: &str, variables: &[(String, String)]) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        let template = template.to_string();
        let variables = variables.to_vec();
        Box::pin(async move {
            let mut result = template;
            for (key, value) in variables {
                result = result.replace(&format!("{{{{{}}}}}", key), &value);
            }
            Ok(result)
        })
    }

    fn classify_task(&self, _task_description: &str) -> Pin<Box<dyn Future<Output = Result<(String, f32)>> + Send + '_>> {
        Box::pin(async move {
            Ok(("query".to_string(), 1.0))
        })
    }

    fn get_system_info_variables(&self) -> Pin<Box<dyn Future<Output = Result<Vec<(String, String)>>> + Send + '_>> {
        Box::pin(async move {
            Ok(vec![
                ("working_directory".to_string(), std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()),
            ])
        })
    }

    fn load_useful_commands(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        Box::pin(async move {
            Ok("Use available tools to complete tasks.".to_string())
        })
    }

    fn get_metadata(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send + '_>> {
        Box::pin(async move {
            Ok(r#"{"id": "simple-lifecycle", "name": "Simple Lifecycle", "version": "0.1.0", "description": "Built-in simple lifecycle without classification"}"#.to_string())
        })
    }
}

/// Legacy LifecyclePlugin type alias for backward compatibility
pub type LifecyclePlugin = WasmLifecycle;

fn create_standalone_instance(extension_dir: &PathBuf) -> Result<LifecycleExtensionInstance> {
    use crate::extension::ExtensionManifest;
    use std::sync::Arc;
    use wasmtime::component::Component;
    use wasmtime::{Config, Engine};

    let manifest_path = extension_dir.join("extension.toml");
    debug!("Loading manifest from: {}", manifest_path.display());
    let manifest = ExtensionManifest::from_file(&manifest_path)
        .with_context(|| format!("Failed to load extension manifest: {}", manifest_path.display()))?;
    debug!("Manifest loaded: {} v{}", manifest.extension.name, manifest.extension.version);

    let mut config = Config::new();
    config.wasm_component_model(true);
    debug!("Creating WASM engine (sync mode)...");
    let engine = Arc::new(Engine::new(&config)
        .context("Failed to create WASM engine for lifecycle")?);
    debug!("WASM engine created");

    let wasm_path = extension_dir.join(&manifest.lib.path);
    debug!("Loading WASM component from: {}", wasm_path.display());
    let wasm_bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))?;
    debug!("Read {} bytes from WASM file", wasm_bytes.len());
    
    let component = Component::from_binary(&engine, &wasm_bytes)
        .with_context(|| format!("Failed to parse WASM component: {}", wasm_path.display()))?;
    debug!("WASM component parsed successfully");

    debug!("Creating LifecycleExtensionInstance...");
    let instance = LifecycleExtensionInstance::new(&engine, &component)
        .with_context(|| "Failed to create lifecycle extension instance from WASM component")?;
    debug!("LifecycleExtensionInstance created successfully");

    Ok(instance)
}

trait ExpandHome {
    fn expand_home(&self) -> Result<PathBuf>;
}

impl ExpandHome for PathBuf {
    fn expand_home(&self) -> Result<PathBuf> {
        if let Some(path_str) = self.to_str() {
            if path_str.starts_with("~") {
                if let Ok(home) = std::env::var("HOME") {
                    let expanded = path_str.replacen("~", &home, 1);
                    return Ok(PathBuf::from(expanded));
                }
            }
        }
        Ok(self.clone())
    }
}

/// Find and load the lifecycle extension (WASM or simple built-in)
/// 
/// If `lifecycle_enabled` is false, returns the simple built-in lifecycle.
/// Otherwise searches for WASM extension.
pub async fn find_lifecycle_plugin_with_config(lifecycle_enabled: bool) -> Result<Box<dyn Lifecycle>> {
    if !lifecycle_enabled {
        return Ok(Box::new(SimpleLifecycle::new()));
    }
    
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
    
    let extension_paths = vec![
        PathBuf::from("extensions/coder-lifecycle"),
        PathBuf::from(format!("~/.{}/extensions/coder-lifecycle", agent_name)).expand_home()?,
    ];

    for path in &extension_paths {
        let manifest_path = path.join("extension.toml");
        if manifest_path.exists() {
            debug!("Found lifecycle extension at: {}", path.display());
            return Ok(Box::new(WasmLifecycle::new(path.clone()).await?));
        }
    }

    // Legacy paths
    let legacy_paths = vec![
        PathBuf::from("providers/lifecycle"),
        PathBuf::from(format!("~/.{}/providers/lifecycle", agent_name)).expand_home()?,
    ];

    for path in &legacy_paths {
        let manifest_path = path.join("extension.toml");
        if manifest_path.exists() {
            debug!("Found legacy lifecycle extension at: {}", path.display());
            eprintln!("[WARN] Using deprecated lifecycle plugin location. Please migrate to ~/.{}/extensions/coder-lifecycle/", agent_name);
            return Ok(Box::new(WasmLifecycle::new(path.clone()).await?));
        }
        
        let wasm_path = path.join("lifecycle.wasm");
        if wasm_path.exists() {
            anyhow::bail!(
                "Found old-format lifecycle plugin at {}. \n\
                The new extension system requires an extension.toml manifest.\n\
                Please update to the new coder-lifecycle-wasm extension format.\n\
                See: https://github.com/podtan/coder-lifecycle-wasm",
                wasm_path.display()
            );
        }
    }

    // Not found, fall back to simple lifecycle
    debug!("Lifecycle extension not found, using simple built-in lifecycle");
    Ok(Box::new(SimpleLifecycle::new()))
}

/// Legacy function for backward compatibility (always tries to load WASM)
pub async fn find_lifecycle_plugin() -> Result<LifecyclePlugin> {
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
    
    let extension_paths = vec![
        PathBuf::from("extensions/coder-lifecycle"),
        PathBuf::from(format!("~/.{}/extensions/coder-lifecycle", agent_name)).expand_home()?,
    ];

    for path in &extension_paths {
        let manifest_path = path.join("extension.toml");
        if manifest_path.exists() {
            debug!("Found lifecycle extension at: {}", path.display());
            return LifecyclePlugin::new(path.clone()).await;
        }
    }

    anyhow::bail!(
        "Lifecycle extension not found. Expected at one of:\n  \
        - extensions/coder-lifecycle/extension.toml\n  \
        - ~/.{}/extensions/coder-lifecycle/extension.toml\n\n\
        Install with: trustee extension install <path-to-coder-lifecycle-wasm>\n\
        Or set lifecycle.enabled = false in config to use simple built-in lifecycle.",
        agent_name
    )
}
