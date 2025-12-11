# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.53] - 2025-12-11

### Fixed
- **list_projects() respects storage_mode**: Only queries configured storage sources
  - `StorageMode::Local` - Only local filesystem
  - `StorageMode::Remote` - Only remote backend (DocumentDB)
  - `StorageMode::Mirror` - Both local and remote
  - Fixes issue where local projects were always queried regardless of storage_mode

- **Session metadata checkpoint_count**: Session metadata is now updated in remote storage
  - `checkpoint_count` now correctly incremented when saving checkpoints to remote
  - Enables `trustee resume` to show sessions with their actual checkpoint counts
  - Previously, sessions saved to remote had `checkpoint_count: 0`

### Removed
- Debug eprintln statements (kept connection messages and warnings)

## [0.1.52] - 2025-12-11

### Changed
- **DocumentDB key format**: Updated to proper hierarchical structure
  - Old format: `sessions/{session_id}/{checkpoint_id}_{type}.json`
  - New format: `projects/{project_hash}/sessions/{session_id}/checkpoints/{checkpoint_id}_{type}.json`
  - Enables proper project-level organization and querying

### Added
- **Project metadata in remote**: Project metadata now saved to remote backend
  - Key: `projects/{project_hash}/metadata.json`
- **Session metadata in remote**: Session metadata now saved to remote backend
  - Key: `projects/{project_hash}/sessions/{session_id}/metadata.json`
- Session creation respects storage_mode (no local dirs in remote-only mode)

### Fixed
- `list_projects()` now correctly queries for `projects/*/metadata.json`
- `load_sessions_from_remote()` uses project-scoped prefix for session discovery
- `try_load_from_remote()` uses correct hierarchical path
- `save_checkpoint()` uses project hash in remote key path

## [0.1.51] - 2025-12-10

### Fixed
- **Remote project discovery**: `list_projects()` now queries remote backend for project metadata
  - Projects stored only in DocumentDB are now discoverable
  - Merges local and remote projects, deduplicates by project_hash
  - Enables `trustee resume` to find projects from DocumentDB when storage_mode=remote

## [0.1.50] - 2025-12-10

### Fixed
- **CLI checkpoint operations**: All checkpoint operations now use configured storage backend
  - Refactored `AbkCheckpointAccess` to use `get_configured_storage_manager()` helper
  - `list_projects()` now loads checkpoint config from agent config file
  - `list_sessions()`, `list_checkpoints()`, `delete_session()` all use configured backend
  - Enables `trustee resume` to discover projects/sessions from DocumentDB
  - Fixed bug where only `list_sessions()` was using remote backend, but `list_projects()` wasn't

## [0.1.49] - 2025-12-10

### Fixed
- **CLI resume**: `list_sessions()` now loads checkpoint config from agent config file
  - Reads `~/.{agent_name}/config/{agent_name}.toml` to get storage_backend config
  - Enables remote session discovery for `trustee resume` command
  - Falls back to local-only storage if config not found

## [0.1.48] - 2025-12-10

### Added
- **Remote session listing**: `list_sessions()` now queries remote backend for sessions
  - Enables `trustee resume` to find sessions stored only in DocumentDB
  - Merges local and remote sessions, deduplicates by session_id

### Fixed  
- Resume functionality now works with remote-only storage mode

## [0.1.47] - 2025-12-10

### Added
- **StorageMode**: New enum for controlling checkpoint storage behavior
  - `Local`: Only write to local filesystem (default for file backend)
  - `Remote`: Only write to remote backend (no local files)
  - `Mirror`: Write to both local and remote (default when remote backend configured)

- **storage_mode config option**: New field in `StorageBackendConfig`
  ```toml
  [checkpointing.storage_backend]
  backend_type = "documentdb"
  storage_mode = "remote"  # or "mirror" or "local"
  ```

- **Remote checkpoint loading**: `load_checkpoint()` now supports loading from remote backend
  - Tries local storage first (for Mirror mode)
  - Falls back to remote backend if local doesn't exist
  - Enables `trustee resume` to work with DocumentDB-only storage

