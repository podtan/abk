# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
