# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.26] - 2026-04-08

### Added
- `on_checkpoint` channel sender is now restored after each send so it survives across multiple workflow iterations
- Incremental `resume_info` is sent after each workflow iteration for real-time session continuity
- `tool_tokens` breakdown in context size reporting for better visibility into tool usage costs

### Fixed
- `on_checkpoint` sender is no longer consumed after the first iteration — checkpoint updates now persist across the full workflow lifecycle
- `create_final_checkpoint_and_get_resume_info` now reads iteration from `AgentContext` instead of stale `SessionManager` field
- Removed stale `submit` tool reference from simple lifecycle system message

### Changed
- `CancellationToken` is now bridged to `cats` `cancel_signal` for instant ESC kill in TUI
- CancellationToken is propagated through the workflow for cooperative cancellation
- Updated `umf` dependency to 0.2.5 (published crate)
- Updated `cats` dependency to 0.1.15 (published crate)
- Fixed all compiler warnings across the codebase

## [0.5.25] - 2026-04-07

### Added
- `CancellationToken` support in orchestration workflow loops (`run_workflow`, `run_workflow_streaming`) for cooperative cancellation. When the token is cancelled, the workflow stops cleanly via `stop_session()` with proper checkpoint finalization.
- `cancel_token` field on `RunOptions` to propagate cancellation tokens from callers through to the workflow loop.
- `cancel_token` parameter on `run_task_from_raw_config()` enabling TUI/pass-through callers to abort workflows mid-execution.
- `tokio-util` dependency (optional, gated behind `orchestration` feature) for `CancellationToken`.

## [0.5.25] - 2026-04-07

### Added
- `CancellationToken` support in workflow orchestration — `run_workflow()` and `run_workflow_streaming()` accept `cancel_token: Option<CancellationToken>` and check cancellation at the top of each loop iteration, enabling cooperative cancellation of long-running workflows
- `RunOptions.cancel_token` field — allows callers to pass a cancellation token through to the workflow loop
- `run_task_from_raw_config()` accepts new `cancel_token` parameter for workflow cancellation support
- `tokio-util` optional dependency, gated behind the `orchestration` feature

### Changed
- `run_workflow()` and `run_workflow_streaming()` signatures now include `cancel_token: Option<CancellationToken>` parameter (backward compatible — pass `None` for existing callers)

## [0.5.24] - 2026-04-04

### Added
- `RunOptions.on_checkpoint`: optional channel to send incremental `ResumeInfo` after each checkpoint, enabling TUI session continuity when ESC cancels mid-workflow

### Changed
- `run_task_from_raw_config()` accepts new `resume_info_tx` parameter for incremental resume info forwarding

## [0.5.23] - 2026-03-24

### Fixed
- `SessionManager::create_final_checkpoint_and_get_resume_info()` now reads the iteration counter from the `AgentContext` (`context.get_current_iteration()`) instead of from the stale `SessionManager.current_iteration` field. This fixes TUI session continuity where follow-up tasks would overwrite checkpoint data from iteration 1 instead of continuing from the correct iteration number.

## [0.5.22] - 2026-03-24

### Changed
- Updated `cats` dependency to 0.1.14

## [0.5.21] - 2026-03-24

### Changed
- Updated `cats` dependency to 0.1.13

## [0.5.20] - 2026-03-23

### Added
- `context_tokens` field to `OutputEvent::ApiCallStarted` — CLI and TUI now display context size in API call info lines (e.g., `Context=43036`)
- `OutputEvent::ToolCompleted` is now emitted by the orchestration layer in `handle_tool_calls()` — was previously defined but never wired up
- `description: Option<String>` field to `OutputEvent::ToolCompleted` — carries tool description (e.g., bash command description) for TUI display
- `description: Option<String>` field to `ToolExecutionResult` in orchestration, agent, and tools modules — description extracted from tool call arguments

### Changed
- `Display` impl for `ApiCallStarted` now shows `Context={}` in the format string
- `Display` impl for `ToolCompleted` now shows description when present (`🔧 bash — Build the project`)

## [0.5.19] - 2026-03-22

