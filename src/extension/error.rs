//! Extension error types

use std::fmt;

/// Result type for extension operations
pub type ExtensionResult<T> = Result<T, ExtensionError>;

/// Errors that can occur during extension operations
#[derive(Debug)]
pub enum ExtensionError {
    /// Manifest file not found
    ManifestNotFound(std::path::PathBuf),

    /// Invalid manifest format
    InvalidManifest(String),

    /// WASM loading error
    WasmLoadError(String),

    /// Incompatible API version
    IncompatibleVersion {
        /// Extension's API version
        extension_version: String,
        /// Host's API version
        host_version: String,
    },

    /// Capability not found
    CapabilityNotFound(String),

    /// Extension not found
    ExtensionNotFound(String),

    /// Extension call failed
    CallFailed(String),

    /// IO error
    IoError(String),

    /// Extension not loaded
    NotLoaded(String),

    /// Extension function call error
    CallError(String),

    /// Extension initialization error
    InitError(String),

    /// Lifecycle capability error
    LifecycleError(String),

    /// Provider capability error
    ProviderError(String),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtensionError::ManifestNotFound(path) => {
                write!(f, "Extension manifest not found: {:?}", path)
            }
            ExtensionError::InvalidManifest(msg) => {
                write!(f, "Invalid extension manifest: {}", msg)
            }
            ExtensionError::WasmLoadError(msg) => {
                write!(f, "WASM load error: {}", msg)
            }
            ExtensionError::IncompatibleVersion {
                extension_version,
                host_version,
            } => {
                write!(
                    f,
                    "Incompatible API version: extension {}, host {}",
                    extension_version, host_version
                )
            }
            ExtensionError::CapabilityNotFound(cap) => {
                write!(f, "Capability not found: {}", cap)
            }
            ExtensionError::ExtensionNotFound(id) => {
                write!(f, "Extension not found: {}", id)
            }
            ExtensionError::CallFailed(msg) => {
                write!(f, "Extension call failed: {}", msg)
            }
            ExtensionError::IoError(msg) => {
                write!(f, "IO error: {}", msg)
            }
            ExtensionError::NotLoaded(id) => {
                write!(f, "Extension not loaded: {}", id)
            }
            ExtensionError::CallError(msg) => {
                write!(f, "Extension call error: {}", msg)
            }
            ExtensionError::InitError(msg) => {
                write!(f, "Extension initialization error: {}", msg)
            }
            ExtensionError::LifecycleError(msg) => {
                write!(f, "Lifecycle error: {}", msg)
            }
            ExtensionError::ProviderError(msg) => {
                write!(f, "Provider error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ExtensionError {}

impl From<std::io::Error> for ExtensionError {
    fn from(err: std::io::Error) -> Self {
        ExtensionError::IoError(err.to_string())
    }
}

impl From<toml::de::Error> for ExtensionError {
    fn from(err: toml::de::Error) -> Self {
        ExtensionError::InvalidManifest(err.to_string())
    }
}

impl From<wasmtime::Error> for ExtensionError {
    fn from(err: wasmtime::Error) -> Self {
        ExtensionError::WasmLoadError(err.to_string())
    }
}
