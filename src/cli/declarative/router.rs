//! Command router - auto-routes from config to ABK adapter commands
//!
//! This is the core of the declarative CLI framework. It maintains a registry
//! of all ABK adapter commands and automatically routes based on config strings.

use super::error::{DeclarativeError, DeclarativeResult};
use clap::ArgMatches;
use std::collections::HashMap;
use std::sync::Arc;

/// Command router that maps config strings to ABK adapter functions
pub struct CommandRouter {
    /// Registry of all available ABK commands
    registry: HashMap<String, CommandHandler>,
}

/// Handler for a command
#[derive(Clone)]
pub enum CommandHandler {
    /// ABK adapter command (e.g., sessions::list)
    AbkCommand {
        category: String,
        command: String,
    },
    /// Special handler (e.g., agent::run)
    SpecialHandler {
        handler: String,
    },
    /// Built-in command (echo, version, help)
    Builtin {
        action: String,
    },
}

impl CommandRouter {
    /// Create a new router with all ABK commands registered
    pub fn new() -> Self {
        let mut registry = HashMap::new();
        
        // Register all ABK adapter commands
        
        // Sessions commands
        Self::register_abk(&mut registry, "sessions::list", "sessions", "list");
        Self::register_abk(&mut registry, "sessions::delete", "sessions", "delete");
        Self::register_abk(&mut registry, "sessions::show", "sessions", "show");
        Self::register_abk(&mut registry, "sessions::clean", "sessions", "clean");
        Self::register_abk(&mut registry, "sessions::prune", "sessions", "prune");
        
        // Checkpoints commands
        Self::register_abk(&mut registry, "checkpoints::list", "checkpoints", "list");
        Self::register_abk(&mut registry, "checkpoints::delete", "checkpoints", "delete");
        Self::register_abk(&mut registry, "checkpoints::show", "checkpoints", "show");
        Self::register_abk(&mut registry, "checkpoints::export", "checkpoints", "export");
        Self::register_abk(&mut registry, "checkpoints::import", "checkpoints", "import");
        Self::register_abk(&mut registry, "checkpoints::clean", "checkpoints", "clean");
        
        // Cache commands
        Self::register_abk(&mut registry, "cache::status", "cache", "status");
        Self::register_abk(&mut registry, "cache::clean", "cache", "clean");
        Self::register_abk(&mut registry, "cache::purge", "cache", "purge");
        
        // Config commands
        Self::register_abk(&mut registry, "config::show", "config", "show");
        Self::register_abk(&mut registry, "config::validate", "config", "validate");
        Self::register_abk(&mut registry, "config::edit", "config", "edit");
        
        // Resume commands
        Self::register_abk(&mut registry, "resume::latest", "resume", "latest");
        Self::register_abk(&mut registry, "resume::session", "resume", "session");
        Self::register_abk(&mut registry, "resume::checkpoint", "resume", "checkpoint");
        
        // Restore commands
        Self::register_abk(&mut registry, "restore::checkpoint", "restore", "checkpoint");
        Self::register_abk(&mut registry, "restore::session", "restore", "session");
        
        // History commands
        Self::register_abk(&mut registry, "history::show", "history", "show");
        Self::register_abk(&mut registry, "history::search", "history", "search");
        Self::register_abk(&mut registry, "history::export", "history", "export");
        
        // Logs commands
        Self::register_abk(&mut registry, "logs::show", "logs", "show");
        Self::register_abk(&mut registry, "logs::tail", "logs", "tail");
        Self::register_abk(&mut registry, "logs::clean", "logs", "clean");
        
        // Stats commands
        Self::register_abk(&mut registry, "stats::usage", "stats", "usage");
        Self::register_abk(&mut registry, "stats::tokens", "stats", "tokens");
        Self::register_abk(&mut registry, "stats::costs", "stats", "costs");
        
        // Tools commands (for simpaticoder)
        Self::register_abk(&mut registry, "tools::list", "tools", "list");
        Self::register_abk(&mut registry, "tools::describe", "tools", "describe");
        
        // Special handlers
        registry.insert(
            "agent::run".to_string(),
            CommandHandler::SpecialHandler {
                handler: "agent::run".to_string(),
            },
        );
        registry.insert(
            "tools::execute".to_string(),
            CommandHandler::SpecialHandler {
                handler: "tools::execute".to_string(),
            },
        );
        
        // Built-in commands
        registry.insert(
            "builtin::echo".to_string(),
            CommandHandler::Builtin {
                action: "echo".to_string(),
            },
        );
        registry.insert(
            "builtin::version".to_string(),
            CommandHandler::Builtin {
                action: "version".to_string(),
            },
        );
        
        Self { registry }
    }
    
