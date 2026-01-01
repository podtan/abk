//! Adapter trait for discovering and registering invokers from external sources.

use crate::invoker::{InvokerDefinition, InvokerError, InvokerRegistry, InvokerSource};

/// Adapter for discovering invokers from an external source.
///
/// Implementations of this trait provide a bridge between external tool
/// sources (CATS, MCP servers, A2A agents) and the invoker registry.
///
/// # Object Safety
///
/// This trait is object-safe and can be used with `dyn InvokerAdapter`.
///
/// # Example
///
/// ```
/// use abk::invoker::{
///     InvokerAdapter, InvokerDefinition, InvokerError, InvokerRegistry,
///     InvokerSource, DefaultInvokerRegistry,
/// };
///
/// struct MockAdapter {
///     tools: Vec<InvokerDefinition>,
/// }
///
/// impl InvokerAdapter for MockAdapter {
///     fn source(&self) -> InvokerSource {
///         InvokerSource::Native
///     }
///
///     fn discover(&self) -> Result<Vec<InvokerDefinition>, InvokerError> {
///         Ok(self.tools.clone())
///     }
/// }
///
/// // Use the adapter
/// let adapter = MockAdapter {
///     tools: vec![InvokerDefinition::new_simple("ping", "Ping", InvokerSource::Native)],
/// };
///
/// let mut registry = DefaultInvokerRegistry::new();
/// let count = adapter.register_all(&mut registry).unwrap();
/// assert_eq!(count, 1);
/// ```
pub trait InvokerAdapter {
	/// Get the source type this adapter provides.
	///
	/// All invokers discovered by this adapter will have this source type.
	fn source(&self) -> InvokerSource;

	/// Discover available invokers from this source.
	///
	/// This method queries the external source and returns a list of
	/// invoker definitions. The implementation should handle connection
	/// errors and return appropriate `InvokerError` variants.
	fn discover(&self) -> Result<Vec<InvokerDefinition>, InvokerError>;

	/// Register all discovered invokers into a registry.
	///
	/// This is a convenience method that calls `discover()` and registers
	/// each result. Returns the number of successfully registered invokers.
	///
	/// # Default Implementation
	///
	/// The default implementation iterates over discovered invokers and
	/// registers each one. If registration fails (e.g., due to duplicate
	/// names), the error is returned immediately.
	fn register_all(&self, registry: &mut dyn InvokerRegistry) -> Result<usize, InvokerError> {
		let definitions = self.discover()?;
		let count = definitions.len();
		for def in definitions {
			registry.register(def)?;
		}
		Ok(count)
	}

	/// Register all discovered invokers, skipping duplicates.
	///
	/// Unlike `register_all`, this method continues even if some
	/// registrations fail due to duplicate names. Returns the number
	/// of successfully registered invokers.
	fn register_all_skip_duplicates(
		&self,
		registry: &mut dyn InvokerRegistry,
	) -> Result<usize, InvokerError> {
		let definitions = self.discover()?;
		let mut count = 0;
		for def in definitions {
			if registry.register(def).is_ok() {
				count += 1;
			}
		}
		Ok(count)
	}
}

/// A simple adapter that returns a fixed list of invoker definitions.
///
/// Useful for testing or for wrapping static tool lists.
///
/// # Example
///
/// ```
/// use abk::invoker::{StaticAdapter, InvokerAdapter, InvokerDefinition, InvokerSource};
///
/// let adapter = StaticAdapter::new(
///     InvokerSource::Native,
///     vec![
///         InvokerDefinition::new_simple("tool_a", "Tool A", InvokerSource::Native),
///         InvokerDefinition::new_simple("tool_b", "Tool B", InvokerSource::Native),
///     ],
/// );
///
/// let discovered = adapter.discover().unwrap();
/// assert_eq!(discovered.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct StaticAdapter {
	source: InvokerSource,
	definitions: Vec<InvokerDefinition>,
}

impl StaticAdapter {
	/// Create a new static adapter with the given definitions.
	pub fn new(source: InvokerSource, definitions: Vec<InvokerDefinition>) -> Self {
		Self { source, definitions }
	}

	/// Create an empty static adapter.
	pub fn empty(source: InvokerSource) -> Self {
		Self::new(source, Vec::new())
	}

	/// Add a definition to this adapter.
	pub fn add(&mut self, definition: InvokerDefinition) {
		self.definitions.push(definition);
	}

