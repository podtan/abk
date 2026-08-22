# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.14.2] - 2026-08-22

### Fixed

- **Mirror-mode remote write failures are no longer swallowed** (nghr 450e00d4). A transient backend error during `save_checkpoint` previously logged a warning and an unconditional `✅ MIRRORED` line, silently leaving the remote copy incomplete — permanently, since later saves only write `remote_hwm+1..`. Every remote write now goes through a bounded retry (3 attempts, exponential backoff); Mirror saves reconcile gaps below the high-water mark before appending (message docs from the authoritative local `conversation.jsonl`, agent-state docs from `agent_state.jsonl`) so a dropped doc self-heals on the next save; the success line is only printed when every remote write succeeded (`❌ NOT MIRRORED … INCOMPLETE` otherwise); and Remote-only mode fails the save loudly on exhausted retries (the remote copy is the only durable copy). A mid-batch message failure stops the batch so sequence numbers cannot desynchronize.
- **Remote-only session listing and `--resume` work** (nghr 67163136). Two defects made Remote-only mode unusable from the CLI while Mirror mode masked them: `AbkCheckpointAccess::get_configured_storage_manager` built the manager with `with_home_dir` whenever a per-user home dir was set (hard-coding `remote_backend: None` and dropping the configured backend), and `SessionStorage::with_remote_backend` loaded the checkpoint index local-only (in-memory index always empty → `list_checkpoints()` empty → resume resolved nothing and silently started a fresh session).
- **`delete_session` / `delete_checkpoint` clean remote storage** (nghr c561e911). Deletion was local-only: the CLI reported success while every remote doc under `projects/{hash}/sessions/{sid}/` persisted, so deleted sessions re-appeared in listings via the local+remote merge and remote storage grew unboundedly. Remote deletion now lists the session/checkpoint prefix and `delete_many`s; the shared message log is never touched (surviving checkpoints address it by cursor).
- Feature gating: `create_final_checkpoint_and_get_resume_info` referenced `crate::cli::ResumeInfo` unconditionally, secretly requiring the `cli` feature for `checkpoint`; now `#[cfg(feature = "cli")]`.

### Tests

- **Checkpoint lib tests compile and pass again** (73/73, `--features "checkpoint,observability"`): 6 of the previously-documented 27 lib-test errors were inside the checkpoint module's own test code (missing `ChatMessage.reasoning` in restoration helpers; `MockAgent` missing two trait methods) and blocked the whole unit. Fixed the latent `resume_tracker` expiry assertion (policy is 7 days, not 2 hours). Lib-test baseline: 27 → 21.
- NEW `tests/cleanup_retention.rs` (3): whole-directory `delete_session` with no orphans; retention preserves surviving sessions' cursors; size tracks data volume.
- NEW `tests/remote_resilience.rs` (6, deterministic flaky-backend injection): transient failures retried, gaps below the cursor reconciled (message + state backfill), Mirror stays local-authoritative when retries exhaust, Remote-only fails the save, remote deletion leaves no orphans, `delete_checkpoint` keeps shared-log docs.

### Docs

- Checkpoint module header documents cleanup semantics (whole-dir session delete; per-checkpoint delete never truncates shared logs) and the legacy-migration path (`trustee sessions migrate [--prune]`).

## [0.14.1] - 2026-08-22

### Fixed
- **fix(checkpoint): lineage identity now includes the tool_calls payload — tool-args-only forks are no longer classified linear** — `messages_same_lineage` (the shared helper behind BOTH the local and remote lineage checks in `save_checkpoint`) compared only `role`/`content`/`tool_call_id`/`name`, omitting `ChatMessage.tool_calls` entirely. A fork whose ONLY divergence is the tool-call `arguments` (same role/content/tool_call_id/name) was therefore classified LINEAR — the same silent-corruption family fixed in 0.13.1 (fork loses branch) and 0.13.2 (length-only heuristic): the tie (`total == hwm`) reloaded the mainline prefix instead of the branch, and the outgrow (`total > hwm`) appended the branch over the mainline's own sequence numbers. The new private `tool_calls_same_lineage()` compares the full payload — `id`, `r#type`, `function.name`, `function.arguments` (serialized JSON text) — and is wired into `messages_same_lineage`. Safety argument: strictly widening identity can only flip linear → fork (the conservative full-snapshot path with `cursor_seq = 0`), never fork → linear, so no existing correct behavior can regress. New test `tool_args_only_divergence_is_treated_as_fork` in `tests/divergent_resume.rs` (verified FAILING pre-fix: "args-only diverged branch at the mainline length must be a snapshot (tie)"; passing post-fix). It deliberately keeps the branch's first `hwm` messages content-identical to the mainline (only tool-call `arguments` differ) so the pre-existing content check cannot mask the gap.

