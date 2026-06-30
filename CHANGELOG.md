# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.3] - 2026-06-30

### Fixed
- **fix(agent): keep McpToolLoader even when all MCP servers fail** — Previously, when all configured MCP servers failed to connect, `loader.has_tools()` returned `false` and the entire loader (including `server_statuses` with per-server error details) was discarded. This caused the TUI MCP status panel to permanently show `0/0 (none)` with no indication that servers were attempted. The fix always retains the loader on `Ok`, so `emit_mcp_server_statuses()` can fire for all servers, showing failed servers with their error messages (e.g., `0/2` with `✗ pdt — 401 Unauthorized`).

## [0.7.1] - 2026-06-17

### Added
- feat: add `OutputEvent::McpServerStatus` for MCP server status visibility in TUI
- feat: add MCP server status panel in TUI showing per-server connection health

## [0.7.0] - 2026-06-10

### Changed
- release(abk): replace all raw `eprintln!` with TUI-safe `tee_eprintln`

## [0.6.3] - 2026-06-08

### Fixed
- fix: MCP command gating for non-registry-mcp builds

## [0.6.2] - 2026-06-05

### Changed
- deps: update cats to 0.1.28 (interactive detector removed)

## [0.6.1] - 2026-06-03

### Added
- feat(config): add interactive MCP auth support with `InteractiveTokenProvider`

## [0.6.0] - 2026-05-28

### Changed
- refactor: major restructure of config, observability, and checkpoint modules

## [0.5.x] - 2026-01 to 2026-05

### Fixed
- Various bug fixes and dependency updates for cats, MCP token handling, and logger permissions.
