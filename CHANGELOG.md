# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.1] - 2026-07-22

### Fixed
- **fix(orchestration): checkpoint tool results before cancellation check** — In `handle_tool_calls()` (`agent_orchestration.rs`), the cancel token check was positioned before tool results were added to conversation history. When ESC was pressed during an in-flight tool call (e.g. file write), the tool completed on disk but its result was never recorded in the session checkpoint, causing a silent desync between on-disk state and session context. The cancel check now runs after tool results are added to `chat_formatter`, ensuring the checkpoint captures the completed iteration while cancellation still takes effect immediately after the tool batch.

## [0.8.0] - 2026-07-17

### Fixed
- **fix(tui): gate all raw `println!`/`eprintln!` with `is_tui_mode()` checks** — `AgentRuntime::log_info()` and `AgentRuntime::tee_println()` in `orchestration/runtime.rs` had bare `println!` in their `else` branches (when `self.logger` is `None`). `CleanupManager` in `checkpoint/cleanup.rs` had ~15 `println!` calls gated only by `self.verbose`. These bypassed the TUI mode flag and wrote directly to stdout while ratatui held the terminal in raw/alternate-screen mode, causing orphan characters and jagged border boxes during streaming output. All occurrences now route through `tee_println()` or check `is_tui_mode()`.

## [0.7.8] - 2026-07-17

### Fixed
- **fix(provider): tool results sent as empty content to LLM** — `messages_to_openai()` in the native OpenAI provider only checked `MessageContent::Text` for tool-role messages, but `ChatMLAdapter` wraps tool results as `MessageContent::Blocks(vec![ContentBlock::ToolResult{...}])`. The `Tool` role handler now also extracts content from `ToolResult` and `Text` blocks, fixing the critical bug where all tool outputs (bash, read, write) were silently dropped to empty strings.

## [0.7.7] - 2026-07-17

### Changed
- **feat(features): make WASM fully optional** — The `agent` feature no longer pulls in `extension` or `provider-wasm`. A new convenience feature `wasm` enables both `provider-wasm` and `extension` in one step. Consumers opt into WASM with `features = ["agent", "wasm"]` or `--features wasm`.
- **refactor(lifecycle): gate `WasmLifecycle` behind `extension` feature** — `SimpleLifecycle` (pure Rust) is always available. `WasmLifecycle`, `find_lifecycle_plugin()`, and `create_standalone_instance()` require the `extension` feature. `find_lifecycle_plugin_with_config()` falls back to `SimpleLifecycle` when the `extension` feature is off.
- **fix(cli): ungate `ExtensionError` variant** — `CliError::ExtensionError` is now always available (was previously behind `#[cfg(feature = "extension")]`), so extension CLI commands compile without the extension feature.

## [0.7.6] - 2026-07-17

### Added
- **feat(provider): add native Rust OpenAI provider** — `OpenAIProvider` implements `LlmProvider` using pure `reqwest` (no wasmtime dependency). Handles non-streaming `generate()`, streaming `generate_stream()` with SSE parsing, tool calling, and reasoning content support for thinking models.
- **feat(provider): split `provider` and `provider-wasm` features** — The `provider` feature no longer requires `wasmtime`/`wasmtime-wasi`. The new `provider-wasm` feature adds wasmtime for WASM-based extensions. This allows building agents with native providers only, significantly reducing compile times and binary size.

### Changed
- **refactor(factory): dispatch `LLM_PROVIDER=openai-unofficial` to native `OpenAIProvider`** — Default (unset) also routes to native. `LLM_PROVIDER=openai-unofficial-wasm` or any other value routes to the WASM `ExtensionProvider`.
- **refactor(provider): gate `wasm` module behind `provider-wasm` feature** — The `extension` module is gated behind the `extension` feature.
- **refactor(agent): use `provider-wasm` instead of direct `wasmtime` dependency** — The `agent` feature now transitively enables `provider-wasm` instead of listing `wasmtime`/`wasmtime-wasi` directly.

## [0.7.5] - 2026-07-08

### Changed
- **perf(checkpoint): eliminate per-iteration `_agent.json` and `_metadata.json` duplicates** —
  `SessionStorage::save_checkpoint()` now writes `session_agent.json` ONCE per session (first
  checkpoint only) instead of duplicating the 8KB agent state across N checkpoint files.
  Per-checkpoint `_metadata.json` files are no longer written; all metadata lives in the
  existing `checkpoints.json` index. Only `{id}_conversation.json` is written per checkpoint
  (legitimately unique). Reduces a 99-iteration session from 299 files to 101 files,
  eliminating ~1.2MB of redundant disk/DocumentDB writes.
- **Backward compatible**: Old sessions with per-checkpoint `_agent.json` / `_metadata.json`
  files remain fully readable via fallback logic in `try_load_from_local()` /
  `try_load_from_remote()`. Resume API is unchanged.
- Applies to both `SessionStorage` (V1, active) and `SessionStorageV2` (V2).
- Works with all storage modes: Local, Remote (DocumentDB), and Mirror.

## [0.7.4] - 2026-07-05

### Fixed
- **fix(resume): `resume -i` hang on Windows** — `read_line` now performs the blocking stdin read in a dedicated OS thread (`std::thread::spawn`) to avoid tokio/IOCP conflict on Windows where console input notifications may not reach the blocking read under the async runtime (issue #2dd0cbb2).
- **fix(observability): add explicit `stdout().flush()` to `tee_println`** — Both `Logger::tee_println` method and standalone `tee_println` function now flush stdout after printing, matching the existing behavior of `tee_print`. Fixes delayed/garbled output on Windows ConPTY/Windows Terminal.

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
