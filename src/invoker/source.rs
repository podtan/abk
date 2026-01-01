//! Invoker source types for identifying where an invoker originated.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies the origin of an invoker definition.
///
/// This enum is used to track where an invocable operation came from,
/// enabling proper routing during execution.
///
/// # Example
///
/// ```
/// use abk::invoker::InvokerSource;
///
/// let source = InvokerSource::Native;
/// assert_eq!(source.to_string(), "native");
///
/// let source = InvokerSource::Mcp;
/// assert_eq!(source.to_string(), "mcp");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvokerSource {
	/// Local tool from CATS or similar native registry.
	///
	/// These are tools that run directly in the same process,
	/// typically registered from the CATS crate.
	Native,

	/// Remote tool from an MCP (Model Context Protocol) server.
	///
	/// These tools are discovered and invoked via JSON-RPC
	/// over stdio or HTTP/SSE transport.
	Mcp,

	/// Skill from a peer agent via A2A (Agent-to-Agent) protocol.
	///
	/// These represent capabilities of other agents that can
	/// be delegated to via the A2A task protocol.
	A2a,
}

impl fmt::Display for InvokerSource {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Native => write!(f, "native"),
			Self::Mcp => write!(f, "mcp"),
			Self::A2a => write!(f, "a2a"),
		}
	}
}

impl Default for InvokerSource {
	fn default() -> Self {
		Self::Native
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_display() {
		assert_eq!(InvokerSource::Native.to_string(), "native");
		assert_eq!(InvokerSource::Mcp.to_string(), "mcp");
		assert_eq!(InvokerSource::A2a.to_string(), "a2a");
	}

	#[test]
	fn test_serde_roundtrip() {
		let sources = [InvokerSource::Native, InvokerSource::Mcp, InvokerSource::A2a];

		for source in sources {
			let json = serde_json::to_string(&source).unwrap();
			let parsed: InvokerSource = serde_json::from_str(&json).unwrap();
			assert_eq!(source, parsed);
		}
	}

	#[test]
	fn test_serde_lowercase() {
		assert_eq!(serde_json::to_string(&InvokerSource::Native).unwrap(), "\"native\"");
		assert_eq!(serde_json::to_string(&InvokerSource::Mcp).unwrap(), "\"mcp\"");
		assert_eq!(serde_json::to_string(&InvokerSource::A2a).unwrap(), "\"a2a\"");
	}

	#[test]
	fn test_equality_and_hash() {
		use std::collections::HashSet;

		let mut set = HashSet::new();
		set.insert(InvokerSource::Native);
		set.insert(InvokerSource::Mcp);
		set.insert(InvokerSource::A2a);

		assert_eq!(set.len(), 3);
		assert!(set.contains(&InvokerSource::Native));
		assert!(set.contains(&InvokerSource::Mcp));
		assert!(set.contains(&InvokerSource::A2a));
	}

	#[test]
	fn test_default() {
		assert_eq!(InvokerSource::default(), InvokerSource::Native);
	}

	#[test]
	fn test_copy() {
		let source = InvokerSource::Mcp;
		let copied = source; // Copy, not move
		assert_eq!(source, copied);
	}
}
