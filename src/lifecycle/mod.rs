//! Lifecycle WASM Plugin Loader
//!
//! This module loads and interfaces with lifecycle extensions which handle
//! template management, task classification, and system information gathering.
//!
//! All templates are embedded in the WASM extension, eliminating filesystem dependencies.
//!
//! # Extension System
//!
//! This module now uses the new ABK extension system (`abk:extension@0.3.0`).
//! Lifecycle extensions are discovered via `ExtensionManager` and provide
//! the `lifecycle` capability.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::path::PathBuf;

use crate::extension::{LifecycleExtensionInstance, ExtensionManager};

/// Conditional debug macro
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG LIFECYCLE] {}", format!($($arg)*));
        }
    };
}

/// Lifecycle WASM extension wrapper
///
/// This is the public API for lifecycle functionality. Internally it uses
/// the new extension system (`ExtensionManager` and `LifecycleExtensionInstance`).
///
/// Uses interior mutability (`RefCell`) to allow `&self` methods while
/// the underlying `LifecycleExtensionInstance` requires `&mut self`.
pub struct LifecyclePlugin {
    /// Extension instance for lifecycle calls (interior mutability for &self methods)
    instance: RefCell<LifecycleExtensionInstance>,

    /// Path to the extension (for debugging)
    #[allow(dead_code)]
    extension_path: PathBuf,
}

impl LifecyclePlugin {
    /// Create a new lifecycle plugin from an extension directory
    ///
    /// # Arguments
    /// * `extension_dir` - Path to extension directory containing extension.toml
    pub async fn new(extension_dir: PathBuf) -> Result<Self> {
        debug!("Creating lifecycle plugin from: {}", extension_dir.display());

        // Create standalone instance directly (more efficient than using manager)
        let mut instance = create_standalone_instance(&extension_dir)?;

        // Initialize the extension
        instance.init()
            .context("Failed to initialize lifecycle extension")?;

        // Verify it has lifecycle capability
        let caps = instance.list_capabilities()
            .context("Failed to list capabilities")?;
        if !caps.iter().any(|c| c == "lifecycle") {
            anyhow::bail!("Extension at '{}' does not have lifecycle capability", extension_dir.display());
        }

        debug!("Lifecycle plugin compiled successfully");

        Ok(Self {
            instance: RefCell::new(instance),
            extension_path: extension_dir,
        })
    }

    /// Load a template by name
    ///
    /// # Arguments
    /// * `template_name` - Template identifier (e.g., "system", "task/bug_fix")
    ///
    /// # Returns
    /// Template content as markdown string
    pub async fn load_template(&self, template_name: &str) -> Result<String> {
        debug!("Loading template: {}", template_name);

        let result = self.instance.borrow_mut().load_template(template_name)
            .context(format!("Failed to load template: {}", template_name))?;

        debug!("Template loaded successfully: {} ({} bytes)", template_name, result.len());
        Ok(result)
    }

    /// Render a template with variable substitution
    ///
    /// # Arguments
    /// * `template_content` - Template with {variable} placeholders
    /// * `variables` - Key-value pairs for substitution
    ///
    /// # Returns
    /// Rendered template content
    pub async fn render_template(
        &self,
        template_content: &str,
        variables: &[(String, String)],
    ) -> Result<String> {
        debug!("Rendering template with {} variables", variables.len());

        let result = self.instance.borrow_mut().render_template(template_content, variables)
            .context("Failed to render template")?;

        debug!("Template rendered successfully");
        Ok(result)
    }

    /// Classify a task based on description
    ///
    /// # Arguments
    /// * `task_description` - User's task description
    ///
    /// # Returns
    /// (task_type, confidence) tuple
    pub async fn classify_task(&self, task_description: &str) -> Result<(String, f32)> {
        debug!("Classifying task: {}", task_description);

        let (task_type, confidence) = self.instance.borrow_mut().classify_task(task_description)
            .context("Failed to classify task")?;

        debug!("Task classified as: {} (confidence: {})", task_type, confidence);
        Ok((task_type, confidence))
    }

    /// Get system information variables
    ///
    /// # Returns
    /// Vector of (key, value) tuples for system info
    pub async fn get_system_info_variables(&self) -> Result<Vec<(String, String)>> {
        debug!("Getting system info variables");

        let vars = self.instance.borrow_mut().get_system_info_variables()
            .context("Failed to get system info variables")?;

        debug!("Retrieved {} system info variables", vars.len());
        Ok(vars)
    }

    /// Load useful commands content
    ///
    /// # Returns
    /// Markdown content with available tools and commands
    pub async fn load_useful_commands(&self) -> Result<String> {
        debug!("Loading useful commands");

        let result = self.instance.borrow_mut().load_useful_commands()
            .context("Failed to load useful commands")?;

        debug!("Useful commands loaded successfully ({} bytes)", result.len());
        Ok(result)
    }

