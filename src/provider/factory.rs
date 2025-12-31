//! Provider factory for creating LLM providers based on configuration.
//!
//! This module provides a factory for dynamically creating LLM providers
//! based on configuration and environment settings.
//!
//! The factory tries to find providers in this order:
//! 1. Extension system: ~/.{agent}/extensions/{name}/ (new)
//! 2. Old plugin system: ~/.{agent}/providers/{name}/provider.wasm (legacy)
//! 3. Development: ./providers/{name}/provider.wasm

use crate::config::EnvironmentLoader;
use crate::provider::LlmProvider;
use crate::provider::wasm::WasmProvider;
use crate::provider::extension::ExtensionProvider;
use anyhow::Result;
use std::path::PathBuf;

/// Conditional debug macro - only prints if RUST_LOG is set to debug
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
    /// Tries to find provider in this order:
    /// 1. Extension system (new) - looks in ~/.{agent}/extensions/{name}/
    /// 2. Old plugin system (legacy) - looks in ~/.{agent}/providers/{name}/provider.wasm
    ///
    /// # Arguments
    /// * `env` - Environment loader with configuration
    ///
    /// # Returns
    /// A boxed LLM provider implementation
    pub async fn create(env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        // Get provider name from environment (default to "openai-unofficial")
        let provider_name = env.llm_provider().unwrap_or_else(|| "openai-unofficial".to_string());

        // Try extension system first (new system)
        if let Ok(provider) = Self::try_create_extension_provider(&provider_name, env).await {
            debug!("Factory - Using extension provider: {}", provider_name);
            return Ok(provider);
        }

        // Fall back to old WASM plugin system
        debug!("Factory - Extension '{}' not found, trying legacy plugin system", provider_name);
        Self::create_wasm_provider(&provider_name, env)
    }

    /// Try to create an extension-based provider
    async fn try_create_extension_provider(
        provider_name: &str,
        env: &EnvironmentLoader,
    ) -> Result<Box<dyn LlmProvider>> {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".to_string());
        debug!("Factory - Agent name: {}", agent_name);

        // Look for extension in installed location
        let extensions_dir = if let Ok(home_dir) = std::env::var("HOME") {
            PathBuf::from(home_dir).join(format!(".{}/extensions", agent_name))
        } else {
            PathBuf::from("extensions")
        };
        debug!("Factory - Looking for extensions in: {}", extensions_dir.display());

        // Check if extension exists
        let extension_path = extensions_dir.join(provider_name);
        let manifest_path = extension_path.join("extension.toml");
        debug!("Factory - Checking manifest at: {}", manifest_path.display());

        if !manifest_path.exists() {
            debug!("Factory - Manifest not found at installed location");
            // Also try development location
            let dev_extensions = PathBuf::from("extensions");
            let dev_manifest = dev_extensions.join(provider_name).join("extension.toml");
            debug!("Factory - Trying dev location: {}", dev_manifest.display());
            
            if !dev_manifest.exists() {
                debug!("Factory - Extension not found anywhere");
                anyhow::bail!("Extension '{}' not found", provider_name);
            }
            
            // Use development extensions directory
            debug!("Factory - Creating extension provider from dev location");
            let provider = ExtensionProvider::new(provider_name.to_string(), dev_extensions, env.clone()).await?;
            return Ok(Box::new(provider));
        }

        // Use installed extensions directory
        debug!("Factory - Creating extension provider from installed location");
        let provider = ExtensionProvider::new(provider_name.to_string(), extensions_dir, env.clone()).await?;
        Ok(Box::new(provider))
    }

    /// Create a WASM-based provider (legacy plugin system)
    fn create_wasm_provider(provider_name: &str, env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        // Try to find WASM file in multiple locations (installed vs development)
        let wasm_path = Self::find_wasm_provider(provider_name)?;

        debug!("Factory - Loading legacy WASM provider: {}", provider_name);
        debug!("  WASM path: {}", wasm_path.display());

        Ok(Box::new(WasmProvider::new(
            provider_name.to_string(),
            wasm_path,
            env.clone(),
        )?))
    }

    /// Find WASM provider in installed or development location
    fn find_wasm_provider(provider_name: &str) -> Result<PathBuf> {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
        
        // 1. Try installed location: ~/.{agent_name}/providers/{name}/provider.wasm
        if let Ok(home_dir) = std::env::var("HOME") {
            let installed_path = PathBuf::from(home_dir)
                .join(format!(".{}", agent_name))
                .join("providers")
                .join(provider_name)
                .join("provider.wasm");
            
            if installed_path.exists() {
                return Ok(installed_path);
            }
        }

        // 2. Try development location: ./providers/{name}/provider.wasm
        let dev_path = PathBuf::from("providers")
            .join(provider_name)
            .join("provider.wasm");
        
        if dev_path.exists() {
            return Ok(dev_path);
        }

        // 3. Not found anywhere
        anyhow::bail!(
            "Unknown provider '{}'. WASM provider not found.\n\
            Tried:\n\
            - ~/.{}/providers/{}/provider.wasm (installed)\n\
            - ./providers/{}/provider.wasm (development)\n\
            \n\
            Supported built-in providers: openai, anthropic, github\n\
            For WASM providers, ensure provider.wasm is in the correct location.",
            provider_name, agent_name, provider_name, provider_name
        )
    }
}