## [0.14.0] - 2026-08-21

### Removed
- **refactor(checkpoint): delete the dead `v2` split-file module** — `src/checkpoint/v2/` (`storage_v2.rs` 599 LOC, `schemas.rs` 468, `events_log.rs` 373, `mod.rs` 22 — ~1.4k LOC total) and its re-exports (`SessionStorageV2`, `ProjectStorageV2`, `EventsLog`, `EventEnvelope`, `ConversationFileV2`, `AgentStateV2`, `CheckpointMetadataV2`, `CheckpointRefs`, `CheckpointsIndex`, `SessionMetadataV2`, `SessionStatusV2`, `WorkflowStepV2`, `EventType`, `CHECKPOINT_VERSION_V2`) are removed. The module was never wired into the production write path — its append-only ideas were already harvested into the live V1 code: `ConversationLog` (`0.13.0`), `AgentStateLog` (0.13.0), and the forked-branch snapshots (0.13.1/0.13.2). No runtime behavior changes; only the unused module and its public names disappear. Anyone who imported `abk::checkpoint::v2::…` directly should switch to the V1 append-only types (`ConversationLog`, `AgentStateLog`, `SessionStorage`, `ProjectStorage`).

### Fixed
- **docs(checkpoint): `checkpoint` module docs and `save_checkpoint` no longer advertise the removed/never-shipped formats** — The `checkpoint` module header described a "V2 Storage Format" (`{NNN}_metadata.json` / `{NNN}_agent.json` / `events.jsonl`) that no such production code ever wrote. It now documents the **actual** append-only layout: local `session_metadata.json` (write-once) + `conversation.jsonl` (shared mainline) + `agent_state.jsonl` + `checkpoints/checkpoints.json` (cursor index) + fork-only `{checkpoint_id}_conversation.json` snapshots; remote `messages/{seq:05}.json` / `state/{seq:05}.json` / `checkpoints/checkpoints.json` / `metadata.json` path-keyed in a single collection. `SessionStorage::save_checkpoint`'s doc comment (previously advertising `session_agent.json` + per-checkpoint `{checkpoint_id}_conversation.json`, which stopped being written in 0.13.0) now describes the append-only layout and fork semantics. `conversation_log.rs`'s header note no longer references the deleted `v2::events_log::EventsLog`. README: fixed duplicated title block, stale `0.1.24` version examples, and the checkpoint section now shows the real format.

## [0.13.2] - 2026-08-20

### Fixed
- **fix(checkpoint): lineage check — forks that outgrow the mainline no longer pollute the shared log** — The 0.13.1 fork detection was a **length-only** heuristic (`is_fork = total < hwm`), which was wrong in two ways: (A) a fork resumed from an earlier checkpoint keeps appending on its branch and can **outgrow** the mainline (`total >= hwm`) — the length check then classified it as linear and appended the branch's messages at `seq = hwm+1`, **overwriting the mainline's own messages** at those sequence numbers (silent corruption of the shared `conversation.jsonl`); (B) the **tie** case `total == hwm` defaulted to linear, so a diverged branch of exactly the mainline length indexed `cursor_seq = hwm` and reloaded the **mainline prefix** instead of the branch. The fix compares **lineage**: a checkpoint is linear only if `total >= hwm` **AND** its first `hwm` messages are identical to the mainline's first `hwm` messages (new `messages_same_lineage()` helper — compares `role` + `content` + `tool_call_id` + `name`; volatile fields `timestamp`/`token_count`/`reasoning` are ignored). The mainline prefix is read from the local `conversation.jsonl` (`ConversationLog::read_up_to(hwm)`) in Local/Mirror mode, or from the remote per-message docs (range read `seq = 1..=hwm`) in Remote-only mode; any gap or read failure is treated as a fork (conservative — never corrupts the mainline). Fork semantics from 0.13.1 are unchanged: full `{NNN}_conversation.json` snapshot with `cursor_seq = 0` (remote: legacy per-checkpoint key). Linear checkpoints (`total == hwm` with matching content) still append nothing and keep their cursor. Known limitation: a long-lived fork re-saves the **full** branch snapshot on every checkpoint (O(N²) within the branch); per-branch logs are a future enhancement. New tests in `tests/divergent_resume.rs`: fork-outgrows-mainline (was failing), exact-length tie (was failing), and a same-content equal-length control (must stay linear).