    /// Get plugin metadata
    pub async fn get_metadata(&self) -> Result<String> {
        debug!("Getting lifecycle plugin metadata");

        let metadata = self.instance.borrow_mut().get_metadata()
            .context("Failed to get metadata")?;

        // Convert ExtensionMetadata to JSON string for compatibility
        let json = serde_json::json!({
            "id": metadata.id,
            "name": metadata.name,
            "version": metadata.version,
            "api_version": metadata.api_version,
            "description": metadata.description,
        });

        debug!("Metadata retrieved");
        Ok(json.to_string())
    }
}

/// Create a standalone LifecycleExtensionInstance for a lifecycle extension
fn create_standalone_instance(extension_dir: &PathBuf) -> Result<LifecycleExtensionInstance> {
    use crate::extension::ExtensionManifest;
    use std::sync::Arc;
    use wasmtime::component::Component;
    use wasmtime::{Config, Engine};

    // Load manifest
    let manifest_path = extension_dir.join("extension.toml");
    debug!("Loading manifest from: {}", manifest_path.display());
    let manifest = ExtensionManifest::from_file(&manifest_path)
        .with_context(|| format!("Failed to load extension manifest: {}", manifest_path.display()))?;
    debug!("Manifest loaded: {} v{}", manifest.extension.name, manifest.extension.version);

    // Create engine WITHOUT async support (sync bindings for lifecycle extensions)
    let mut config = Config::new();
    config.wasm_component_model(true);
    // Note: async_support is NOT enabled for lifecycle extensions
    // because they use sync bindgen
    debug!("Creating WASM engine (sync mode)...");
    let engine = Arc::new(Engine::new(&config)
        .context("Failed to create WASM engine for lifecycle")?);
    debug!("WASM engine created");

    // Load component
    let wasm_path = extension_dir.join(&manifest.lib.path);
    debug!("Loading WASM component from: {}", wasm_path.display());
    let wasm_bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))?;
    debug!("Read {} bytes from WASM file", wasm_bytes.len());
    
    let component = Component::from_binary(&engine, &wasm_bytes)
        .with_context(|| format!("Failed to parse WASM component: {}", wasm_path.display()))?;
    debug!("WASM component parsed successfully");

    // Create instance using LifecycleExtensionInstance (lifecycle-extension world)
    debug!("Creating LifecycleExtensionInstance...");
    let instance = LifecycleExtensionInstance::new(&engine, &component)
        .with_context(|| "Failed to create lifecycle extension instance from WASM component")?;
    debug!("LifecycleExtensionInstance created successfully");

    Ok(instance)
}

/// Find and load the lifecycle extension
/// 
/// Search order (new extension system):
/// 1. Project-local extensions/coder-lifecycle/ (with extension.toml)
/// 2. Home directory ~/.{agent}/extensions/coder-lifecycle/ (with extension.toml)
///
/// Legacy paths (deprecated, will be removed in future):
/// 3. Project-local providers/lifecycle/lifecycle.wasm
/// 4. Home directory ~/.{agent}/providers/lifecycle/lifecycle.wasm
pub async fn find_lifecycle_plugin() -> Result<LifecyclePlugin> {
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
    
    // New extension system paths (preferred)
    let extension_paths = vec![
        // Project-local extension
        PathBuf::from("extensions/coder-lifecycle"),
        // Home directory extension
        PathBuf::from(format!("~/.{}/extensions/coder-lifecycle", agent_name)).expand_home()?,
    ];

    // Check new extension system first
    for path in &extension_paths {
        let manifest_path = path.join("extension.toml");
        if manifest_path.exists() {
            debug!("Found lifecycle extension at: {}", path.display());
            return LifecyclePlugin::new(path.clone()).await;
        }
    }

    // Legacy paths (deprecated)
    let legacy_paths = vec![
        // Legacy project-local path
        PathBuf::from("providers/lifecycle"),
        // Legacy home directory path  
        PathBuf::from(format!("~/.{}/providers/lifecycle", agent_name)).expand_home()?,
    ];

    for path in &legacy_paths {
        // Check if there's an extension.toml (legacy extension format)
        let manifest_path = path.join("extension.toml");
        if manifest_path.exists() {
            debug!("Found legacy lifecycle extension at: {}", path.display());
            eprintln!("[WARN] Using deprecated lifecycle plugin location. Please migrate to ~/.{}/extensions/coder-lifecycle/", agent_name);
            return LifecyclePlugin::new(path.clone()).await;
        }
        
        // Check for bare WASM file (very old format - not supported by new system)
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

    anyhow::bail!(
        "Lifecycle extension not found. Expected at one of:\n  \
        - extensions/coder-lifecycle/extension.toml\n  \
        - ~/.{}/extensions/coder-lifecycle/extension.toml\n\n\
        Install with: trustee extension install <path-to-coder-lifecycle-wasm>",
        agent_name
    )
}

/// Helper trait to expand ~ in paths
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
