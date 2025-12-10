//! Storage Backend Abstraction
//!
//! This module provides a trait-based abstraction for checkpoint storage backends.
//! The default implementation uses local filesystem storage, but alternative backends
//! (DocumentDB, S3, etc.) can be implemented.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────┐
//! │   SessionStorage    │
//! │  (high-level API)   │
//! └──────────┬──────────┘
//!            │
//! ┌──────────▼──────────┐
//! │   StorageBackend    │  <-- Trait
//! │      (async)        │
//! └──────────┬──────────┘
//!            │
//!     ┌──────┴──────┐
//!     │             │
//! ┌───▼───┐   ┌─────▼─────┐
//! │ File  │   │ DocumentDB│
//! │Backend│   │  Backend  │
//! └───────┘   └───────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use abk::checkpoint::backend::{StorageBackend, FileStorageBackend};
//!
//! async fn example() -> anyhow::Result<()> {
//!     // Use default file storage
//!     let backend = FileStorageBackend::new("/path/to/storage")?;
//!     
//!     // Write data
//!     backend.write("key", b"value").await?;
//!     
//!     // Read data
//!     let data = backend.read("key").await?;
//!     
//!     Ok(())
//! }
//! ```

mod traits;
mod file_backend;

pub use traits::*;
pub use file_backend::*;

#[cfg(feature = "storage-documentdb")]
mod documentdb_backend;

#[cfg(feature = "storage-documentdb")]
pub use documentdb_backend::DocumentDBStorageBackend;
