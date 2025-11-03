//! Test utilities and mock implementations for CLI testing
//!
//! Provides mock adapter implementations for unit testing CLI commands
//! without requiring full application context.

pub mod mocks;

pub use mocks::{MockCommandContext, MockCheckpointAccess, MockProviderFactory, MockToolRegistry};
