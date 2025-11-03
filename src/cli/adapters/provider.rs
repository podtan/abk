//! ProviderFactory adapter trait
//!
//! Provides access to LLM provider management operations.

use crate::cli::error::CliResult;
use std::collections::HashMap;

/// Information about an available provider
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider identifier (e.g., "tanbal", "openai")
    pub id: String,
    /// Display name
    pub name: String,
    /// Provider version
    pub version: String,
    /// Provider type (wasm, native)
    pub provider_type: String,
    /// Whether provider is available
    pub available: bool,
    /// Path to provider binary (for WASM providers)
    pub path: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Configuration for provider creation
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider ID to create
    pub provider_id: String,
    /// Additional configuration parameters
    pub parameters: HashMap<String, String>,
}

/// Factory for creating and managing LLM providers
///
/// This trait abstracts provider management without depending on
/// specific provider implementations.
///
/// # Example
///
/// ```rust,ignore
/// use abk::cli::ProviderFactory;
///
/// struct MyProviderFactory {
///     // ... fields
/// }
///
/// impl ProviderFactory for MyProviderFactory {
///     fn list_providers(&self) -> CliResult<Vec<ProviderInfo>> {
///         // Return available providers
///         Ok(vec![])
///     }
///
///     // ... implement remaining methods
/// }
/// ```
pub trait ProviderFactory {
    /// List all available providers
    fn list_providers(&self) -> CliResult<Vec<ProviderInfo>>;

    /// Get detailed information about a specific provider
    fn get_provider_info(&self, provider_id: &str) -> CliResult<ProviderInfo>;

    /// Get metadata about a provider
    ///
    /// Returns structured metadata about capabilities, supported models, etc.
    fn get_provider_metadata(&self, provider_id: &str) -> CliResult<HashMap<String, serde_json::Value>>;

    /// Validate that a provider is properly configured
    ///
    /// Checks environment variables, WASM binary existence, etc.
    fn validate_provider(&self, provider_id: &str) -> CliResult<bool>;
}
