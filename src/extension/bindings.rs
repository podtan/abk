//! Host-side WIT bindings for extensions
//!
//! This module generates and exports the host-side bindings for calling
//! extension WASM components using wasmtime's component model.

use super::error::{ExtensionError, ExtensionResult};
use std::sync::Arc;
use tracing::debug;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::WasiCtx;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::WasiView;

// Generate bindings for the full extension world (requires all interfaces)
wasmtime::component::bindgen!({
    path: "wit/extension",
    world: "extension",
    async: false,
});

// Generate bindings for lifecycle-only extensions (sync for lifecycle)
mod lifecycle_extension_bindings {
    wasmtime::component::bindgen!({
        path: "wit/extension",
        world: "lifecycle-extension",
        async: false,
    });
}

// Generate bindings for provider-only extensions (async for proper wasmtime support)
mod provider_extension_bindings {
    wasmtime::component::bindgen!({
        path: "wit/extension",
        world: "provider-extension",
        async: true,
    });
}

// Re-export generated types at module level

/// Core interface types
pub mod core {
    #![allow(missing_docs)]
    pub use super::exports::abk::extension::core::*;
}

/// Lifecycle interface types
pub mod lifecycle {
    #![allow(missing_docs)]
    pub use super::exports::abk::extension::lifecycle::*;
}

/// Provider interface types
pub mod provider {
    #![allow(missing_docs)]
    pub use super::exports::abk::extension::provider::*;
}

/// Provider-only extension types (from provider-extension world)
pub mod provider_only {
    #![allow(missing_docs)]
    pub use super::provider_extension_bindings::exports::abk::extension::core as core;
    pub use super::provider_extension_bindings::exports::abk::extension::provider as provider;
}

/// Lifecycle-only extension types (from lifecycle-extension world)
pub mod lifecycle_only {
    #![allow(missing_docs)]
    pub use super::lifecycle_extension_bindings::exports::abk::extension::core as core;
    pub use super::lifecycle_extension_bindings::exports::abk::extension::lifecycle as lifecycle;
}

/// WASI state for extension execution
pub struct ExtensionState {
    /// WASI context
    ctx: WasiCtx,
    /// Resource table for WASI
    table: ResourceTable,
}

impl ExtensionState {
    /// Create a new extension state with default WASI context
    pub fn new() -> Self {
        Self {
            ctx: WasiCtxBuilder::new()
                .inherit_stdout()
                .inherit_stderr()
                .build(),
            table: ResourceTable::new(),
        }
    }
}

impl Default for ExtensionState {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiView for ExtensionState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// An instantiated extension ready for calling
pub struct ExtensionInstance {
    /// Store with extension state
    store: Store<ExtensionState>,
    /// Bindings to the extension's exports
    bindings: Extension,
}

impl ExtensionInstance {
    /// Instantiate an extension from a loaded component
    ///
    /// Note: The engine must be configured WITHOUT async_support for this to work,
    /// since we use synchronous bindgen for the extension world.
    pub fn new(engine: &Arc<Engine>, component: &Component) -> ExtensionResult<Self> {
        debug!(target: "abk::extension", "Creating linker for extension");
        let mut linker = Linker::new(engine);

        // Add WASI to linker
        debug!(target: "abk::extension", "Adding WASI to linker");
        wasmtime_wasi::add_to_linker_sync(&mut linker).map_err(|e| {
            ExtensionError::WasmLoadError(format!("Failed to add WASI to linker: {}", e))
        })?;

        // Create store with state
        debug!(target: "abk::extension", "Creating store");
        let state = ExtensionState::new();
        let mut store = Store::new(engine, state);

        // Instantiate
        debug!(target: "abk::extension", "Instantiating extension component");
        let bindings = Extension::instantiate(&mut store, component, &linker).map_err(|e| {
            debug!(target: "abk::extension", "Instantiation failed: {}", e);
            ExtensionError::WasmLoadError(format!("Failed to instantiate extension: {}", e))
        })?;
        debug!(target: "abk::extension", "Extension instantiated successfully");

        Ok(Self { store, bindings })
    }

    /// Alias for `new` - instantiate with a provided engine
    ///
    /// The engine must NOT have async_support enabled (sync bindgen).
    pub fn new_with_engine(engine: &Arc<Engine>, component: &Component) -> ExtensionResult<Self> {
        Self::new(engine, component)
    }

    /// Get extension metadata via core interface
    pub fn get_metadata(&mut self) -> ExtensionResult<core::ExtensionMetadata> {
        self.bindings
            .abk_extension_core()
            .call_get_metadata(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("get_metadata failed: {}", e)))
    }

