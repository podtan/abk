//! Provider factory for creating LLM providers based on configuration.
//!
//! This module provides a factory for dynamically creating LLM providers
//! based on configuration and environment settings.
//!
//! Dispatch logic:
//! - `openai-unofficial` (or unset) → native Rust `OpenAIProvider` (no wasmtime)
//! - `openai-unofficial-wasm` → WASM `ExtensionProvider`
//! - Any other name → `ExtensionProvider` (WASM extension system)

use crate::config::EnvironmentLoader;
use crate::provider::LlmProvider;
use crate::provider::openai::OpenAIProvider;
#[cfg(feature = "extension")]
use crate::provider::extension::ExtensionProvider;
use anyhow::Result;
use std::path::PathBuf;

macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            crate::observability::tee_eprintln(&format!("[DEBUG] {}", format!($($arg)*)));
        }
    };
}

/// Factory for creating LLM providers
pub struct ProviderFactory;

impl ProviderFactory {
    /// Create a provider based on environment configuration
    ///
    /// Dispatch:
    /// - `openai-unofficial` or unset → native Rust `OpenAIProvider`
    /// - `openai-unofficial-wasm` or any other → `ExtensionProvider` (WASM)
    pub async fn create(env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        let provider_name = env.llm_provider().unwrap_or_else(|| "openai-unofficial".to_string());

        debug!("Factory - provider_name: {}", provider_name);

        // Route to native Rust provider for the default / native case
        if provider_name == "openai-unofficial" {
            debug!("Factory - using native Rust OpenAIProvider");
            let provider = OpenAIProvider::new()?;
            return Ok(Box::new(provider));
        }

        // Everything else goes through the WASM extension system
        #[cfg(feature = "extension")]
        {
            debug!("Factory - using WASM ExtensionProvider for: {}", provider_name);
            return Self::create_extension_provider(&provider_name, env).await;
        }

        #[cfg(not(feature = "extension"))]
        {
            anyhow::bail!(
                "Provider '{}' requires the 'extension' feature (WASM), which is not enabled. \
                 Set LLM_PROVIDER=openai-unofficial for the native Rust provider.",
                provider_name
            );
        }
    }

    /// Create an extension-based (WASM) provider
    #[cfg(feature = "extension")]
    async fn create_extension_provider(
        provider_name: &str,
        env: &EnvironmentLoader,
    ) -> Result<Box<dyn LlmProvider>> {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".to_string());
        debug!("Factory - Agent name: {}", agent_name);

        let extensions_dir = if let Ok(home_dir) = crate::get_home_dir() {
            PathBuf::from(home_dir).join(format!(".{}/extensions", agent_name))
        } else {
            PathBuf::from("extensions")
        };
        debug!("Factory - Looking for extensions in: {}", extensions_dir.display());

        let extension_path = extensions_dir.join(provider_name);
        let manifest_path = extension_path.join("extension.toml");
        debug!("Factory - Checking manifest at: {}", manifest_path.display());

        if !manifest_path.exists() {
            debug!("Factory - Manifest not found at installed location");
            let dev_extensions = PathBuf::from("extensions");
            let dev_manifest = dev_extensions.join(provider_name).join("extension.toml");
            debug!("Factory - Trying dev location: {}", dev_manifest.display());
            
            if !dev_manifest.exists() {
                anyhow::bail!(
                    "Provider extension '{}' not found.\n\
                    Tried:\n\
                    - {}/extension.toml (installed)\n\
                    - extensions/{}/extension.toml (development)\n\
                    \n\
                    Install a provider extension or create one.",
                    provider_name, extension_path.display(), provider_name
                );
            }
            
            debug!("Factory - Creating extension provider from dev location");
            let provider = ExtensionProvider::new(provider_name.to_string(), dev_extensions, env.clone()).await?;
            return Ok(Box::new(provider));
        }

        debug!("Factory - Creating extension provider from installed location");
        debug!("Factory - Using extension provider: {}", provider_name);
        let provider = ExtensionProvider::new(provider_name.to_string(), extensions_dir, env.clone()).await?;
        Ok(Box::new(provider))
    }
}
