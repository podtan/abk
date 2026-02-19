//! Provider factory for creating LLM providers based on configuration.
//!
//! This module provides a factory for dynamically creating LLM providers
//! based on configuration and environment settings.
//!
//! The factory uses the extension system: ~/.{agent}/extensions/{name}/

use crate::config::EnvironmentLoader;
use crate::provider::LlmProvider;
use crate::provider::extension::ExtensionProvider;
use anyhow::Result;
use std::path::PathBuf;

macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG] {}", format!($($arg)*));
        }
    };
}

/// Factory for creating LLM providers
pub struct ProviderFactory;

impl ProviderFactory {
    /// Create a provider based on environment configuration
    ///
    /// Uses extension system: ~/.{agent}/extensions/{name}/
    ///
    /// # Arguments
    /// * `env` - Environment loader with configuration
    ///
    /// # Returns
    /// A boxed LLM provider implementation
    pub async fn create(env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        let provider_name = env.llm_provider().unwrap_or_else(|| "openai-unofficial".to_string());
        Self::create_extension_provider(&provider_name, env).await
    }

    /// Create an extension-based provider
    async fn create_extension_provider(
        provider_name: &str,
        env: &EnvironmentLoader,
    ) -> Result<Box<dyn LlmProvider>> {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".to_string());
        debug!("Factory - Agent name: {}", agent_name);

        let extensions_dir = if let Ok(home_dir) = std::env::var("HOME") {
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