### Changed
- **save_checkpoint()**: Now respects storage_mode
  - `Local`: Only writes to local files
  - `Remote`: Only writes to remote backend (no local files created)
  - `Mirror`: Writes to both local and remote

- **effective_storage_mode()**: Helper method on StorageBackendConfig
  - Automatically determines best mode based on backend_type
  - Remote backend configured + Local mode → defaults to Mirror for safety

- **SessionStorage/ProjectStorage**: Now track storage_mode
  - Passed down from CheckpointStorageManager through ProjectStorage to SessionStorage

### Notes
- Default behavior (Mirror) maintains backward compatibility
- Use `storage_mode = "remote"` for pure DocumentDB storage (no local files)
- Resume works with any storage mode - checks remote if local doesn't exist

## [0.1.46] - 2025-12-10

### Changed
- **Config Deserialization**: Added `#[serde(default)]` to all checkpoint config structs
  - `GlobalCheckpointConfig`, `RetentionPolicy`, `GitIntegrationConfig`, 
    `PerformanceConfig`, `SecurityConfig`, `LoggingConfig`
  - Allows partial config from TOML files (missing fields use defaults)
  - Fixes "Failed to parse checkpoint config" error when config has subset of fields

- **Agent Backend Init**: Fixed config parsing in `initialize_remote_checkpoint_backend()`
  - Now properly re-serializes TOML section before deserializing to struct
  - Handles partial checkpoint configurations correctly

## [0.1.45] - 2025-12-10

### Added
- **Agent.initialize_remote_checkpoint_backend()**: New async method to initialize remote storage backend
  - Loads checkpoint config from agent's TOML config file
  - Initializes DocumentDB/MongoDB connection for checkpoint mirroring
  - Can be called after agent creation to enable remote storage

- **SessionManager.initialize_remote_backend()**: New async method to set up remote backend
  - Enables remote backend initialization on existing SessionManager
  - Alternative to using `SessionManager::with_config()` constructor

- **SessionManager.with_config()**: New async constructor (feature-gated on `storage-documentdb`)
  - Creates SessionManager with remote backend from GlobalCheckpointConfig
  - Enables DocumentDB/MongoDB support from config

### Changed
- CLI `run` command now initializes remote checkpoint backend from config
  - Automatic DocumentDB/MongoDB backend initialization when configured
  - Graceful fallback with info message if backend initialization fails

### Notes
- Remote backend is initialized asynchronously after agent creation
- Config file location: `~/.{agent_name}/config/{agent_name}.toml`
- Requires `storage-documentdb` feature flag

## [0.1.44] - 2025-12-10

### Added
- **End-to-End Backend Integration**: Storage backend now fully integrated through entire checkpoint pipeline
  - `SessionStorage` now accepts and uses remote backend for checkpoint mirroring
  - `ProjectStorage` now holds and passes remote backend to sessions
  - `CheckpointStorageManager.with_config_async()` initializes backend from config
  - `get_project_storage()` now passes remote backend to project storage

### Changed
- `SessionStorage` struct:
  - New `remote_backend` field (feature-gated on `storage-documentdb`)
  - New `with_remote_backend()` constructor for remote backend support
  - New `set_remote_backend()` method to set backend after creation
  - `save_checkpoint()` now mirrors to remote backend when configured
  
- `ProjectStorage` struct:
  - New `remote_backend` field (feature-gated on `storage-documentdb`)
  - New `with_remote_backend()` constructor
  - New `set_remote_backend()` method
  - `create_session()` now passes remote backend to sessions

- `CheckpointStorageManager`:
  - `get_project_storage()` now creates `ProjectStorage` with remote backend

### Notes
- Checkpoints are written to both local file system AND remote backend (mirroring)
- Remote writes are non-blocking with warnings on failure
- Local filesystem remains primary storage for backward compatibility

## [0.1.43] - 2025-12-10