## [0.13.1] - 2026-08-20

### Fixed
- **fix(checkpoint): divergent-resume fork silently corrupted the shared conversation log** — Resuming a **non-latest** checkpoint and continuing from it forks the conversation: the new branch's messages do **not** extend the session's append-only `conversation.jsonl` (they diverged from the mainline). `save_checkpoint` only appended when `total > hwm`, so a forked checkpoint (`total < hwm`) indexed a cursor pointing at the **mainline** messages and the branch was silently lost on reload — a fresh `load_checkpoint` returned the first `total` mainline lines instead of the diverged branch, with no error. The fix detects a fork (`total < hwm`) and, instead of appending the branch to the shared log (which would corrupt the mainline at the wrong sequence numbers), persists it as a full `{NNN}_conversation.json` snapshot with `cursor_seq = 0` — exactly the layout the existing legacy fallback reader loads. The shared `conversation.jsonl` is never rewritten or truncated, and linear sessions (`total >= hwm`) are unchanged. Applied to **both** the local path and the remote (DocumentDB) path: the remote fork writes the full snapshot to the legacy per-checkpoint key (`{prefix}/checkpoints/{id}_conversation.json`) instead of inserting the branch as per-message docs. `message_count` now records the checkpoint's own message count (the branch length) even when `cursor_seq = 0`. New E2E test `tests/divergent_resume.rs`: a linear-history baseline (green), the divergent-resume repro (was failing, proving the bug), and a shared-log integrity check (the fork must not append to the mainline).

## [0.13.0] - 2026-08-17

### Added
- **feat(checkpoint): append-only `conversation.jsonl` + cursor-based checkpoints** — Replaces the per-checkpoint `{NNN}_conversation.json` full-history snapshots (O(N²) storage) with a single append-only JSON Lines log per session: one `ChatMessage` per line, written with `O_APPEND` + `fsync` (crash-safe, never rewritten). New `ConversationLog` handler (`src/checkpoint/conversation_log.rs`) harvested from the dead `v2::EventsLog` design. `save_checkpoint` diffs against the log's high-water mark and appends only NEW messages. `CheckpointMetadata` gains `cursor_seq` + `message_count` (serde default 0 → old indexes still deserialize). Read path rebuilds the conversation from the log up to the cursor. Remote (DocumentDB) mirrors the layout 1:1: per-message docs `projects/{hash}/sessions/{sid}/messages/{seq:05d}.json` as pure inserts, resume = range read `seq <= cursor`.
- **feat(checkpoint): append-only `agent_state.jsonl` + session-constant metadata** — Replaces the per-checkpoint `{NNN}_agent.json` files (O(N) files, ~95% redundant: iteration/step/mode were also in the `checkpoints.json` index, and task/config/working-dir never change within a session). Mutable agent state now lives in a single append-only `agent_state.jsonl` log: one compact line per checkpoint (`{seq, checkpoint_id, iteration, step, mode, ts}`), same `O_APPEND` + `fsync` + torn-line recovery as the conversation log, with its **own** sequence space (never touches the conversation cursor or the remote `messages/` keys). This is the designated home for future mutable state — a new mutable field = a new field on the line, no new file, no migration. Session-**constant** fields (`task_description`, `configuration`, `working_directory`, `max_iterations`) move to `SessionMetadata` (written once at session start — a write-once file has no staleness), recorded via a new `SessionConstants` passed through `create_session_with_description`. Remote mirrors the log 1:1: `projects/{hash}/sessions/{sid}/state/{seq:05d}.json` as pure inserts.

