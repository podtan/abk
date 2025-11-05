//! Lifecycle WASM Plugin Loader
//!
//! This module loads and interfaces with the lifecycle.wasm plugin which handles
//! template management, task classification, and system information gathering.
//!
//! All templates are embedded in the WASM plugin, eliminating filesystem dependencies.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use wasmtime::component::*;
use wasmtime::Engine;
use wasmtime_wasi::{WasiCtx, WasiView};

/// Conditional debug macro
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG LIFECYCLE] {}", format!($($arg)*));
        }
    };
}

// WASI host state for component instantiation
struct ComponentState {
    ctx: WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl WasiView for ComponentState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }

    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}

// Generate bindings from WIT interface for lifecycle world
bindgen!({
    path: "wit/lifecycle",
    world: "lifecycle",
    async: true,
});

/// Lifecycle WASM plugin wrapper
pub struct LifecyclePlugin {
    /// Plugin name (always "lifecycle")
    #[allow(dead_code)]
    name: String,

    /// WASM module path
    #[allow(dead_code)]
    wasm_path: PathBuf,

    /// Shared WASM engine
    engine: Arc<Engine>,

    /// Pre-compiled WASM component (cached)
    component: Arc<Component>,

    /// Linker for WASI (cached)
    linker: Arc<OnceCell<Arc<Linker<ComponentState>>>>,
}

impl LifecyclePlugin {
    /// Create a new lifecycle plugin loader
    ///
    /// # Arguments
    /// * `wasm_path` - Path to lifecycle.wasm file
    pub fn new(wasm_path: PathBuf) -> Result<Self> {
        debug!("Creating lifecycle plugin from: {}", wasm_path.display());

        // Create shared WASM engine
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(Engine::new(&config)?);

        // Pre-compile component
        let component_bytes = std::fs::read(&wasm_path)
            .with_context(|| format!("Failed to read lifecycle WASM: {}", wasm_path.display()))?;
        let component = Arc::new(Component::new(&engine, &component_bytes)?);

        debug!("Lifecycle plugin compiled successfully");

        Ok(Self {
            name: "lifecycle".to_string(),
            wasm_path,
            engine,
            component,
            linker: Arc::new(OnceCell::new()),
        })
    }

    /// Get or create the linker (lazy initialization)
    async fn get_linker(&self) -> Result<Arc<Linker<ComponentState>>> {
        if let Some(linker) = self.linker.get() {
            return Ok(linker.clone());
        }

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        let linker_arc = Arc::new(linker);
        let _ = self.linker.set(linker_arc.clone());
        Ok(linker_arc)
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

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        let result = instance
            .simpaticoder_lifecycle_adapter()
            .call_load_template(&mut store, template_name)
            .await
            .with_context(|| format!("Failed to call load_template for: {}", template_name))?;

        match result {
            Ok(content) => {
                debug!("Template loaded successfully: {} ({} bytes)", template_name, content.len());
                Ok(content)
            }
            Err(err) => {
                anyhow::bail!("Lifecycle plugin error: {}", err.message);
            }
        }
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

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        // Convert to WASM types
        let wasm_vars: Vec<_> = variables
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    exports::simpaticoder::lifecycle::adapter::TemplateVariable {
                        key: k.clone(),
                        value: v.clone(),
                    },
                )
            })
            .collect();

        let wasm_vars_slice: Vec<_> = wasm_vars
            .iter()
            .map(|(_, v)| v.clone())
            .collect();

        let result = instance
            .simpaticoder_lifecycle_adapter()
            .call_render_template(&mut store, template_content, &wasm_vars_slice)
            .await
            .with_context(|| "Failed to call render_template")?;

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

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        let result = instance
            .simpaticoder_lifecycle_adapter()
            .call_classify_task(&mut store, task_description)
            .await
            .with_context(|| "Failed to call classify_task")?;

        match result {
            Ok(classification) => {
                debug!(
                    "Task classified as: {} (confidence: {})",
                    classification.task_type, classification.confidence
                );
                Ok((classification.task_type, classification.confidence))
            }
            Err(err) => {
                anyhow::bail!("Lifecycle plugin error: {}", err.message);
            }
        }
    }

    /// Get system information variables
    ///
    /// # Returns
    /// Vector of (key, value) tuples for system info
    pub async fn get_system_info_variables(&self) -> Result<Vec<(String, String)>> {
        debug!("Getting system info variables");

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        let result = instance
            .simpaticoder_lifecycle_adapter()
            .call_get_system_info_variables(&mut store)
            .await
            .with_context(|| "Failed to call get_system_info_variables")?;

        let vars: Vec<_> = result
            .iter()
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();

        debug!("Retrieved {} system info variables", vars.len());
        Ok(vars)
    }

    /// Load useful commands content
    ///
    /// # Returns
    /// Markdown content with available tools and commands
    pub async fn load_useful_commands(&self) -> Result<String> {
        debug!("Loading useful commands");

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        let result = instance
            .simpaticoder_lifecycle_adapter()
            .call_load_useful_commands(&mut store)
            .await
            .with_context(|| "Failed to call load_useful_commands")?;

        match result {
            Ok(content) => {
                debug!("Useful commands loaded successfully ({} bytes)", content.len());
                Ok(content)
            }
            Err(err) => {
                anyhow::bail!("Lifecycle plugin error: {}", err.message);
            }
        }
    }

    /// Get plugin metadata
    pub async fn get_metadata(&self) -> Result<String> {
        debug!("Getting lifecycle plugin metadata");

        let linker = self.get_linker().await?;
        let wasi_ctx = WasiCtx::builder().inherit_stdio().build();
        let mut store = wasmtime::Store::new(
            &self.engine,
            ComponentState {
                ctx: wasi_ctx,
                table: wasmtime::component::ResourceTable::new(),
            },
        );

        let instance = Lifecycle::instantiate_async(&mut store, &self.component, &linker)
            .await
            .with_context(|| "Failed to instantiate lifecycle plugin")?;

        let metadata = instance
            .simpaticoder_lifecycle_adapter()
            .call_get_lifecycle_metadata(&mut store)
            .await
            .with_context(|| "Failed to call get_lifecycle_metadata")?;

        debug!("Metadata retrieved");
        Ok(metadata)
    }
}

/// Find and load the lifecycle plugin
pub fn find_lifecycle_plugin() -> Result<LifecyclePlugin> {
    // Try multiple locations
    let possible_paths = vec![
        PathBuf::from("providers/lifecycle/lifecycle.wasm"),
        PathBuf::from("~/.simpaticoder/providers/lifecycle/lifecycle.wasm").expand_home()?,
    ];

    for path in possible_paths {
        if path.exists() {
            debug!("Found lifecycle plugin at: {}", path.display());
            return LifecyclePlugin::new(path);
        }
    }

    anyhow::bail!(
        "Lifecycle plugin not found. Expected at: providers/lifecycle/lifecycle.wasm"
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