### Added
- **Storage Backend Configuration**: Configuration-driven storage backend selection
  - `StorageBackendType` enum: `File`, `DocumentDB`, `MongoDB`
  - `StorageBackendConfig` struct with fields:
    - `backend_type`: Select storage backend from config
    - `connection_url`: Connection URL with env var substitution (`${DOCUMENTDB_URL}`)
    - `database`: Database name for remote backends
    - `collection`: Collection name (default: "checkpoints")
    - `username`, `password`: Credentials with env var substitution
    - `tls_enabled`, `tls_allow_invalid_certs`: TLS configuration
    - `connection_timeout_secs`: Connection timeout
  - `build_connection_string()`: Builds MongoDB URI with credentials and TLS options
  - Environment variable substitution: `${VAR_NAME}` syntax in config values

- **GlobalCheckpointConfig.storage_backend**: New field for backend configuration
  - Agents can select backend via TOML config without code changes
  - Default: File backend for backward compatibility

- **Integration Tests**: 4 new tests for DocumentDB backend
  - Connection test
  - CRUD operations test
  - JSON serialization test
  - List operations test

### Changed
- `checkpoint` feature now includes `regex` and `urlencoding` dependencies
- `storage-documentdb` feature now includes `futures-util` dependency
- New re-exports: `StorageBackendConfig`, `StorageBackendType`

### Notes
- Tested with local DocumentDB container:
  ```bash
  docker run -dt -p 10260:10260 --name documentdb-container documentdb --username trustee --password abk12345
  ```
- All 4 DocumentDB integration tests pass

## [0.1.42] - 2025-12-10

### Added
- **Storage Backend Abstraction**: New pluggable storage backend system for checkpoint storage
  - `StorageBackend` trait: Core async trait for storage operations (read, write, delete, list, exists, metadata)
  - `StorageBackendExt` trait: Extension trait for JSON serialization/deserialization helpers
  - `FileStorageBackend`: Default file system implementation with atomic writes
  - `StorageError`: Comprehensive error types for storage operations (IO, NotFound, Serialization, Connection, etc.)
  - `StorageItemMeta`: Metadata struct for stored items (key, size, modified_at, content_type)
  - `ListOptions` / `ListResult`: Pagination support for listing operations
  - `StorageBackendBuilder`: Builder pattern for creating backends from configuration

- **DocumentDB Storage Backend** (feature-gated: `storage-documentdb`):
  - `DocumentDBStorageBackend`: MongoDB/DocumentDB implementation for cloud-native checkpoint storage
  - Supports distributed agents and multi-region deployments
  - Enable with: `abk = { features = ["checkpoint", "storage-documentdb"] }`

### Changed
- `checkpoint` feature now includes `async-trait` dependency for backend trait support
- New re-exports from `checkpoint` module: `FileStorageBackend`, `StorageBackend`, `StorageBackendExt`, `StorageError`, `StorageResult`, `ListOptions`, `ListResult`, `StorageItemMeta`, `StorageBackendBuilder`

### Notes
- FileStorageBackend includes atomic write support (temp file + rename pattern)
- Directory traversal prevention in file backend
- 9 new tests for storage backend module

## [0.1.41] - 2025-12-10

### Fixed
- **Checkpoint restoration with V2 format**: Fixed `CheckpointRestoration::load_checkpoint()` to use `SessionStorage::load_checkpoint()` instead of direct file reading
  - This properly handles the V2 split-file format (`{id}_metadata.json`, `{id}_agent.json`, `{id}_conversation.json`)
  - Previously it was directly reading the metadata file and failing with "missing field `metadata`" error
  - Resume functionality now works correctly with V2 checkpoints

## [0.1.40] - 2025-12-10

### Fixed
- **Import path fix**: Fixed `super::` import to `super::models::` for internal types in `storage.rs`
  - Affected types: `ExecutionContext`, `FilePermissions`, `ProcessInfo`, `ResourceUsage`, `SystemInfo`, `ToolState`
  - This fixes compilation error when using ABK as a dependency

