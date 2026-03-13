# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
