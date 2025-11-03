//! CLI utilities and commands for agent applications
//!
//! This module provides reusable CLI infrastructure for building agent applications,
//! including:
//! - Display utilities (panels, progress indicators, formatters)
//! - File and directory utilities
//! - Color and styling helpers

#[cfg(feature = "cli")]
pub mod utils;

#[cfg(feature = "cli")]
pub use utils::*;
