//! CLI command implementations
//!
//! Generic command implementations that work via adapter traits.
//! Commands are organized by functionality and feature-gated appropriately.

pub mod misc;
pub mod init;
pub mod sessions;

// Future command modules (will be added during extraction)
// pub mod config;
// pub mod cache;
// pub mod checkpoints;
