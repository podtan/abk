# Issue: Declarative CLI Extraction — Divergence between simpaticoder and ABK

Status: documented (issue report only — no solution proposed here)

## Summary

The project goal was to extract the CLI implementation into the ABK (Agent Builder Kit) crate so that simpaticoder's CLI usage would be a single line of Rust code, e.g.:

    DeclarativeCli::from_file("config/simpaticoder-cli.toml")?.execute().await

and ABK would implement runtime behavior driven entirely by configuration files (TOML/JSON/YAML). The broader ambition is to find the right abstractions so that an application could construct and run a fully working agent in ~10 lines of Rust. To reach that goal several responsibilities have already been extracted from simpaticoder into separate crates: abk (Agent Builder Kit), cats, coder-lifecycle, tanbal-provider, umf; each is a separate Rust crate. ABK itself exposes multiple features: agent, checkpoint, cli, config, observability, orchestration, provider.

The intent was: move CLI wiring and runtime into ABK, keep simpaticoder's CLI wiring minimal (one small glue line if any), and keep application-specific agent creation logic in the application only when unavoidable. Instead, the current state shows both:

- ABK implementing a declarative CLI framework that exposes registration APIs and intentional stubs for runtime behavior, and
- Simpaticoder reintroducing substantial application-specific Rust handler code (command handlers, agent creation and init logic) in `src/lib.rs` and under `src/cli`.

This file documents the divergence, the commits where the divergence occurred, concrete evidence from source files, and the observed impacts and risks. This document records the problem and evidence for triage — it does not prescribe the fix.

## Desired goal (for context)

- CLI behavior configured entirely from `config/simpaticoder-cli.toml` (TOML/JSON/YAML).
- ABK implements the declarative CLI runtime, adapters, and any non-application-specific factories required to run the common agent lifecycle.
- Applications call ABK with one small integration point (ideally one line) to start the CLI/runtime.
- Simpaticoder contains no substantial CLI command handlers (at most a tiny integration glue), and ABK does not contain application-specific agent creation code.
- By finding the right abstraction we should be able to create and run an agent in ~10 lines of Rust code.

## Where we are now (high-level)

- Branches in use:
  - simpaticoder: `feature/declarative-cli-framework`
  - tmp/abk: `feature/declarative-cli-framework`

- Key files observed:
  - simpaticoder: `src/lib.rs` (contains `run_cli()` that registers `agent::run` and `init::global` special handlers with closures implementing agent creation and init logic).
  - simpaticoder: `config/simpaticoder-cli.toml` (configuration exists and describes commands/adapters/agent settings).
  - tmp/abk: `src/cli/declarative/executor.rs` (ABK's DeclarativeCli; registration APIs and runtime stubs that explicitly instruct the application to register factories/handlers).

## Divergence point (merge base)

A common merge-base between `main` and the feature branches was used as the starting point for the following commit lists. (Internal SHA used as merge-base: ae4f8b6b79ddaf2b5d8f58af76941d213f7db2ef.)

### Commits on simpaticoder (feature/declarative-cli-framework) since divergence

From `main..feature/declarative-cli-framework` (simplified):

- f8948b5 docs: Update NEXT_STEPS.md - mark init and env paths as completed
- efdc958 feat(cli): Implement init and fix environment path defaults
- 9dfbdee docs: Add NEXT_STEPS.md with implementation roadmap
- a21adee feat(cli): Implement declarative CLI with ABK integration
- 8a948ee feat: Wire agent execution to declarative CLI
- bc3408f docs: Mark agent extraction PRD as completed
- 3bdc455 refactor: Use ABK agent runtime, delete src/agent/
- 2374e6e docs(specs): Update declarative CLI specs with completion status
- 8d52928 docs(specs): Add PRD and tasks for moving Agent to ABK
- 17f7a41 docs: mark Tasks 5-6 complete - CLI replacement successful
- fc1457e feat: replace 1,657 lines of CLI code with ONE line using declarative framework
- 5338ad0 docs: mark Tasks 1-4 complete in declarative CLI framework
- cdc4d01 fix: update PRD and tasks to ONE-LINE design (no handlers in simpaticoder)
- dbadf02 docs: add PRD and tasks for declarative CLI framework

Notes:
- The commits `a21adee` and `fc1457e` are specifically referenced as implementing/claiming the CLI extraction. However, `src/lib.rs` currently contains non-trivial handler closures (see Evidence below).
- Several documentation commits assert the one-line goal and that CLI logic moved to ABK, which is not fully reflected in runtime code.

### Commits in tmp/abk (feature/declarative-cli-framework) since divergence

From `tmp/abk` branch (simplified):

- cfa1c7f feat(cli): Allow custom special handlers without router registration
- 863f47f feat: Add agent feature with trait-based dependency injection
- 1f7d961 feat(cli): add declarative CLI framework

Notes:
- ABK contains a declarative CLI implementation and exposes registration APIs (`register_special_handler`, `register_abk_handler`) and an executor that expects the application to register or provide an agent factory.
- ABK's runtime includes stubs and explicit error messages that the application must register runtime behavior (e.g., agent construction). See Evidence.

## Evidence (source pointers & behavior)

1) Simpaticoder registers special handlers directly in `src/lib.rs` (i.e., runtime application code exists in simpaticoder)

