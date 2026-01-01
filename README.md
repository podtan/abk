# ABK (Agent Builder Kit)

**Modular utilities for building LLM agents**

ABK is a feature-gated Rust crate providing essential utilities for building LLM-based agents. Choose only the components you need via Cargo features.

# ABK (Agent Builder Kit)

**Complete modular agent building blocks with feature-gated modules**

ABK is a comprehensive Rust crate providing feature-gated modules for building LLM-based agents. Choose only the components you need via Cargo features to keep your builds lean and focused.

## Features

ABK provides feature-gated modules organized by functionality:

### Core Features
- **`config`** - TOML configuration loading and environment variable resolution
- **`observability`** - Structured logging with file/console output
- **`checkpoint`** - Session persistence and resume capabilities
- **`invoker`** - Unified abstraction for invocable operations (tools, MCP, A2A)

### Execution Features
- **`executor`** - Command execution with timeout and validation
- **`orchestration`** - Workflow coordination and session management
- **`lifecycle`** - WASM lifecycle plugin integration

### High-Level Features
- **`cli`** - Command-line interface utilities and formatting with convenience functions
- **`provider`** - LLM provider abstraction with WASM support
- **`agent`** - Complete agent implementation with all dependencies

### Composite Features
- **`all`** - Enables all features for complete functionality

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
# Enable only the features you need:
abk = { version = "0.1.24", features = ["config"] }

# Or enable multiple features:
abk = { version = "0.1.24", features = ["config", "observability", "executor"] }

# Or enable everything:
abk = { version = "0.1.24", features = ["all"] }
```

## Usage

### Configuration Feature

```rust
use abk::config::{ConfigurationLoader, EnvironmentLoader};
use std::path::Path;

// Load environment variables
let env = EnvironmentLoader::new(None);

// Load configuration from TOML
let config_loader = ConfigurationLoader::new(
    Some(Path::new("config/simpaticoder.toml"))
).unwrap();
let config = &config_loader.config;

// Access configuration
println!("Max iterations: {}", config.execution.max_iterations);
println!("LLM provider: {:?}", env.llm_provider());
```

### Observability Feature

```rust
use abk::observability::Logger;
use std::collections::HashMap;

// Create a logger with custom path and log level
let logger = Logger::new(
    Some(Path::new("logs/agent.md")),
    Some("DEBUG")
).unwrap();

// Log session lifecycle
let config = HashMap::new();
logger.log_session_start("auto", &config).unwrap();

// Log LLM interactions
let messages = vec![];
logger.log_llm_interaction(&messages, "Response text", "gpt-4").unwrap();

// Log completion
logger.log_completion("Task completed successfully").unwrap();
```

### CLI Feature

```rust
use abk::cli::{run_configured_cli_from_config, CommandContext};

// Option 1: One-liner convenience function (recommended for simple apps)
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_configured_cli_from_config("config/agent.toml").await
}

// Option 2: Full customization with CommandContext trait
struct MyContext { /* custom implementation */ }
impl CommandContext for MyContext { /* implement all methods */ }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = MyContext::new();
    let cli_config = CliConfig::from_simpaticoder_config(&context.config);
    run_configured_cli(&context, &cli_config).await
}
```

### Checkpoint Feature

```rust
use abk::checkpoint::{get_storage_manager, CheckpointResult};

// Initialize checkpoint storage
let manager = get_storage_manager()?;
let project_path = Path::new(".");
let project_storage = manager.get_project_storage(project_path).await?;

// Create a new session
let session_storage = project_storage.create_session("my-task").await?;
```

### Invoker Feature

```rust
use abk::invoker::{
    InvokerDefinition, InvokerSource, InvokerRegistry,
    DefaultInvokerRegistry, InvokerAdapter, StaticAdapter,
    generate_openai_tools,
};
use serde_json::json;

// Create a registry
let mut registry = DefaultInvokerRegistry::new();

