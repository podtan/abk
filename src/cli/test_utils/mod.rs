//! Test utilities and mock implementations for CLI testing
//!
//! Provides mock adapter implementations for unit testing CLI commands
//! without requiring full application context.

pub mod mocks;

pub use mocks::{MockCommandContext, MockCheckpointAccess, MockProviderFactory, MockToolRegistry};

/// Ensure the test environment is set up for unit tests that rely on
/// environment variables. Returns a guard that restores the previous
/// environment variables (notably `RUST_LOG`) when dropped.
pub fn setup_env() -> TestEnvGuard {
    TestEnvGuard::new()
}

/// Guard that restores environment variables on drop.
///
/// The main purpose is to pin `RUST_LOG` to a predictable, non-debug value
/// for the duration of a test, since several code paths (e.g.
/// `Agent::new_from_config`) branch on `RUST_LOG=debug`.
pub struct TestEnvGuard {
    restored: Vec<(String, Option<String>)>,
}

impl TestEnvGuard {
    pub fn new() -> Self {
        let restored = vec![(
            "RUST_LOG".to_string(),
            std::env::var("RUST_LOG").ok(),
        )];
        std::env::set_var("RUST_LOG", "info");
        TestEnvGuard { restored }
    }
}

impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.restored.iter() {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}