    /// List capabilities via core interface
    pub fn list_capabilities(&mut self) -> ExtensionResult<Vec<String>> {
        self.bindings
            .abk_extension_core()
            .call_list_capabilities(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("list_capabilities failed: {}", e)))
    }

    /// Initialize the extension via core interface
    pub fn init(&mut self) -> ExtensionResult<()> {
        self.bindings
            .abk_extension_core()
            .call_init(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("init call failed: {}", e)))?
            .map_err(|e| ExtensionError::InitError(e))
    }

    // ===== Lifecycle Interface Methods =====

    /// Load template (lifecycle capability)
    pub fn load_template(&mut self, template_name: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_load_template(&mut self.store, template_name)
            .map_err(|e| ExtensionError::CallError(format!("load_template failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })
    }

    /// Render template with variables (lifecycle capability)
    pub fn render_template(
        &mut self,
        content: &str,
        variables: &[(String, String)],
    ) -> ExtensionResult<String> {
        let vars: Vec<lifecycle::TemplateVariable> = variables
            .iter()
            .map(|(k, v)| lifecycle::TemplateVariable {
                key: k.clone(),
                value: v.clone(),
            })
            .collect();

        self.bindings
            .abk_extension_lifecycle()
            .call_render_template(&mut self.store, content, &vars)
            .map_err(|e| ExtensionError::CallError(format!("render_template failed: {}", e)))
    }

    /// Extract sections from markdown (lifecycle capability)
    pub fn extract_sections(&mut self, content: &str) -> ExtensionResult<Vec<(String, String)>> {
        let sections = self
            .bindings
            .abk_extension_lifecycle()
            .call_extract_sections(&mut self.store, content)
            .map_err(|e| ExtensionError::CallError(format!("extract_sections failed: {}", e)))?;

        Ok(sections.into_iter().map(|s| (s.name, s.content)).collect())
    }

    /// Validate template variables (lifecycle capability)
    pub fn validate_template_variables(&mut self, content: &str) -> ExtensionResult<Vec<String>> {
        self.bindings
            .abk_extension_lifecycle()
            .call_validate_template_variables(&mut self.store, content)
            .map_err(|e| {
                ExtensionError::CallError(format!("validate_template_variables failed: {}", e))
            })
    }

    /// Get task template name (lifecycle capability)
    pub fn get_task_template_name(&mut self, task_type: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_get_task_template_name(&mut self.store, task_type)
            .map_err(|e| ExtensionError::CallError(format!("get_task_template_name failed: {}", e)))
    }

    /// Classify task (lifecycle capability)
    pub fn classify_task(
        &mut self,
        task_description: &str,
    ) -> ExtensionResult<(String, f32)> {
        let result = self
            .bindings
            .abk_extension_lifecycle()
            .call_classify_task(&mut self.store, task_description)
            .map_err(|e| ExtensionError::CallError(format!("classify_task failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })?;

        Ok((result.task_type, result.confidence))
    }

    /// Load useful commands (lifecycle capability)
    pub fn load_useful_commands(&mut self) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_load_useful_commands(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("load_useful_commands failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })
    }

    /// Get system info variables (lifecycle capability)
    pub fn get_system_info_variables(&mut self) -> ExtensionResult<Vec<(String, String)>> {
        let vars = self
            .bindings
            .abk_extension_lifecycle()
            .call_get_system_info_variables(&mut self.store)
            .map_err(|e| {
                ExtensionError::CallError(format!("get_system_info_variables failed: {}", e))
            })?;

        Ok(vars.into_iter().map(|v| (v.key, v.value)).collect())
    }

    // ===== Provider Interface Methods =====

    /// Get provider metadata (provider capability)
    pub fn get_provider_metadata(&mut self) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_get_provider_metadata(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("get_provider_metadata failed: {}", e)))
    }

    /// Format request for LLM API (provider capability)
    pub fn format_request(
        &mut self,
        messages: &[(String, String)],
        config: &provider::Config,
        tools: Option<&[provider::Tool]>,
    ) -> ExtensionResult<String> {
        let msgs: Vec<provider::Message> = messages
            .iter()
            .map(|(role, content)| provider::Message {
                role: role.clone(),
                content: content.clone(),
            })
            .collect();

        self.bindings
            .abk_extension_provider()
            .call_format_request(&mut self.store, &msgs, config, tools)
            .map_err(|e| ExtensionError::CallError(format!("format_request failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }

    /// Parse response from provider API (provider capability)
    pub fn parse_response(
        &mut self,
        body: &str,
        model: &str,
    ) -> ExtensionResult<provider::AssistantMessage> {
        self.bindings
            .abk_extension_provider()
            .call_parse_response(&mut self.store, body, model)
            .map_err(|e| ExtensionError::CallError(format!("parse_response failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }

    /// Handle streaming chunk (provider capability)
    pub fn handle_stream_chunk(
        &mut self,
        chunk: &str,
    ) -> ExtensionResult<Option<provider::ContentDelta>> {
        self.bindings
            .abk_extension_provider()
            .call_handle_stream_chunk(&mut self.store, chunk)
            .map_err(|e| ExtensionError::CallError(format!("handle_stream_chunk failed: {}", e)))
    }

    /// Get API URL for a model (provider capability)
    pub fn get_api_url(&mut self, base_url: &str, model: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_get_api_url(&mut self.store, base_url, model)
            .map_err(|e| ExtensionError::CallError(format!("get_api_url failed: {}", e)))
    }

    /// Check if streaming is supported for a model (provider capability)
    pub fn supports_streaming(&mut self, model: &str) -> ExtensionResult<bool> {
        self.bindings
            .abk_extension_provider()
            .call_supports_streaming(&mut self.store, model)
            .map_err(|e| ExtensionError::CallError(format!("supports_streaming failed: {}", e)))
    }

    /// Format request from JSON (provider capability)
    /// Used for complex messages with tool_call_id, tool_calls arrays, etc.
    pub fn format_request_from_json(
        &mut self,
        messages_json: &str,
        model: &str,
        tools_json: Option<&str>,
        tool_choice_json: Option<&str>,
        max_tokens: Option<u32>,
        temperature: f32,
        enable_streaming: bool,
    ) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_format_request_from_json(
                &mut self.store,
                messages_json,
                model,
                tools_json,
                tool_choice_json,
                max_tokens,
                temperature,
                enable_streaming,
            )
            .map_err(|e| ExtensionError::CallError(format!("format_request_from_json failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }
}

/// An instantiated provider-only extension (no lifecycle interface required)
pub struct ProviderExtensionInstance {
    /// Store with extension state
    store: Store<ExtensionState>,
    /// Bindings to the extension's exports (provider-extension world)
    bindings: provider_extension_bindings::ProviderExtension,
}

impl ProviderExtensionInstance {
    /// Instantiate a provider-only extension from a loaded component
    pub async fn new(engine: &Arc<Engine>, component: &Component) -> ExtensionResult<Self> {
        let mut linker = Linker::new(engine);

        // Add WASI to linker (async version for async bindings)
        wasmtime_wasi::add_to_linker_async(&mut linker).map_err(|e| {
            ExtensionError::WasmLoadError(format!("Failed to add WASI to linker: {}", e))
        })?;

        // Create store with state
        let state = ExtensionState::new();
        let mut store = Store::new(engine, state);

        // Instantiate using provider-extension world (async)
        let bindings = provider_extension_bindings::ProviderExtension::instantiate_async(&mut store, component, &linker)
            .await
            .map_err(|e| {
                ExtensionError::WasmLoadError(format!("Failed to instantiate provider extension: {}", e))
            })?;

        Ok(Self { store, bindings })
    }

    /// Get extension metadata via core interface
    pub async fn get_metadata(&mut self) -> ExtensionResult<provider_only::core::ExtensionMetadata> {
        self.bindings
            .abk_extension_core()
            .call_get_metadata(&mut self.store)
            .await
            .map_err(|e| ExtensionError::CallError(format!("get_metadata failed: {}", e)))
    }

    /// List capabilities via core interface
    pub async fn list_capabilities(&mut self) -> ExtensionResult<Vec<String>> {
        self.bindings
            .abk_extension_core()
            .call_list_capabilities(&mut self.store)
            .await
            .map_err(|e| ExtensionError::CallError(format!("list_capabilities failed: {}", e)))
    }

    /// Initialize the extension via core interface
    pub async fn init(&mut self) -> ExtensionResult<()> {
        self.bindings
            .abk_extension_core()
            .call_init(&mut self.store)
            .await
            .map_err(|e| ExtensionError::CallError(format!("init call failed: {}", e)))?
            .map_err(|e| ExtensionError::InitError(e))
    }

    // ===== Provider Interface Methods =====

    /// Get provider metadata (provider capability)
    pub async fn get_provider_metadata(&mut self) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_get_provider_metadata(&mut self.store)
            .await
            .map_err(|e| ExtensionError::CallError(format!("get_provider_metadata failed: {}", e)))
    }

    /// Format request for LLM API (provider capability)
    pub async fn format_request(
        &mut self,
        messages: &[(String, String)],
        config: &provider_only::provider::Config,
        tools: Option<&[provider_only::provider::Tool]>,
    ) -> ExtensionResult<String> {
        let msgs: Vec<provider_only::provider::Message> = messages
            .iter()
            .map(|(role, content)| provider_only::provider::Message {
                role: role.clone(),
                content: content.clone(),
            })
            .collect();

        self.bindings
            .abk_extension_provider()
            .call_format_request(&mut self.store, &msgs, config, tools)
            .await
            .map_err(|e| ExtensionError::CallError(format!("format_request failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }

    /// Parse response from provider API (provider capability)
    pub async fn parse_response(
        &mut self,
        body: &str,
        model: &str,
    ) -> ExtensionResult<provider_only::provider::AssistantMessage> {
        self.bindings
            .abk_extension_provider()
            .call_parse_response(&mut self.store, body, model)
            .await
            .map_err(|e| ExtensionError::CallError(format!("parse_response failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }

    /// Handle streaming chunk (provider capability)
    pub async fn handle_stream_chunk(
        &mut self,
        chunk: &str,
    ) -> ExtensionResult<Option<provider_only::provider::ContentDelta>> {
        self.bindings
            .abk_extension_provider()
            .call_handle_stream_chunk(&mut self.store, chunk)
            .await
            .map_err(|e| ExtensionError::CallError(format!("handle_stream_chunk failed: {}", e)))
    }

    /// Get API URL for a model (provider capability)
    pub async fn get_api_url(&mut self, base_url: &str, model: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_get_api_url(&mut self.store, base_url, model)
            .await
            .map_err(|e| ExtensionError::CallError(format!("get_api_url failed: {}", e)))
    }

    /// Check if streaming is supported for a model (provider capability)
    pub async fn supports_streaming(&mut self, model: &str) -> ExtensionResult<bool> {
        self.bindings
            .abk_extension_provider()
            .call_supports_streaming(&mut self.store, model)
            .await
            .map_err(|e| ExtensionError::CallError(format!("supports_streaming failed: {}", e)))
    }

    /// Format request from JSON (provider capability)
    /// Used for complex messages with tool_call_id, tool_calls arrays, etc.
    pub async fn format_request_from_json(
        &mut self,
        messages_json: &str,
        model: &str,
        tools_json: Option<&str>,
        tool_choice_json: Option<&str>,
        max_tokens: Option<u32>,
        temperature: f32,
        enable_streaming: bool,
    ) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_provider()
            .call_format_request_from_json(
                &mut self.store,
                messages_json,
                model,
                tools_json,
                tool_choice_json,
                max_tokens,
                temperature,
                enable_streaming,
            )
            .await
            .map_err(|e| ExtensionError::CallError(format!("format_request_from_json failed: {}", e)))?
            .map_err(|e| ExtensionError::ProviderError(format!("{}: {:?}", e.message, e.code)))
    }
}

/// An instantiated lifecycle-only extension (no provider interface required)
pub struct LifecycleExtensionInstance {
    /// Store with extension state
    store: Store<ExtensionState>,
    /// Bindings to the extension's exports (lifecycle-extension world)
    bindings: lifecycle_extension_bindings::LifecycleExtension,
}

impl LifecycleExtensionInstance {
    /// Instantiate a lifecycle-only extension from a loaded component
    ///
    /// Note: The engine must be configured WITHOUT async_support for this to work,
    /// since we use synchronous bindgen for lifecycle extensions.
    pub fn new(engine: &Arc<Engine>, component: &Component) -> ExtensionResult<Self> {
        debug!(target: "abk::extension", "LifecycleExtensionInstance: Creating linker");
        let mut linker = Linker::new(engine);

        // Add WASI to linker (sync version for sync bindings)
        debug!(target: "abk::extension", "LifecycleExtensionInstance: Adding WASI to linker");
        wasmtime_wasi::add_to_linker_sync(&mut linker).map_err(|e| {
            ExtensionError::WasmLoadError(format!("Failed to add WASI to linker: {}", e))
        })?;

        // Create store with state
        debug!(target: "abk::extension", "LifecycleExtensionInstance: Creating store");
        let state = ExtensionState::new();
        let mut store = Store::new(engine, state);

        // Instantiate using lifecycle-extension world (sync)
        debug!(target: "abk::extension", "LifecycleExtensionInstance: Instantiating component");
        let bindings = lifecycle_extension_bindings::LifecycleExtension::instantiate(&mut store, component, &linker)
            .map_err(|e| {
                debug!(target: "abk::extension", "LifecycleExtensionInstance: Instantiation failed: {}", e);
                ExtensionError::WasmLoadError(format!("Failed to instantiate lifecycle extension: {}", e))
            })?;
        debug!(target: "abk::extension", "LifecycleExtensionInstance: Created successfully");

        Ok(Self { store, bindings })
    }

    /// Get extension metadata via core interface
    pub fn get_metadata(&mut self) -> ExtensionResult<lifecycle_only::core::ExtensionMetadata> {
        self.bindings
            .abk_extension_core()
            .call_get_metadata(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("get_metadata failed: {}", e)))
    }

    /// List capabilities via core interface
    pub fn list_capabilities(&mut self) -> ExtensionResult<Vec<String>> {
        self.bindings
            .abk_extension_core()
            .call_list_capabilities(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("list_capabilities failed: {}", e)))
    }

    /// Initialize the extension via core interface
    pub fn init(&mut self) -> ExtensionResult<()> {
        self.bindings
            .abk_extension_core()
            .call_init(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("init call failed: {}", e)))?
            .map_err(|e| ExtensionError::InitError(e))
    }

    // ===== Lifecycle Interface Methods =====

    /// Load template (lifecycle capability)
    pub fn load_template(&mut self, template_name: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_load_template(&mut self.store, template_name)
            .map_err(|e| ExtensionError::CallError(format!("load_template failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })
    }

    /// Render template with variables (lifecycle capability)
    pub fn render_template(
        &mut self,
        content: &str,
        variables: &[(String, String)],
    ) -> ExtensionResult<String> {
        let vars: Vec<lifecycle_only::lifecycle::TemplateVariable> = variables
            .iter()
            .map(|(k, v)| lifecycle_only::lifecycle::TemplateVariable {
                key: k.clone(),
                value: v.clone(),
            })
            .collect();

        self.bindings
            .abk_extension_lifecycle()
            .call_render_template(&mut self.store, content, &vars)
            .map_err(|e| ExtensionError::CallError(format!("render_template failed: {}", e)))
    }

    /// Extract sections from markdown (lifecycle capability)
    pub fn extract_sections(&mut self, content: &str) -> ExtensionResult<Vec<(String, String)>> {
        let sections = self
            .bindings
            .abk_extension_lifecycle()
            .call_extract_sections(&mut self.store, content)
            .map_err(|e| ExtensionError::CallError(format!("extract_sections failed: {}", e)))?;

        Ok(sections.into_iter().map(|s| (s.name, s.content)).collect())
    }

    /// Validate template variables (lifecycle capability)
    pub fn validate_template_variables(&mut self, content: &str) -> ExtensionResult<Vec<String>> {
        self.bindings
            .abk_extension_lifecycle()
            .call_validate_template_variables(&mut self.store, content)
            .map_err(|e| {
                ExtensionError::CallError(format!("validate_template_variables failed: {}", e))
            })
    }

    /// Get task template name (lifecycle capability)
    pub fn get_task_template_name(&mut self, task_type: &str) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_get_task_template_name(&mut self.store, task_type)
            .map_err(|e| ExtensionError::CallError(format!("get_task_template_name failed: {}", e)))
    }

    /// Classify task (lifecycle capability)
    pub fn classify_task(
        &mut self,
        task_description: &str,
    ) -> ExtensionResult<(String, f32)> {
        let result = self
            .bindings
            .abk_extension_lifecycle()
            .call_classify_task(&mut self.store, task_description)
            .map_err(|e| ExtensionError::CallError(format!("classify_task failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })?;

        Ok((result.task_type, result.confidence))
    }

    /// Load useful commands (lifecycle capability)
    pub fn load_useful_commands(&mut self) -> ExtensionResult<String> {
        self.bindings
            .abk_extension_lifecycle()
            .call_load_useful_commands(&mut self.store)
            .map_err(|e| ExtensionError::CallError(format!("load_useful_commands failed: {}", e)))?
            .map_err(|e| {
                ExtensionError::LifecycleError(format!("{}: {:?}", e.message, e.code))
            })
    }

    /// Get system info variables (lifecycle capability)
    pub fn get_system_info_variables(&mut self) -> ExtensionResult<Vec<(String, String)>> {
        let vars = self
            .bindings
            .abk_extension_lifecycle()
            .call_get_system_info_variables(&mut self.store)
            .map_err(|e| {
                ExtensionError::CallError(format!("get_system_info_variables failed: {}", e))
            })?;

        Ok(vars.into_iter().map(|v| (v.key, v.value)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_state_creation() {
        let state = ExtensionState::new();
        // Just verify it creates without panic
        drop(state);
    }

    #[test]
    fn test_extension_state_default() {
        let state = ExtensionState::default();
        drop(state);
    }
}