// Register tools from different sources
registry.register(InvokerDefinition::new(
    "read_file",
    "Read the contents of a file",
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path" }
        },
        "required": ["path"]
    }),
    InvokerSource::Native,
)).unwrap();

// Use adapters to bulk-register tools
let mcp_adapter = StaticAdapter::new(
    InvokerSource::Mcp,
    vec![InvokerDefinition::new_simple("mcp_tool", "MCP tool", InvokerSource::Mcp)],
);
mcp_adapter.register_all(&mut registry).unwrap();

// Generate OpenAI-compatible tool schemas for LLM
let tools = generate_openai_tools(&registry);
println!("Registered {} tools", tools.len());

// Filter by source
let native_tools = registry.list_by_source(InvokerSource::Native);
let mcp_tools = registry.list_by_source(InvokerSource::Mcp);
```

### Provider Feature

```rust
use abk::provider::ProviderFactory;

// Create LLM provider from environment
let provider = ProviderFactory::create(&env)?;

// Generate text
let config = GenerateConfig {
    max_tokens: 100,
    temperature: 0.7,
    ..Default::default()
};
let response = provider.generate(&messages, &config).await?;
```

### Agent Feature

```rust
use abk::agent::Agent;

// Create a complete agent
let mut agent = Agent::new(
    Some(Path::new("config.toml")),
    Some(Path::new(".env")),
    Some(AgentMode::Confirm)
)?;

// Agent has access to all features:
// - Configuration via agent.config
// - Executor via agent.executor
// - Logger via agent.logger
// - Provider via agent.provider
// - Checkpoint manager via agent.session_manager
```

## Roadmap

ABK has evolved from a simple configuration utility to a comprehensive **Agent Builder Kit**:

- ✅ **Phase 1**: `config` feature (v0.1.0 - configuration management)
- ✅ **Phase 2**: `observability` feature (v0.1.1 - logging and metrics)
- ✅ **Phase 3**: `checkpoint` feature (v0.1.2 - session persistence)
- ✅ **Phase 4**: `provider` feature (v0.1.3+ - LLM provider abstraction)
- ✅ **Phase 5**: `executor` feature (v0.1.23 - command execution)
- ✅ **Phase 6**: `orchestration` feature (v0.1.23 - workflow management)
- ✅ **Phase 7**: `lifecycle` feature (v0.1.23 - WASM plugin integration)
- ✅ **Phase 8**: `cli` feature (v0.1.23 - command-line utilities)
- ✅ **Phase 9**: `agent` feature (v0.1.23 - complete agent implementation)
- ✅ **Phase 10**: `invoker` feature (v0.3.0 - unified tool abstraction for CATS/MCP/A2A)
- 🔄 **Phase 11**: `tools` feature (v0.3.0 - CATS adapter for invoker system)

## Why ABK?

ABK provides a **unified, modular foundation** for building LLM agents:

### 🏗️ **Modular Architecture**
- **Feature-gated modules**: Only compile what you need
- **Clean separation**: Each feature has focused responsibilities
- **Composable design**: Mix and match components as needed

### 📦 **Unified Package**
Instead of maintaining separate crates for each component, ABK unifies them under one package with feature flags:
- **Unified versioning** - One version number for all infrastructure utilities
- **Simplified dependencies** - Import one crate instead of nine
- **Coordinated releases** - Breaking changes managed together

### 🚀 **Production Ready**
- **Comprehensive testing** - Extensive test coverage for all features
- **Error handling** - Robust error types and recovery mechanisms
- **Performance optimized** - Efficient implementations with async support
- **Well documented** - Complete API documentation and examples

### 🔧 **Developer Experience**
- **Type safety** - Strongly typed APIs with compile-time guarantees
- **Intuitive APIs** - Easy-to-use interfaces following Rust conventions
- **Extensible design** - Easy to add new features and providers

## License

Dual-licensed under MIT OR Apache-2.0