## [0.1.39] - 2025-12-10

### Changed
- **SessionStorage now uses V2 split-file format**: The main `SessionStorage::save_checkpoint()` method now creates split files:
  - `{checkpoint_id}_metadata.json` - Lightweight checkpoint metadata
  - `{checkpoint_id}_agent.json` - Agent state snapshot
  - `{checkpoint_id}_conversation.json` - Conversation state
- **Backward compatible loading**: `SessionStorage::load_checkpoint()` supports both:
  - V2 split-file format (checks for `{id}_metadata.json` first)
  - V1 single-file format (falls back to `{id}.json`)
- **Breaking Change**: New checkpoints will create split files instead of single files

### Notes
- Existing v1 checkpoints can still be loaded
- New checkpoints use the more modular v2 format
- This change affects `trustee`, `simpaticoder`, and any agent using ABK's checkpoint system

## [0.1.38] - 2025-12-10

### Added
- **V2 Checkpoint Storage Format**: New split-file checkpoint architecture for better modularity
  - `checkpoint::v2` module with new storage implementation
  - `CheckpointMetadataV2`: Lightweight, queryable metadata file
  - `AgentStateV2`: Agent state snapshot (separate file)
  - `ConversationFileV2`: Conversation events wrapper (separate file)
  - `CheckpointRefs`: References to companion files
  - `SessionStorageV2` / `ProjectStorageV2`: Split file save/load operations
  - `EventsLog`: Append-only JSONL event log (`events.jsonl`)
  - `EventEnvelope`: Serializable event wrapper with types

### Changed
- **Split Checkpoint Files**: Each checkpoint now creates multiple focused files:
  - `{NNN}_metadata.json` - Checkpoint metadata (small, queryable)
  - `{NNN}_agent.json` - Agent state snapshot
  - `{NNN}_conversation.json` - Conversation events
  - `events.jsonl` - Append-only session event log

### Notes
- V2 format is additive - existing v1 storage still works
- New `checkpoint::v2` re-exports available from `checkpoint` module
- 11 new tests for v2 module

## [0.1.37] - 2025-12-10

### Changed
- **Metadata File Naming**: Renamed metadata files for clarity and to avoid confusion
  - Project metadata: `metadata.json` → `project_metadata.json`
  - Session metadata: `metadata.json` → `session_metadata.json`
  - **Breaking Change**: Existing checkpoint directories will need migration or the agent will need to recreate sessions
  - **Impact**: Eliminates confusion between project and session metadata files

### Fixed
- Documentation updated to reflect new metadata filenames in checkpoint format docs

## [0.1.36] - 2025-12-09

### Fixed
- **Checkpoint Resume with Missing Projects**: Fixed issue where `resume` command would fail when checkpoint metadata contained project paths that no longer exist on disk
  - Added existence checks in `list_sessions`, `list_checkpoints`, and `delete_session` methods
  - Resume operations now gracefully skip projects with deleted paths instead of aborting
  - **Impact**: Resume functionality is now robust against stale project metadata

## [0.1.30] - 2024-11-25

### Removed
- **Template Configuration**: Completely removed template-related code from ABK
  - Removed `templates` field from `Configuration` struct (now `Option` field removed entirely)
  - Removed `TemplateConfig` struct and all template path methods
  - Removed `get_template_path()`, `get_task_template_path()`, `get_all_template_paths()` methods
  - **Breaking Change**: `[templates]` section no longer required in config files
  - **Rationale**: Templates are now exclusively handled by lifecycle WASM plugins (e.g., coder-lifecycle)
  - **Impact**: Agents must remove `[templates]` section from their TOML config files

### Changed
- Template configuration lookups now return `None` for backward compatibility
- `template_base` parameter in `ConfigurationLoader::new_with_bases()` is now ignored (templates handled by plugins)

## [0.1.29] - 2024-11-25