### Added
- `ResumeInfo` and `TaskResult` types in `abk::cli` for in-memory session continuity
- `SessionStorage::session_id()` and `SessionStorage::latest_checkpoint_id()` accessor methods
- `SessionManager::create_final_checkpoint_and_get_resume_info()` — creates a final checkpoint and returns `ResumeInfo` for session resumption
- `Agent::create_final_checkpoint_and_get_resume_info()` — public API for creating final checkpoint and extracting resume info
- `RunOptions::resume_info` field — allows passing `ResumeInfo` to `execute_run()` for session continuity
- `run_task_from_raw_config()` now accepts optional `ResumeInfo` parameter and returns `TaskResult` instead of `()`

## [0.5.18] - 2026-03-22

### Added
- `OutputEvent::ReasoningChunk` variant for streaming reasoning/thinking tokens from models (GLM, Qwen, etc.)
- Reasoning-to-content transition detection in streaming loop — emits a newline separator when reasoning ends and content begins

### Changed
- `StdoutSink` prints reasoning chunks to stderr in dim grey (`\x1b[90m`) to visually distinguish them from content
- Streaming loop in `context_orch.rs` now emits `ReasoningChunk` events alongside `StreamingChunk` events

## [0.5.17] - 2026-03-21

### Fixed
- Fixed duplicate LLM response output: `handle_content_response()` now accepts a `was_streamed` flag and skips emitting `LlmResponse` event when the text was already streamed chunk-by-chunk via `StreamingChunk` events — eliminates duplicate display in both CLI and TUI
- Fixed CLI streaming output: `StdoutSink` now uses `print!` (no newline) for `StreamingChunk` events instead of `println!` — streaming text flows naturally instead of each token appearing on its own line

### Added
- `Logger::append_to_log()` is now public for direct log file writes from orchestration code
- `get_global_logger_opt()` returns `Option<&Logger>` without auto-initializing a default logger

## [0.5.16] - 2026-03-21

### Added
- `StreamingChunk` output events are now emitted to the `OutputSink` during streaming LLM responses — enables real-time streaming display in TUI and other sink consumers
- `LlmResponse` output events are now emitted in `handle_content_response()` — TUI receives full LLM response text when the model finishes generating

### Changed
- `handle_content_response()` now writes to log file via `tee_println` instead of `log_tee` to ensure consistent logging behavior

## [0.5.15] - 2026-03-15

### Fixed
- Converted all raw `println!`/`eprintln!` calls in agent, checkpoint, and provider modules to use TUI-aware `tee_*` functions — fixes TUI display corruption from direct stdout/stderr writes
- Added `tee_println` free function and exported `is_tui_mode` for use by other modules
- Fixed text wrapping and scroll behavior in TUI output (consumer-side)

## [0.5.14] - 2026-03-15

### Added
- TUI mode flag (`set_tui_mode`) — when enabled, all console output (stdout/stderr) from `tee_*` functions and `Logger` methods is suppressed; log file output is unaffected
- Exported `set_tui_mode` from `abk::observability` for use by TUI consumers

### Fixed
- Fixed TUI rendering corruption caused by process-global `dup2` stdout/stderr redirect — TUI mode suppresses console output at the source instead

## [0.5.13] - 2026-03-15

### Fixed
- Strip ANSI escape codes from log file output — `tee_print`, `tee_eprint`, `tee_eprintln` now write clean text to log while preserving ANSI colors on terminal
- Reasoning tokens no longer pollute log files with `\x1b[90m...\x1b[0m` escape sequences
- Fix reasoning appearing line-by-line: changed `tee_eprintln` to `tee_eprint` for streaming reasoning tokens (no forced newline after each token)

## [0.5.12] - 2026-03-14

### Fixed
- Fixed silent streaming failure: `agent_orchestration` now logs actual error before wrapping with "Streaming workflow failed"
- Added retry logic for retryable streaming errors (finish_reason, network_error, stream errors) in `agent_orchestration`
- Extended streaming request timeout from 120s to 600s (LLM responses with reasoning can take minutes)
- CLI now shows full error chain with `{:#}` format for better debugging
- Added `tee_eprintln` for byte stream errors so they appear in both stderr and log file

## [0.5.11] - 2026-03-14

### Fixed
- Fixed premature session termination on `finish_reason: "network_error"` from LLM SSE streams
- Extension provider now logs stream errors with `tee_eprintln` (visible in both stderr and log file)
- Streaming workflow retries on retryable errors (network_error, stream errors) before falling back to non-streaming

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