### Fixed
- **fix(checkpoint): checkpoint-ID collision on legacy/mixed resume** — Resuming a legacy/mixed session (e.g. an old session resumed with a new binary) picked the "latest" checkpoint by max `created_at` and resumed numbering at that checkpoint's iteration + 1. A stale entry carrying a newer timestamp could win, so numbering restarted mid-history and the next saves **overwrote** existing checkpoints (silently orphaning the old `{NNN}_conversation.json` files). Three coordinated fixes: (1) new `max_session_iteration()` computes the **true** highest iteration across every source of truth — the `checkpoints.json` index, the append-only `agent_state.jsonl`, and any on-disk `{NNN}_*.json` / `{NNN}.json` files (via `checkpoint_iteration`) — so the next number is always fresh; (2) `latest_checkpoint_id()` now returns the highest **iteration** (the true ordering in a linear session) instead of max `created_at`; (3) `resume_from_checkpoint` resumes at `max(true_max, resumed_iteration) + 1` instead of `resumed_iteration + 1`, so numbering continues past the session's real maximum regardless of which checkpoint is resumed.
- **fix(checkpoint): mixed legacy/new session loading** — A session that has legacy checkpoints (cursor_seq=0, the serde default) plus a `conversation.jsonl` created after an upgrade+resume now loads old checkpoints from their legacy `{id}_conversation.json` files instead of reading the jsonl up to cursor 0 (which would yield no messages). Same guard on the remote path.