### Added
- **Smart Config Path Selection**: `run_configured_cli_from_config()` now automatically selects the appropriate config path based on the command being executed
  - `init` command: Uses project config (e.g., `config/agent.toml`) for setting up global environment
  - All other commands: Use global user config (`~/.{agent-name}/config/{agent-name}.toml`)
  - Agent name is extracted from config filename pattern (e.g., "trustee.toml" → "trustee")
  - Provides helpful error message if global config doesn't exist
  - **Impact**: Agents can now run from any directory without requiring a local config file

### Changed
- CLI runner now parses command line arguments early to determine config path strategy
- Error messages now suggest running `{agent-name} init --force` when global config is missing

### Fixed
- Config path inconsistency where `run` command incorrectly used project config instead of global config
- Agents can now properly execute commands from any directory after running `init`

## [0.1.28] - 2024-11-25

### Changed
- **WIT Interface Namespace Migration**: Migrated from `simpaticoder:*` to `abk:*` namespace
  - Updated `wit/lifecycle/lifecycle.wit`: `simpaticoder:lifecycle@0.1.0` → `abk:lifecycle@0.2.0`
  - Updated `wit/provider/provider.wit`: `simpaticoder:provider@1.0.0` → `abk:provider@0.2.0`
  - Updated all binding references: `.simpaticoder_*_adapter()` → `.abk_*_adapter()`
  - Updated type paths: `exports::simpaticoder::*` → `exports::abk::*`
  - **Breaking Change**: All WASM plugins must be rebuilt with new WIT definitions
  - **Result**: Zero "simpaticoder" references in codebase ✅

## [0.1.27] - 2024-11-25

### Fixed
- **Complete hardcoded name removal**: Removed remaining "simpaticoder" references from comments, documentation, and non-WIT code
  - Fixed lifecycle plugin path to use `ABK_AGENT_NAME` (was `~/.simpaticoder/providers/`, now `~/.{agent_name}/providers/`)
  - Fixed provider factory path to use dynamic agent name
  - Updated all module and function documentation to use "ABK" or "agent" instead of "simpaticoder"
  - Fixed test assertions and example code paths
  - Updated logging paths to use generic "agent" name
  - Fixed CLI adapter documentation
  - **Note**: WIT binding references remain unchanged (autogenerated code from WIT interface definitions)

### Changed
- All comments and documentation now use generic terminology
- Test fixtures use "agent" or "NO_AGENT_NAME" instead of "simpaticoder"
- Provider and lifecycle paths dynamically resolve from `ABK_AGENT_NAME` environment variable

## [0.1.26] - 2024-11-25

### Fixed
- **Agent-Agnostic Refactoring**: Removed all hardcoded "simpaticoder" references
  - Changed all fallback values from "simpaticoder" to "NO_AGENT_NAME"
  - Fixed hardcoded config path in agent initialization (`.simpaticoder/config/simpaticoder.toml` → dynamic `.{agent_name}/config/{agent_name}.toml`)
  - Updated default config values (agent name, binary name, config path)
  - Renamed `from_simpaticoder_config()` to `from_agent_config()`
  - Made error messages use dynamic agent name from `ABK_AGENT_NAME`
  - Added "projects" and "temp" to init command directory creation
  - Fixed runtime directory auto-creation (now works when directories are deleted)
  - Updated documentation to use `{agent_name}` placeholder

### Changed
- Checkpoint system now properly initializes when `ABK_AGENT_NAME` is set
- SessionManager initialization respects agent-specific config paths
- All paths and error messages dynamically use agent name from environment

## [0.1.24] - 2025-11-06

### Added
- **CLI Convenience Function**: One-liner CLI execution for simple applications
  - `run_configured_cli_from_config()` function for config-file-based CLI execution
  - `DefaultCommandContext` struct providing standard `CommandContext` implementation
  - Enables "10-line agent" goal with single function call
  - Maintains full `CommandContext` trait for advanced customization
  - Two-tier API: convenience function for 80% of use cases, full trait for 20%

### Changed
- Updated CLI module exports to include new convenience functions

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
