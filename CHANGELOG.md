# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.23] - 2025-11-06

### Added
- **Provider feature**: Complete LLM provider abstraction with WASM support
  - `LlmProvider` trait for unified provider interface
  - `ProviderFactory` for creating providers from configuration
  - `ChatMLAdapter` for message format conversion
  - `ToolAdapter` for tool representation conversion
  - `WasmProvider` for WebAssembly-based provider loading
  - Support for OpenAI, GitHub Copilot, and Anthropic backends
  - Streaming response support with SSE parsing
  - Environment-driven configuration via API keys and base URLs
  - Multi-backend routing through single WASM provider

- **Executor feature**: Command execution with timeout and validation
  - `CommandExecutor` for safe shell command execution
  - Configurable timeout handling
  - Command validation and safety checks
  - Retry logic with exponential backoff
  - Execution result tracking with stdout/stderr capture
  - Working directory management
  - Async execution with tokio

- **Orchestration feature**: Workflow coordination and session management
  - `AgentOrchestration` trait for workflow management
  - `WorkflowCoordinator` for coordinating agent workflows
  - `ToolCoordinator` for tool execution orchestration
  - Session lifecycle management
  - Workflow step tracking and state management
  - Message accumulation and delta handling
  - Template loading and rendering via WASM plugins

- **Lifecycle feature**: WASM lifecycle plugin integration
  - `LifecyclePlugin` for loading WASM lifecycle modules
  - Template management and rendering
  - Plugin discovery from filesystem locations
  - Async template operations
  - Error handling and validation

- **Agent feature**: Complete agent implementation
  - `Agent` struct with full agent functionality
  - Integration of all ABK features (config, observability, checkpoint, provider, executor, orchestration, lifecycle)
  - Tool registry management with CATS integration
  - Session management and checkpoint integration
  - Workflow execution and state tracking
  - Command execution coordination
  - Provider abstraction usage

- **CLI feature enhancements**: Complete command-line interface utilities
  - Command delegation and context management
  - CLI command implementations (run, init, config, cache, resume, checkpoints, sessions, misc)
  - Display utilities and formatting helpers
  - Table rendering and color output
  - Progress indicators and status displays

### Changed
- Updated package description to reflect comprehensive agent building kit
- Expanded feature set from 3 to 9 feature-gated modules
- Updated documentation to cover all new features
- Enhanced README with comprehensive usage examples
- Updated keywords and categories for broader applicability

### Features
- `config` - Configuration and environment loading
- `observability` - Logging and metrics
- `checkpoint` - Session and checkpoint management
- `provider` - LLM provider abstraction with WASM support
- `executor` - Command execution with timeout and validation
- `orchestration` - Workflow coordination and session management
- `lifecycle` - WASM lifecycle plugin integration
- `cli` - Command-line interface utilities and formatting
- `agent` - Complete agent implementation with all dependencies
- `all` - Enable all features

### Dependencies
- Added `async-trait` ^0.1 (optional, for provider and orchestration)
- Added `wasmtime` ^25 and `wasmtime-wasi` ^25 (optional, for provider and lifecycle)
- Added `reqwest` ^0.11 with stream feature (optional, for provider)
- Added `futures-util` ^0.3 (optional, for provider)
- Added `cats` ^0.1.2 (optional, for agent)
- Added `regex` ^1.0 (optional, for agent)
- Added `clap` ^4.0 with derive feature (optional, for cli)
- Added `comfy-table` ^7.0 (optional, for cli)
- Added `colored` ^2.0 (optional, for cli)
- Added `unicode-width` ^0.1 (optional, for cli)
- Added `dirs` ^5.0 (optional, for cli)
- Added `shellexpand` ^3.0 (optional, for cli)

## [0.1.2] - 2025-11-01

