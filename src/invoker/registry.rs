//! Registry trait and default implementation for managing invoker definitions.

use crate::invoker::{InvokerDefinition, InvokerError, InvokerSource};
use std::collections::HashMap;

/// A registry for managing invoker definitions.
///
/// This trait defines the interface for storing, retrieving, and managing
/// invoker definitions from multiple sources.
///
/// # Object Safety
///
/// This trait is object-safe and can be used with `dyn InvokerRegistry`.
///
/// # Example
///
/// ```
/// use abk::invoker::{InvokerRegistry, DefaultInvokerRegistry, InvokerDefinition, InvokerSource};
/// use serde_json::json;
///
/// let mut registry = DefaultInvokerRegistry::new();
///
/// let def = InvokerDefinition::new_simple("ping", "Ping service", InvokerSource::Native);
/// registry.register(def).unwrap();
///
/// assert!(registry.contains("ping"));
/// assert_eq!(registry.len(), 1);
/// ```
pub trait InvokerRegistry {
	/// Register an invoker definition.
	///
	/// Returns an error if an invoker with the same name already exists.
	fn register(&mut self, definition: InvokerDefinition) -> Result<(), InvokerError>;

	/// Get an invoker by name.
	///
	/// Returns `None` if the invoker is not found.
	fn get(&self, name: &str) -> Option<&InvokerDefinition>;

	/// List all registered invokers.
	fn list(&self) -> Vec<&InvokerDefinition>;

	/// List invokers from a specific source.
	fn list_by_source(&self, source: InvokerSource) -> Vec<&InvokerDefinition>;

	/// Check if an invoker exists.
	fn contains(&self, name: &str) -> bool;

	/// Remove an invoker, returning it if it existed.
	fn remove(&mut self, name: &str) -> Option<InvokerDefinition>;

	/// Get the number of registered invokers.
	fn len(&self) -> usize;

	/// Check if the registry is empty.
	fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Get all invoker names.
	fn names(&self) -> Vec<&str>;
}

/// Default implementation of `InvokerRegistry` using a HashMap.
///
/// This implementation provides O(1) lookup by name and is suitable
/// for most use cases.
///
/// # Example
///
/// ```
/// use abk::invoker::{DefaultInvokerRegistry, InvokerRegistry, InvokerDefinition, InvokerSource};
///
/// let mut registry = DefaultInvokerRegistry::new();
///
/// // Register some invokers
/// registry.register(InvokerDefinition::new_simple(
///     "tool_a",
///     "First tool",
///     InvokerSource::Native,
/// )).unwrap();
///
/// registry.register(InvokerDefinition::new_simple(
///     "tool_b",
///     "Second tool",
///     InvokerSource::Mcp,
/// )).unwrap();
///
/// // Query the registry
/// assert_eq!(registry.len(), 2);
/// assert_eq!(registry.list_by_source(InvokerSource::Native).len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct DefaultInvokerRegistry {
	invokers: HashMap<String, InvokerDefinition>,
}

impl DefaultInvokerRegistry {
	/// Create a new empty registry.
	pub fn new() -> Self {
		Self::default()
	}

	/// Create a registry with pre-allocated capacity.
	pub fn with_capacity(capacity: usize) -> Self {
		Self {
			invokers: HashMap::with_capacity(capacity),
		}
	}

	/// Register an invoker, replacing any existing one with the same name.
	///
	/// Unlike `register()`, this method does not return an error if the
	/// invoker already exists.
	pub fn register_or_replace(&mut self, definition: InvokerDefinition) -> Option<InvokerDefinition> {
		self.invokers.insert(definition.name.clone(), definition)
	}

	/// Get a mutable reference to an invoker.
	pub fn get_mut(&mut self, name: &str) -> Option<&mut InvokerDefinition> {
		self.invokers.get_mut(name)
	}

	/// Clear all invokers from the registry.
	pub fn clear(&mut self) {
		self.invokers.clear();
	}

	/// Iterate over all invokers.
	pub fn iter(&self) -> impl Iterator<Item = (&String, &InvokerDefinition)> {
		self.invokers.iter()
	}
}

impl InvokerRegistry for DefaultInvokerRegistry {
	fn register(&mut self, definition: InvokerDefinition) -> Result<(), InvokerError> {
		if self.invokers.contains_key(&definition.name) {
			return Err(InvokerError::duplicate_name(&definition.name));
		}
		self.invokers.insert(definition.name.clone(), definition);
		Ok(())
	}

	fn get(&self, name: &str) -> Option<&InvokerDefinition> {
		self.invokers.get(name)
	}

	fn list(&self) -> Vec<&InvokerDefinition> {
		self.invokers.values().collect()
	}

	fn list_by_source(&self, source: InvokerSource) -> Vec<&InvokerDefinition> {
		self.invokers
			.values()
			.filter(|def| def.source == source)
			.collect()
	}

	fn contains(&self, name: &str) -> bool {
		self.invokers.contains_key(name)
	}

	fn remove(&mut self, name: &str) -> Option<InvokerDefinition> {
		self.invokers.remove(name)
	}

	fn len(&self) -> usize {
		self.invokers.len()
	}

	fn names(&self) -> Vec<&str> {
		self.invokers.keys().map(|s| s.as_str()).collect()
	}
}

impl Clone for DefaultInvokerRegistry {
	fn clone(&self) -> Self {
		Self {
			invokers: self.invokers.clone(),
		}
	}
}

