# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.10] - 2026-03-14

### Changed
- **BREAKING**: Removed `log_file` from `LoggingConfig` — use `log_dir` instead
- Logger always writes timestamped files to `log_dir` (defaults to `/tmp/{ABK_AGENT_NAME}/`)
- Standalone `tee_*` functions now use a global `Logger` instance (via `init_global_logger`) instead of a separate `CACHED_LOG_PATH`
- Added `init_global_logger()` and `current_log_path()` to `abk::observability` for consolidated logging
- Replaced raw `eprint!`/`print!` in extension provider with `tee_eprintln`/`tee_print` for reasoning content logging

### Fixed
- Fixed dual log file issue where agent logger and standalone `tee_*` functions wrote to separate files
- Fixed AI reasoning/thinking content not appearing in log files

## [0.5.9] - 2026-03-13

### Added
- Added standalone `tee_print`, `tee_eprint`, and `tee_eprintln` functions to `abk::observability` for components without a `Logger` reference.
- Added `run_task_from_raw_config` to `abk::cli::runner` for programmatic task execution without CLI argument parsing.

### Changed
- Replaced `eprintln!` in `checkpoint/storage.rs` with `tee_eprintln` to ensure checkpoint status reaches log files.
- Replaced `eprint!` and `print!` in `provider/wasm/mod.rs` with `tee_eprint` and `tee_print` for streaming reasoning and content redirection.

## [0.5.8] - 2026-03-13

### Changed
- Refactored Logger to tee-write all console output to log file (plain text instead of markdown)
- Added `tee_println()` and `tee_eprintln()` methods to Logger for dual console+file output
- Changed default log path to `/tmp/{agent_name}.log`
- Replaced all `println!`/`eprintln!` in orchestration with logging methods
- Added `log_tee()` to `AgentContext` trait and `tee_println()` to `OrchestrationLogger` trait
- Added optional Logger support to `AgentRuntime`

### Fixed
- `RawConfigCommandContext` now reads `logging.log_file` from config instead of falling back to default path (prevented duplicate log files)

## [0.5.7] - 2026-03-11

### Changed
- Updated `umf` dependency to 0.2.4
- Updated `cats` dependency to 0.1.11

## [0.5.6] - 2026-03-10

### Changed
- Updated `cats` dependency to 0.1.10

## [0.5.5] - 2026-02-27

### Changed
- Updated `cats` dependency to 0.1.6 (rustls TLS backend for cross-compilation)
- Changed `cats` from path dependency to crates.io

## [0.5.4] - 2026-02-19

### Added
- Registry feature for multi-source tool aggregation
- MCP tool source provider
- Native tool source provider

### Changed
- Improved provider extension system
- Enhanced checkpoint storage backend