### Kept-Compat
- Legacy sessions (pre-0.13.0 layouts: `{NNN}_conversation.json`, `session_agent.json`, per-checkpoint `_agent.json`/`_metadata.json`, V1 single-file) still load unchanged via fallback readers: agent-state resolution is `agent_state.jsonl` → `{id}_agent.json` → `session_agent.json`. New sessions no longer write `{NNN}_agent.json` or `session_agent.json`. Deleting a checkpoint never truncates the shared `conversation.jsonl`/`agent_state.jsonl` (later checkpoints' cursors depend on earlier entries).

## [0.12.12] - 2026-08-09

### Added
- **feat(cli): `generate_handoff_briefing()`** — Generates a session handoff briefing with a SINGLE direct LLM call. Loads conversation history from the last checkpoint (`SessionStorage::load_checkpoint`), then makes one `provider.generate()` call with the MAIN model (not `[llm.utility]`), briefing prompt as system + conversation transcript as context. No tools, no workflow loop, no checkpointing. This replaces the full `run_task_from_raw_config` workflow approach which could loop indefinitely and brick sessions.

## [0.12.11] - 2026-08-09

### Fixed
- **fix(cli): clone messages for retry in title generation** — First `generate()` call moved `messages`, causing compilation failure when retry logic tried to reuse it. Now clones for all calls.

## [0.12.10] - 2026-08-09

### Fixed
- **fix(cli): thinking model title generation with insufficient max_tokens** — GLM-4.7-Flash and similar thinking models consume 500-600+ reasoning tokens before producing a title. With `max_tokens=300` (or the old default of 100), the response was truncated mid-reasoning with empty `content`. Now: (1) default `max_tokens` raised from 100 to 1000, (2) added `extract_title_from_response()` helper with improved extraction strategies including GLM "Idea N:" brainstorming patterns, (3) automatic retry with doubled tokens then 3000 tokens if extraction fails.
- **fix(config): default `UtilityLlmConfig.max_tokens` raised from 100 to 1000** — Old default was far too low for thinking models.

## [0.12.9] - 2026-08-09

### Fixed
- **fix(checkpoint): preserve description on session resume** — `create_session_with_description` was always creating brand-new metadata, overwriting the existing description with null when resuming. Now reads existing `session_metadata.json` and preserves description/tags.
- **fix(checkpoint): fallback to task_description when identity.name is None** — Web sessions pass `SessionIdentity { name: None }`, causing description to be null. Now falls back to truncated task description.
- **fix(checkpoint): removed Solution A from create_checkpoint** — Was causing race condition where `save_checkpoint` overwrote Solution A's description on subsequent checkpoints.
- **fix(cli): removed checkpoint_count > 5 guard** — Was blocking legitimate title generation on fresh sessions with many tool calls.

## [0.12.8] - 2026-08-09

### Fixed
- **fix(cli): should_generate_title now checks checkpoint count** — Sessions with >5 checkpoints are treated as resumed and skip title generation, regardless of `resume_info` state. This prevents title corruption when trustee web restarts on a session with existing history.

## [0.12.7] - 2026-08-08

### Added
- **feat(cli): `should_generate_title()` guard function** — Checks existing session metadata to determine if LLM title generation is needed. Returns `false` if the description was already LLM-generated or user-set, preventing title overwrite on subsequent commands in the same session.

### Fixed
- **fix(cli): improved reasoning title extraction** — Better extraction from thinking model reasoning content. Now searches for quoted strings first, then falls back to clean lines with improved filtering of analysis/meta-language patterns.

## [0.12.6] - 2026-08-08

### Fixed
- **fix(cli): `persist_session_title()` now supports remote backends** — The standalone title persistence function was local-filesystem-only. Now accepts `config_toml`, parses `[checkpointing.storage_backend]`, and when a remote backend (DocumentDB/MongoDB) is configured, constructs a backend and writes the updated metadata to remote storage in addition to local. Works with `Mirror` and `Remote` storage modes.

## [0.12.5] - 2026-08-08

### Fixed
- **fix(cli): title generation works with thinking models** — GLM-5.2 and other thinking models return content in `reasoning_content` with empty `content`. Now falls back to reasoning, extracts last meaningful line. Increased default max_tokens from 100 to 500 to accommodate thinking overhead.

### Added
- **feat(cli): `persist_session_title()` standalone function** — Persists a session title directly to `session_metadata.json` on disk without requiring an active `SessionManager`.

## [0.12.4] - 2026-08-08

### Added
- **feat(cli): `persist_session_title()` standalone function** — Persists a session title directly to `session_metadata.json` on disk without requiring an active `SessionManager`. Constructs the path from `RunContext` (home_dir/project_id/session_id), reads existing metadata, updates `description`, and writes back atomically. Used by trustee-core after LLM title generation (Solution B) to persist titles when the ABK SessionManager has already been dropped.

## [0.12.3] - 2026-08-03

### Added
- **feat(config): `[llm.utility]` config section** — New optional `UtilityLlmConfig` struct under `[llm.utility]` for configuring lightweight background LLM calls (session title generation, summaries). Fields: `model: Option<String>`, `max_tokens: u32` (default 100), `temperature: f32` (default 0.3). Falls back to main provider when absent.
- **feat(cli): `generate_session_title()` public function** — Lightweight LLM call that generates a concise (≤50 chars) session title from the user's command. Uses `ProviderFactory` to create a provider, makes a single non-streaming `generate()` call. Reads `[llm.utility]` settings when available.
- **feat(checkpoint): `update_session_description()` on SessionStorage** — Persists a new description/title to `session_metadata.json` on disk (and remote backend if configured). Used by SessionManager for Solution A (persist title after first checkpoint) and Solution B (LLM-generated titles).
- **feat(checkpoint): `update_session_description()` + `current_session_id()` on SessionManager** — Public API for updating session titles post-creation and querying the active session ID.
- **feat(checkpoint): `checkpoint_count()` getter on SessionStorage** — Exposes metadata checkpoint count for external callers without requiring `synchronize_metadata()`.

### Fixed
- **fix(checkpoint): persist session description after first checkpoint** — Session descriptions were only written at session creation time and never updated. Now after the first checkpoint, the task description is persisted to `session_metadata.json`, ensuring the title is available immediately rather than only at creation time.

## [0.8.3] - 2026-07-23

### Added
- **feat(resume): cross-project session resume** — `ResumeInfo` now carries an optional `project_path: Option<PathBuf>` field. When set, `execute_run()` uses this path instead of the process CWD for checkpoint lookup and tool working directory, enabling sessions to be resumed from projects other than the current working directory. This fixes a long-standing limitation where resuming a session created in project B from project A would silently fail (checkpoint hash mismatch). Backward compatible: `None` falls back to CWD (legacy behaviour), and `#[serde(default)]` ensures old serialized data deserializes correctly.

## [0.8.2] - 2026-07-22

### Fixed
- **fix(run): create final checkpoint and sync metadata on cancel/error paths** — When a workflow was cancelled (ESC) or errored, `execute_run` skipped `stop_session()` entirely, meaning no final checkpoint was created and `session_metadata.json` went stale. Now on the `Err` path: (1) if cancelled, creates a final checkpoint to capture tool results that completed before ESC, (2) always calls `finalize_checkpoint_session()` to sync `checkpoint_count` with reality. New pub methods `Agent::create_checkpoint_now()` and `Agent::is_checkpointing_enabled()` added.
- **fix(storage): heal stale checkpoint_count on every read** — `load_sessions_from_disk()` previously only healed `checkpoint_count` when it was exactly 0. Non-zero stale counts (from cancelled sessions where `finalize` was skipped) were never corrected. Now always cross-checks against `checkpoints.json` index and persists the healed value.

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