impl FromIterator<InvokerDefinition> for DefaultInvokerRegistry {
	fn from_iter<T: IntoIterator<Item = InvokerDefinition>>(iter: T) -> Self {
		let mut registry = Self::new();
		for def in iter {
			// Use register_or_replace to avoid errors on duplicates
			registry.register_or_replace(def);
		}
		registry
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_def(name: &str, source: InvokerSource) -> InvokerDefinition {
		InvokerDefinition::new_simple(name, format!("Description for {}", name), source)
	}

	#[test]
	fn test_new_registry() {
		let registry = DefaultInvokerRegistry::new();
		assert!(registry.is_empty());
		assert_eq!(registry.len(), 0);
	}

	#[test]
	fn test_register_and_get() {
		let mut registry = DefaultInvokerRegistry::new();

		let def = make_def("my_tool", InvokerSource::Native);
		registry.register(def).unwrap();

		let retrieved = registry.get("my_tool").unwrap();
		assert_eq!(retrieved.name, "my_tool");
		assert_eq!(retrieved.source, InvokerSource::Native);
	}

	#[test]
	fn test_register_duplicate() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("tool", InvokerSource::Native)).unwrap();

		let result = registry.register(make_def("tool", InvokerSource::Mcp));
		assert!(result.is_err());

		match result.unwrap_err() {
			InvokerError::DuplicateName { name } => assert_eq!(name, "tool"),
			_ => panic!("Expected DuplicateName error"),
		}
	}

	#[test]
	fn test_register_or_replace() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("tool", InvokerSource::Native)).unwrap();
		let old = registry.register_or_replace(make_def("tool", InvokerSource::Mcp));

		assert!(old.is_some());
		assert_eq!(old.unwrap().source, InvokerSource::Native);
		assert_eq!(registry.get("tool").unwrap().source, InvokerSource::Mcp);
	}

	#[test]
	fn test_list() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("a", InvokerSource::Native)).unwrap();
		registry.register(make_def("b", InvokerSource::Mcp)).unwrap();
		registry.register(make_def("c", InvokerSource::A2a)).unwrap();

		let list = registry.list();
		assert_eq!(list.len(), 3);
	}

	#[test]
	fn test_list_by_source() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("native1", InvokerSource::Native)).unwrap();
		registry.register(make_def("native2", InvokerSource::Native)).unwrap();
		registry.register(make_def("mcp1", InvokerSource::Mcp)).unwrap();
		registry.register(make_def("a2a1", InvokerSource::A2a)).unwrap();

		assert_eq!(registry.list_by_source(InvokerSource::Native).len(), 2);
		assert_eq!(registry.list_by_source(InvokerSource::Mcp).len(), 1);
		assert_eq!(registry.list_by_source(InvokerSource::A2a).len(), 1);
	}

	#[test]
	fn test_contains() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("exists", InvokerSource::Native)).unwrap();

		assert!(registry.contains("exists"));
		assert!(!registry.contains("missing"));
	}

	#[test]
	fn test_remove() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("tool", InvokerSource::Native)).unwrap();
		assert_eq!(registry.len(), 1);

		let removed = registry.remove("tool");
		assert!(removed.is_some());
		assert_eq!(removed.unwrap().name, "tool");
		assert_eq!(registry.len(), 0);

		// Remove non-existent
		let removed = registry.remove("missing");
		assert!(removed.is_none());
	}

	#[test]
	fn test_names() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("alpha", InvokerSource::Native)).unwrap();
		registry.register(make_def("beta", InvokerSource::Mcp)).unwrap();

		let names = registry.names();
		assert_eq!(names.len(), 2);
		assert!(names.contains(&"alpha"));
		assert!(names.contains(&"beta"));
	}

	#[test]
	fn test_clear() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("a", InvokerSource::Native)).unwrap();
		registry.register(make_def("b", InvokerSource::Native)).unwrap();
		assert_eq!(registry.len(), 2);

		registry.clear();
		assert!(registry.is_empty());
	}

	#[test]
	fn test_get_mut() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("tool", InvokerSource::Native)).unwrap();

		let def = registry.get_mut("tool").unwrap();
		def.description = "Updated description".to_string();

		assert_eq!(registry.get("tool").unwrap().description, "Updated description");
	}

	#[test]
	fn test_iter() {
		let mut registry = DefaultInvokerRegistry::new();

		registry.register(make_def("a", InvokerSource::Native)).unwrap();
		registry.register(make_def("b", InvokerSource::Mcp)).unwrap();

		let count = registry.iter().count();
		assert_eq!(count, 2);
	}

	#[test]
	fn test_clone() {
		let mut registry = DefaultInvokerRegistry::new();
		registry.register(make_def("tool", InvokerSource::Native)).unwrap();

		let cloned = registry.clone();
		assert_eq!(cloned.len(), 1);
		assert!(cloned.contains("tool"));
	}

	#[test]
	fn test_from_iterator() {
		let defs = vec![
			make_def("a", InvokerSource::Native),
			make_def("b", InvokerSource::Mcp),
			make_def("c", InvokerSource::A2a),
		];

		let registry: DefaultInvokerRegistry = defs.into_iter().collect();
		assert_eq!(registry.len(), 3);
	}

	#[test]
	fn test_with_capacity() {
		let registry = DefaultInvokerRegistry::with_capacity(100);
		assert!(registry.is_empty());
	}

	#[test]
	fn test_dyn_registry() {
		// Test that InvokerRegistry is object-safe
		fn use_registry(registry: &dyn InvokerRegistry) -> usize {
			registry.len()
		}

		let mut registry = DefaultInvokerRegistry::new();
		registry.register(make_def("tool", InvokerSource::Native)).unwrap();

		assert_eq!(use_registry(&registry), 1);
	}
}
