# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