    /// Register an ABK command in the registry
    fn register_abk(
        registry: &mut HashMap<String, CommandHandler>,
        key: &str,
        category: &str,
        command: &str,
    ) {
        registry.insert(
            key.to_string(),
            CommandHandler::AbkCommand {
                category: category.to_string(),
                command: command.to_string(),
            },
        );
    }
    
    /// Route a command from config to its handler
    pub fn route(
        &self,
        abk_command: Option<&str>,
        special_handler: Option<&str>,
        builtin: Option<&str>,
    ) -> DeclarativeResult<CommandHandler> {
        // Priority: builtin > special_handler > abk_command
        if let Some(builtin_name) = builtin {
            let key = format!("builtin::{}", builtin_name);
            return self.registry
                .get(&key)
                .cloned()
                .ok_or_else(|| DeclarativeError::routing(format!("Unknown builtin: {}", builtin_name)));
        }
        
        if let Some(handler) = special_handler {
            return self.registry
                .get(handler)
                .cloned()
                .ok_or_else(|| DeclarativeError::HandlerNotFound(handler.to_string()));
        }
        
        if let Some(command) = abk_command {
            return self.registry
                .get(command)
                .cloned()
                .ok_or_else(|| DeclarativeError::CommandNotFound(command.to_string()));
        }
        
        Err(DeclarativeError::routing("No handler specified for command"))
    }
    
    /// Check if a command exists in the registry
    pub fn has_command(&self, command: &str) -> bool {
        self.registry.contains_key(command)
    }
    
    /// Get all registered commands
    pub fn registered_commands(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Context for command execution
pub struct ExecutionContext {
    /// Parsed command-line arguments
    pub matches: ArgMatches,
    
    /// Command handler to execute
    pub handler: CommandHandler,
    
    /// Adapter instances (will be populated by executor)
    pub adapters: AdapterRegistry,
}

/// Registry of adapter instances
#[derive(Clone)]
pub struct AdapterRegistry {
    adapters: Arc<HashMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
}

impl AdapterRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            adapters: Arc::new(HashMap::new()),
        }
    }
    
    /// Insert an adapter
    pub fn insert<T: 'static + Send + Sync>(&mut self, name: String, adapter: T) {
        let mut adapters = (*self.adapters).clone();
        adapters.insert(name, Arc::new(adapter));
        self.adapters = Arc::new(adapters);
    }
    
    /// Get an adapter by name and type
    pub fn get<T: 'static + Send + Sync>(&self, name: &str) -> Option<Arc<T>> {
        self.adapters
            .get(name)
            .and_then(|a| a.clone().downcast::<T>().ok())
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_router_creation() {
        let router = CommandRouter::new();
        assert!(!router.registry.is_empty());
    }
    
    #[test]
    fn test_route_abk_command() {
        let router = CommandRouter::new();
        let handler = router.route(Some("sessions::list"), None, None).unwrap();
        
        match handler {
            CommandHandler::AbkCommand { category, command } => {
                assert_eq!(category, "sessions");
                assert_eq!(command, "list");
            }
            _ => panic!("Expected ABK command handler"),
        }
    }
    
    #[test]
    fn test_route_special_handler() {
        let router = CommandRouter::new();
        let handler = router.route(None, Some("agent::run"), None).unwrap();
        
        match handler {
            CommandHandler::SpecialHandler { handler } => {
                assert_eq!(handler, "agent::run");
            }
            _ => panic!("Expected special handler"),
        }
    }
    
    #[test]
    fn test_route_builtin() {
        let router = CommandRouter::new();
        let handler = router.route(None, None, Some("echo")).unwrap();
        
        match handler {
            CommandHandler::Builtin { action } => {
                assert_eq!(action, "echo");
            }
            _ => panic!("Expected builtin handler"),
        }
    }
    
    #[test]
    fn test_route_unknown_command() {
        let router = CommandRouter::new();
        let result = router.route(Some("unknown::command"), None, None);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_has_command() {
        let router = CommandRouter::new();
        assert!(router.has_command("sessions::list"));
        assert!(router.has_command("agent::run"));
        assert!(!router.has_command("nonexistent::command"));
    }
    
    #[test]
    fn test_registered_commands() {
        let router = CommandRouter::new();
        let commands = router.registered_commands();
        assert!(commands.contains(&"sessions::list".to_string()));
        assert!(commands.contains(&"checkpoints::list".to_string()));
        assert!(commands.contains(&"agent::run".to_string()));
    }
}