### Added
- **Checkpoint feature**: Merged complete agent-checkpoint functionality into abk
  - Session persistence and restoration
  - Checkpoint storage with compression support
  - Retention policies and automatic cleanup
  - Project isolation via hash-based directories
  - Atomic file operations with locking
  - Resume tracking across sessions
  - Storage size calculation and monitoring
  - Validation and restoration utilities
- Complete checkpoint module with all submodules:
  - `atomic` - Atomic file operations and locking
  - `cleanup` - Cleanup manager for expired data
  - `config` - Checkpoint configuration management
  - `errors` - Error types and result handling
  - `models` - Core checkpoint data models
  - `restoration` - Checkpoint restoration and validation
  - `resume_tracker` - Resume context tracking
  - `size_calc` - Storage size calculation utilities
  - `storage` - Storage manager and project/session storage
- Convenience functions: `initialize()`, `get_storage_manager()`, `cleanup_expired_data()`, `calculate_total_storage_usage()`
- Key checkpoint types re-exported in prelude module
- All data stored centrally in `~/.simpaticoder/` to avoid project pollution

### Changed
- Updated package description to include checkpointing
- Added `checkpoint` to the `all` feature flag

### Features
- `config` - Configuration and environment loading (from v0.1.0)
- `observability` - Logging and metrics (from v0.1.1)
- `cli` - CLI display utilities (from v0.1.1)
- `checkpoint` - Session and checkpoint management (new in v0.1.2)
- `all` - Enable all features

### Dependencies
- Added `thiserror` ^1.0 (optional, for checkpoint)
- Added `tokio` ^1.0 with fs and io-util features (optional, for checkpoint)
- Added `sha2` ^0.10 (optional, for checkpoint)
- Added `uuid` ^1.0 with v4 feature (optional, for checkpoint)
- Added `umf` ^0.1.0 (optional, for checkpoint)
- Added `tokio-test` ^0.4 (dev dependency)

## [0.1.1] - 2025-11-01

### Added
- **Observability feature**: Extracted logger implementation from simpaticoder
  - `Logger` struct for markdown-formatted logging
  - Session lifecycle logging
  - LLM interaction tracking
  - Command execution logging
  - Tool execution logging
  - Workflow iteration tracking
  - Error logging with context
  - Custom logging support
  - Debug-level message inspection (controlled by RUST_LOG environment variable)
- Comprehensive test suite for observability (5 tests)
- Documentation and usage examples for observability feature
- `Logger` re-exported in prelude module

### Changed
- Renamed from `trustee-config` to `abk` (Agent Builder Kit)
- Updated package metadata to reflect unified crate approach
- Updated documentation to reflect feature-gated architecture

### Features
- `config` - Configuration and environment loading (from v0.1.0)
- `observability` - Logging and metrics (new in v0.1.1)
- `cli` - CLI display utilities (placeholder for future)
- `all` - Enable all features

## [0.1.0] - 2025-11-01

### Added
- Initial release of trustee-config
- TOML configuration file parsing via `ConfigurationLoader`
- Environment variable loading via `EnvironmentLoader`
- Support for `.env` file loading
- Type-safe configuration structures:
  - `Configuration` - Main configuration
  - `AgentConfig` - Agent settings
  - `TemplateConfig` - Template paths
  - `LoggingConfig` - Logging configuration
  - `ExecutionConfig` - Execution limits
  - `ModesConfig` - Operation modes
  - `ToolsConfig` - Tool-specific settings
  - `SearchFilteringConfig` - Search filtering
  - `LlmConfig` - LLM provider configuration
- Path resolution helpers
- Validation and sensible defaults
- Comprehensive test suite (7 tests)
- Documentation and usage examples

### Features
- Load configuration from TOML files
- Load environment variables from `.env` files
- Provider selection via `LLM_PROVIDER` environment variable
- Default configuration generation
- Template path resolution
- Type-safe configuration access

[0.1.0]: https://github.com/AAG81/simpaticoder/releases/tag/trustee-config-v0.1.0
