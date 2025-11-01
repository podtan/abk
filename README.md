# ABK (Agent Builder Kit)

**Modular utilities for building LLM agents**

ABK is a feature-gated Rust crate providing essential utilities for building LLM-based agents. Choose only the components you need via Cargo features.

## Features

ABK provides three main feature modules:

### `config` - Configuration Management
- TOML configuration file parsing
- Environment variable loading via `.env` files
- Type-safe configuration structures
- Path resolution helpers
- Validation and defaults

### `observability` - Logging & Metrics
*Coming soon - will be extracted from simpaticoder*
- Structured logging
- Metrics collection
- Distributed tracing

### `cli` - CLI Display Utilities
*Coming soon - will be extracted from simpaticoder*
- Panel and message box rendering
- Time formatting utilities
- Text truncation and formatting
- Color and styling helpers

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
# Enable only the features you need:
abk = { version = "0.1", features = ["config"] }

# Or enable multiple features:
abk = { version = "0.1", features = ["config", "observability"] }

# Or enable everything:
abk = { version = "0.1", features = ["all"] }
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

### Configuration Structure

The `Configuration` struct includes:

- **Agent**: Core agent settings (name, version, user agent)
- **Templates**: Template paths and settings
- **Logging**: Logging configuration (level, format, targets)
- **Execution**: Execution limits (timeout, retries, max iterations)
- **Modes**: Operation modes (auto-approve, dry-run, verbose)
- **Tools**: Tool-specific configuration (file window size, result limits)
- **Search Filtering**: Directory and file filtering for search operations
- **LLM**: LLM provider configuration (endpoint, streaming)

### Environment Variables

The `EnvironmentLoader` handles:

- `.env` file loading (if provided)
- System environment variable access
- Provider selection via `LLM_PROVIDER`

## Roadmap

ABK is part of the larger Trustee ecosystem extraction from [simpaticoder](https://github.com/podtan/simpaticoder).

- ✅ Phase 1: `config` feature (extracted from trustee-config)
- ⏳ Phase 2: `observability` feature (extracting from simpaticoder/src/logger)
- ⏳ Phase 3: `cli` feature (extracting from simpaticoder/src/cli/commands/utils.rs)

## Why ABK?

Instead of maintaining separate `trustee-config`, `trustee-observability`, and `trustee-cli` crates, ABK unifies them under one package with feature flags. This provides:

- **Unified versioning** - One version number for all infrastructure utilities
- **Simplified dependencies** - Import one crate instead of three
- **Modular builds** - Only compile what you use
- **Coordinated releases** - Breaking changes managed together

## License

Dual-licensed under MIT OR Apache-2.0