- File: `src/lib.rs`
  - Function: `pub async fn run_cli() -> anyhow::Result<()>`
  - Calls:
    - `DeclarativeCli::from_file("config/simpaticoder-cli.toml")?`
    - `.register_special_handler("agent::run", |matches| { ... })` — large closure that:
      - Extracts CLI args (task, config, env, mode, yolo).
      - Builds paths and default locations if not provided.
      - Creates an agent via `create_agent_with_bases(...)` (Simpaticoder's factory function).
      - Starts the session and runs the workflow (streaming/traditional).
    - `.register_special_handler("init::global", |matches| { ... })` — closure that performs filesystem operations to initialize a global installation (dirs, copying config, creating symlinks, etc.).

These closures contain a large amount of application-specific runtime logic and even call `create_agent_with_bases()` — they are not "one-line" glue.

2) ABK's declarative CLI expects application integration and registers handlers

- File: `tmp/abk/src/cli/declarative/executor.rs`
  - Provides `DeclarativeCli` with methods:
    - `register_special_handler()` and `register_abk_handler()` (both accept a handler closure and store them in maps).
    - `from_file()` and `execute()` implemented.
  - Important behavior in `execute()`:
    - When routing to an ABK command, ABK will attempt to call `self.abk_handlers.get(handler_name)` and run it if found; otherwise it returns an explicit error: "ABK command execution not implemented: {}::{}".
    - For `SpecialHandler` types, ABK has a `match handler.as_str()` for built-in names like `agent_run`, `init`, `resume` and calls `self.execute_agent_run`, `self.execute_init`, `self.execute_resume` — but these methods are stubs that either return an error or explicitly state: "Agent construction requires application integration. Simpaticoder must call ABK with agent factory." (see `execute_agent_run()` and its explicit Err).
  - `execute_agent_run()` contains code to read agent config and CLI args but ultimately returns an error instructing the application to integrate a factory.

3) Configuration file exists but does not by itself provide runtime behavior

- File: `config/simpaticoder-cli.toml` exists at `config/` and was added/edited as part of the feature. The config defines commands, adapters, agent defaults, and mappings to ABK command names and special handlers. However, ABK's runtime needs either:
  - registered handlers from the application, or
  - ABK-provided factories that can construct application-specific things like the agent.

Therefore the config alone is not enough to run the agent without application-level code.

## Observed mismatch between stated intent and code

- Stated intent (documentation & PRDs): zero/one lines in simpaticoder (one-line usage), and ABK owning CLI runtime.
- Actual code: ABK provides the declarative runtime and APIs, but leaves runtime agent construction unimplemented and expects the application to register handlers; simpaticoder in the feature branch registers large handler closures (reintroducing application-level CLI code) including direct agent creation and filesystem actions. Thus the codebase ended up in a hybrid approach instead of the intended separation.

