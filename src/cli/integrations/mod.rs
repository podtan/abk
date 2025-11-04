//! Simpaticoder-specific CLI integration
//!
//! This module provides simpaticoder-specific implementations of agent
//! construction and command handling. It's enabled via the `simpaticoder-cli` feature.

#[cfg(all(feature = "agent", feature = "simpaticoder-cli"))]
pub mod simpaticoder;
