# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