	/// Get the number of definitions in this adapter.
	pub fn len(&self) -> usize {
		self.definitions.len()
	}

	/// Check if this adapter is empty.
	pub fn is_empty(&self) -> bool {
		self.definitions.is_empty()
	}
}

impl InvokerAdapter for StaticAdapter {
	fn source(&self) -> InvokerSource {
		self.source
	}

	fn discover(&self) -> Result<Vec<InvokerDefinition>, InvokerError> {
		Ok(self.definitions.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::invoker::DefaultInvokerRegistry;

	fn make_def(name: &str, source: InvokerSource) -> InvokerDefinition {
		InvokerDefinition::new_simple(name, format!("Description for {}", name), source)
	}

	#[test]
	fn test_static_adapter_source() {
		let adapter = StaticAdapter::empty(InvokerSource::Mcp);
		assert_eq!(adapter.source(), InvokerSource::Mcp);
	}

	#[test]
	fn test_static_adapter_discover() {
		let adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![
				make_def("a", InvokerSource::Native),
				make_def("b", InvokerSource::Native),
			],
		);

		let discovered = adapter.discover().unwrap();
		assert_eq!(discovered.len(), 2);
	}

	#[test]
	fn test_static_adapter_add() {
		let mut adapter = StaticAdapter::empty(InvokerSource::Native);
		assert!(adapter.is_empty());

		adapter.add(make_def("tool", InvokerSource::Native));
		assert_eq!(adapter.len(), 1);
	}

	#[test]
	fn test_register_all() {
		let adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![
				make_def("a", InvokerSource::Native),
				make_def("b", InvokerSource::Native),
			],
		);

		let mut registry = DefaultInvokerRegistry::new();
		let count = adapter.register_all(&mut registry).unwrap();

		assert_eq!(count, 2);
		assert_eq!(registry.len(), 2);
		assert!(registry.contains("a"));
		assert!(registry.contains("b"));
	}

	#[test]
	fn test_register_all_duplicate_error() {
		let adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![
				make_def("dup", InvokerSource::Native),
				make_def("dup", InvokerSource::Native), // duplicate
			],
		);

		let mut registry = DefaultInvokerRegistry::new();
		let result = adapter.register_all(&mut registry);

		assert!(result.is_err());
		match result.unwrap_err() {
			InvokerError::DuplicateName { name } => assert_eq!(name, "dup"),
			_ => panic!("Expected DuplicateName error"),
		}
	}

	#[test]
	fn test_register_all_skip_duplicates() {
		let adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![
				make_def("a", InvokerSource::Native),
				make_def("a", InvokerSource::Native), // duplicate
				make_def("b", InvokerSource::Native),
			],
		);

		let mut registry = DefaultInvokerRegistry::new();
		let count = adapter.register_all_skip_duplicates(&mut registry).unwrap();

		// Only 2 should be registered (first "a" and "b")
		assert_eq!(count, 2);
		assert_eq!(registry.len(), 2);
	}

	#[test]
	fn test_multiple_adapters() {
		let native_adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![make_def("native_tool", InvokerSource::Native)],
		);

		let mcp_adapter = StaticAdapter::new(
			InvokerSource::Mcp,
			vec![make_def("mcp_tool", InvokerSource::Mcp)],
		);

		let mut registry = DefaultInvokerRegistry::new();
		native_adapter.register_all(&mut registry).unwrap();
		mcp_adapter.register_all(&mut registry).unwrap();

		assert_eq!(registry.len(), 2);
		assert_eq!(registry.list_by_source(InvokerSource::Native).len(), 1);
		assert_eq!(registry.list_by_source(InvokerSource::Mcp).len(), 1);
	}

	#[test]
	fn test_dyn_adapter() {
		// Test that InvokerAdapter is object-safe
		fn use_adapter(adapter: &dyn InvokerAdapter) -> InvokerSource {
			adapter.source()
		}

		let adapter = StaticAdapter::empty(InvokerSource::A2a);
		assert_eq!(use_adapter(&adapter), InvokerSource::A2a);
	}

	#[test]
	fn test_clone() {
		let adapter = StaticAdapter::new(
			InvokerSource::Native,
			vec![make_def("tool", InvokerSource::Native)],
		);

		let cloned = adapter.clone();
		assert_eq!(cloned.len(), 1);
		assert_eq!(cloned.source(), InvokerSource::Native);
	}
}