## Developer actions noted (missed points and narrative)

This section records the observed developer decisions and where they missed the intended target.

- The developer created `config/simpaticoder-cli.toml` under `config/` — this is the expected declarative configuration, and it contains mappings for `agent` settings, adapters, and command definitions.
- The developer added an `agent` feature to ABK (i.e., `abk[agent]`) and implemented a declarative CLI in ABK exposing registration points and an executor.
- Despite adding the ABK agent feature and the declarative CLI, the developer continued to add application-specific handler code in `simpaticoder/src/lib.rs` and also reintroduced CLI code under `src/cli` (see `src/cli.backup`, `src/cli_backup`, and `src/lib.rs` where handlers are registered). In short: instead of moving runtime factories into ABK (or providing a minimal glue call from simpaticoder), substantial runtime logic was left or re-added in the application crate.
- The developer reported that `abk[agent]` is required; they created it. But the output shows ABK still contains explicit stubs requiring the application to register factories/handlers. This means the task was partially completed (config + feature flag added, ABK declarative framework added) but the crucial agent construction factory was not fully moved into ABK or otherwise automated such that simpaticoder could remain free of runtime handlers.
- The sequence of commits shows repeated documentation claims of "ONE LINE" and refactors that indicate CLI extraction, while runtime closures continued to exist or reappear in the application repository. This suggests either a misunderstanding about responsibility boundaries or an incomplete extraction.

## Impact and risks

- Duplication of responsibility: CLI behavior is partially implemented in ABK and partially in the application. This means developers must understand two places to modify CLI behavior.
- Increased maintenance: Large closures in the consumer crate (simpaticoder) undermine the purpose of moving logic into ABK and increase churn when the CLI or agent behavior must change.
- Confusion about ownership: ABK's documentation and error messages indicate it expects application integration, but the overarching design and PRDs aimed for minimal application code. This mismatch will cause confusion and friction.
- Testing complexity: Tests that validate CLI behavior need to consider both ABK and simpaticoder code paths.
- Deployment/packaging concerns: If the application expects to be a one-liner but actually needs many lines to register handlers and create agents, packaging and distribution instructions will be inaccurate.

## How to inspect the divergence locally (repro steps for reviewers)

- Show branch and recent commits in simpaticoder:

  git -C /path/to/simpaticoder branch --show-current
  git -C /path/to/simpaticoder log --oneline --decorate --graph main..feature/declarative-cli-framework

- Show ABK's branch and commits (tmp/abk is a subdirectory with its own git repo):

  git -C /path/to/simpaticoder/tmp/abk rev-parse --abbrev-ref HEAD
  git -C /path/to/simpaticoder/tmp/abk log --oneline --decorate --graph main..feature/declarative-cli-framework

- Inspect application runtime (where handlers live):

  less src/lib.rs
  # look for `DeclarativeCli::from_file` and `.register_special_handler`

- Inspect ABK declarative executor and registration APIs:

  less tmp/abk/src/cli/declarative/executor.rs
  # look for `register_special_handler`, `register_abk_handler`, `execute_agent_run` and related stubs

- Inspect the CLI config file:

  less config/simpaticoder-cli.toml

## Conclusions (issue statement only)

- The original design objective (move CLI entirely to ABK; application runs a single-line invocation) is not realized.
- ABK contains the declarative framework but intentionally delegates runtime agent construction and special handling to the application.
- Simpaticoder's feature branch reintroduced significant runtime code (agent creation and init logic) into the application.

This mismatch should be triaged: it may be a misunderstanding of responsibilities, an incomplete ABK feature (agent factory not extracted), or an incremental change that accidentally reintroduced code into simpaticoder. The next step is to evaluate ownership and decide whether to move the necessary runtime factories into ABK or to accept a small application-level glue layer (but that would still need to be documented and kept minimal).

---

Generated by automation: document created on branch `feature/declarative-cli-framework` to capture current state. This is an issue report (no solution/proposed changes included here).
