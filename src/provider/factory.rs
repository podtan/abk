//! Provider factory for creating LLM providers based on configuration.
//!
//! This module provides a factory for dynamically creating LLM providers
//! based on configuration and environment settings.

use crate::config::EnvironmentLoader;
use crate::provider::LlmProvider;
use crate::provider::wasm::WasmProvider;
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
    /// This will read the LLM_PROVIDER environment variable (defaulting to "tanbal")
    /// and create the appropriate WASM provider instance.
    ///
    /// # Arguments
    /// * `env` - Environment loader with configuration
    ///
    /// # Returns
    /// A boxed LLM provider implementation
    ///
    /// # Example
    /// ```ignore
    /// use abk::provider::ProviderFactory;
    /// use abk::config::EnvironmentLoader;
    ///
    /// let env = EnvironmentLoader::load()?;
    /// let provider = ProviderFactory::create(&env)?;
    /// ```
    pub fn create(env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        // Get provider name from environment (default to "tanbal")
        let provider_name = env.llm_provider().unwrap_or_else(|| "tanbal".to_string());

        // All providers are now WASM-based
        Self::create_wasm_provider(&provider_name, env)
    }

    /// Create a WASM-based provider (generic loader for ANY .wasm file)
    fn create_wasm_provider(provider_name: &str, env: &EnvironmentLoader) -> Result<Box<dyn LlmProvider>> {
        // Try to find WASM file in multiple locations (installed vs development)
        let wasm_path = Self::find_wasm_provider(provider_name)?;

        debug!("Factory - Loading WASM provider: {}", provider_name);
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

