//! Environment variable loading and management.
//!
//! This module handles ONLY host-level configuration.
//! Provider-specific configuration (API keys, base URLs, models) is handled
//! by WASM components reading their own environment variables via WASI.

use std::env;
use std::path::Path;

/// Loads environment variables from .env file and system environment.
#[derive(Debug, Clone)]
pub struct EnvironmentLoader {
    #[allow(dead_code)]
    env_file: Option<String>,
}

impl EnvironmentLoader {
    /// Initialize the environment loader.
    ///
    /// # Arguments
    /// * `env_file` - Path to .env file. If None, looks for .env in current directory.
    pub fn new(env_file: Option<&Path>) -> Self {
        let env_path = env_file.unwrap_or(Path::new(".env"));

        // Only load a .env file if an explicit path was provided. This avoids
        // picking up repository or system .env files during unit tests which
        // expect default values.
        if env_file.is_some() && env_path.exists() {
            if let Err(e) = dotenv::from_path(env_path) {
                eprintln!("Warning: Failed to load .env file: {}", e);
            }
        }

        Self {
            env_file: env_file.map(|p| p.to_string_lossy().to_string()),
        }
    }

    /// Get LLM provider selection from environment.
    ///
    /// This is the ONLY provider-related env var the host cares about.
    /// Returns the provider name (e.g., "openai", "anthropic", "tanbal") or None to use default.
    pub fn llm_provider(&self) -> Option<String> {
        env::var("LLM_PROVIDER").ok()
    }
}

impl Default for EnvironmentLoader {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_provider_selection() {
        env::remove_var("LLM_PROVIDER");
        let env_loader = EnvironmentLoader::default();
        assert_eq!(env_loader.llm_provider(), None);

        env::set_var("LLM_PROVIDER", "tanbal");
        let env_loader = EnvironmentLoader::default();
        assert_eq!(env_loader.llm_provider(), Some("tanbal".to_string()));

        env::remove_var("LLM_PROVIDER");
    }

    #[test]
    fn test_env_file_loading() {
        // Test that EnvironmentLoader can be created
        let env_loader = EnvironmentLoader::new(None);
        assert!(env_loader.env_file.is_none());
    }
}
