# ABK Checkpoint & Project Storage Format

This document describes how the `abk[checkpoint]` subsystem lays out project and session data on disk, and the JSON formats used for metadata and checkpoint files.

Paths described here use the placeholder `{app-name}` for the application name (for example `simpaticoder` or `trustee`) and `{FOLDER_HASH}` for a stable hash derived from the project root (implementation-specific). A single machine may host many projects under `~/.{app-name}/projects/`.

Directory layout
----------------

At initialization the checkpoint system creates:

- `~/.{app-name}/projects/{FOLDER_HASH}/` — project root directory
  - `metadata.json` — project-level metadata (one file)
  - `sessions/` — directory containing all sessions
    - `{session_id}/` — one directory per session (multiple)
      - `metadata.json` — session-level metadata
      - `001_analyze.json`, `002_analyze.json`, ... — ordered action/observation records
      - `checkpoints.json` — checkpoint index (described below)
      - other payload files (artifacts, attachments)

Notes:
- Session directories are created per agent run or logical session. `session_id` may be a timestamped string, UUID, or user-provided label — the code should document the naming policy used by the caller.
- Numeric prefixes on `*_analyze.json` files ensure lexical ordering corresponds to chronological order.

JSON files and schemas
----------------------

1) Project-level `metadata.json`

Purpose: describe the project-level identity and runtime statistics used by the checkpoint subsystem.

The actual on-disk `metadata.json` produced by current ABK implementations uses the following fields (example taken from an existing project):

{
  "project_hash": "72d2d90308c9e884",
  "project_path": "/home/leo/Infra",
  "name": "Infra",
  "created_at": "2025-12-09T17:01:52.331786577Z",
  "last_accessed": "2025-12-09T17:01:52.331786661Z",
  "session_count": 0,
  "size_bytes": 0,
  "git_remote": null
}

Fields (explanation):
- `project_hash` (string): the FOLDER_HASH used in the path. Stable identifier for the project directory under `~/.{app-name}/projects/`.
- `project_path` (string): canonical absolute path to the project workspace on disk.
- `name` (string): short, human-friendly project name.
- `created_at` / `last_accessed` (ISO 8601): timestamps for lifecycle bookkeeping.
- `session_count` (integer): number of sessions recorded for this project (may be 0).
- `size_bytes` (integer): approximate size of project data on disk.
- `git_remote` (nullable string): upstream remote URL if detected, or `null`.

Directory notes:
- The project directory typically contains a `sessions/` subdirectory (holds session folders) and a `cache/` directory for temporary data. If `session_count` is `0` then `sessions/` may be empty.

2) Session-level `metadata.json`

Purpose: describe a single session run, configuration, state pointers, and checkpoints index reference.

The actual on-disk `metadata.json` produced by current ABK implementations uses the following fields (example taken from an existing session):

{
  "session_id": "session_2025_12_09_17_01_my_name_is",
  "project_hash": "72d2d90308c9e884",
  "created_at": "2025-12-09T17:03:10.044824449Z",
  "last_accessed": "2025-12-09T17:03:10.202868114Z",
  "checkpoint_count": 4,
  "status": "Active",
  "description": null,
  "tags": [],
  "size_bytes": 0
}

Fields (explanation):
- `session_id` (string): unique session identifier, typically includes timestamp and description.
- `project_hash` (string): the project hash this session belongs to.
- `created_at` / `last_accessed` (ISO 8601): timestamps for lifecycle bookkeeping.
- `checkpoint_count` (integer): number of checkpoints recorded for this session.
- `status` (string): current session status ("Active", "Completed", "Failed", "Archived").
- `description` (nullable string): optional human-readable description.
- `tags` (array of strings): user-defined tags for categorization.
- `size_bytes` (integer): approximate size of session data on disk.

3) Numbered analyze files: `NNN_analyze.json`

Purpose: store step-by-step action/observation artifacts for the session. These files are produced by `abk[checkpoint]` and the examples below are exact — taken from a real session stored in `~/.{app-name}/projects/{FOLDER_HASH}/sessions/session_2025_12_09_17_01_my_name_is/001_analyze.json`.

Exact example (verbatim):

{
  "metadata": {
    "checkpoint_id": "001_analyze",
    "session_id": "session_001_analyze",
    "project_hash": "unknown_project",
    "created_at": "2025-12-09T17:01:52.332795344Z",
    "iteration": 1,
    "workflow_step": "Analyze",
    "checkpoint_version": "1.0",
    "compressed_size": 0,
    "uncompressed_size": 0,
    "description": "Checkpoint at analyze step, iteration 1",
    "tags": [
      "analyze",
      "iter_1"
    ]
  },
  "agent_state": {
    "current_mode": "confirm",
    "current_iteration": 1,
    "current_step": "Analyze",
    "max_iterations": 100,
    "task_description": "my name is leo, nice to meet you",
    "configuration": {
      "default_mode": "confirm",
      "max_iterations": 1000,
      "timeout_seconds": 120
    },
    "working_directory": ".",
    "session_start_time": "2025-12-09T17:01:52.332796905Z",
    "last_activity": "2025-12-09T17:01:52.332796952Z"
  },
  "conversation_state": {
    "messages": [
      {
        "role": "system",
        "content": "You are Simpaticoder, a terminal-first software engineering agent that follows a spec-driven\n development workflow.\n\nFirst, analyze the following task request and classify it into one of these categories:\n\n**Task Types:**\n- **bug_fix**: Issues that need to be resolved, errors to fix, problems to solve\n- **feature**: New functionality to implement, capabilities to add, enhancements to create  \n- **maintenance**: Code cleanup, refactoring, dependency updates, documentation improvements\n- **query**: Questions to answer, information to research, exploratory investigations\n\n**IMPORTANT**: You must use the `classify_task` tool to perform the classification. \n\nCall the `classify_task` tool with one of these exact values: bug_fix, feature, maintenance, query\n\nExample tool call format:\n```json\n{"name": "classify_task", "arguments": {"task_type": "query"}}\n```\n\nDo NOT respond with plain text classification. You MUST use the classify_task tool to classify the task, then proceed with the task execution using the appropriate approach based on the classification result.",                                                                                                                      "timestamp": "2025-12-09T17:01:52.332802181Z",
        "timestamp": "2025-12-09T17:01:52.332802181Z",
        "token_count": 268,
        "tool_calls": null,
        "tool_call_id": null,
        "name": "trustee"
      },
      {
        "role": "user",
        "content": "Please solve this issue:\n\nmy name is leo, nice to meet you\n\nI couldn't clearly classify \nthis task type, so I'm using a **general approach**. Follow this workflow carefully:\n\n## General Workflow\n\n**Complete each step thoroughly before proceeding to the next:**\n\n1. **Understand the project** - Use `filemap` to generate a quick directory overview; check helper files (e.g. .rules, .cursorrules, AGENTS.md) and read them if present\n2. **Analyze the task** - Find and read relevant files to understand the request\n3. **Identify the approach** - Determine if this is bug fix, feature, maintenance, or query\n4. **Create a specific plan** - Based on analysis, plan detailed steps\n5. **Implement the solution** - Execute your plan methodically\n6. **Verify your work** - Test that your solution addresses the request\n7. **Test edge cases** - Ensure robustness\n8. **Run existing tests** - Verify no regressions\n9. **Clean up** - Remove temporary files\n9. **Commit changes** - Use Angular commit convention: type(scope): short description\n10. **Clean up** - Remove temporary files\n\n**When you have completed ALL steps above and verified your solution works**, then use:\n\n**When you have completed ALL steps above and verified your solution works**, then use:\n\n```json\n{"name": "submit", "arguments": {}}\n```\n\n## Important Rules\n\n- **Execute multiple tools efficiently** - You can and should use multiple tools in a single response to work more efficiently\n- Always include a THOUGHT section explaining your reasoning and planned tool usage\n- Work methodically through each step\n- Don't skip verification steps\n- Only declare completion when ALL work is actually done\n\n## Current Context\n\nWorking in: .\nKey files: {key_files}\n\n## Success Criteria\n\n{success_criteria}\n\n## Additional Instructions\n\n{additional_instructions}\n\n{useful_commands}",                                                                                                                    "timestamp": "2025-12-09T17:01:52.332802398Z",
        "timestamp": "2025-12-09T17:01:52.332802398Z",
        "token_count": 458,
        "tool_calls": null,
        "tool_call_id": null,
        "name": "user"
      }
    ],
    "system_prompt": "Simpaticoder System",
    "context_window_size": 659,
    "model_configuration": {
      "model_name": "openai/gpt-5-mini",
      "max_tokens": 4000,
      "temperature": 0.7,
      "top_p": null,
      "frequency_penalty": null,
      "presence_penalty": null
    },
    "conversation_stats": {
      "total_tokens": 659,
      "total_messages": 2,
      "estimated_cost": null,
      "api_calls": 0
    }
  },
  "file_system_state": {
    "working_directory": ".",
    "tracked_files": [],
    "modified_files": [],
    "git_status": null,
    "file_permissions": {}
  },
  "tool_state": {
    "active_tools": {},
    "executed_commands": [],
    "tool_registry": {},
    "execution_context": {
      "environment_variables": {
        "PWD": "/home/leo/Infra",
        "TERM": "foot",
        "LANG": "en_US.UTF-8",
        "PATH": "/home/leo/.wasmtime/bin:/home/leo/.pixi/bin:/home/leo/.local/bin:/home/leo/.local/bin:/home/leo\n/.wasmedge/bin:/home/leo/.wasmtime/bin:/home/leo/.pixi/bin:/home/leo/.nvm/versions/node/v24.4.1/bin:/home/leo/.cargo/bin:/home/leo/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games:/home/leo/minio-binaries/:/opt/android-studio/bin/:/home/leo/.modular/bin:/home/leo/.lmstudio/bin:/home/leo/.lmstudio/bin:/home/leo/minio-binaries/:/opt/android-studio/bin/:/home/leo/.modular/bin:/home/leo/.lmstudio/bin",
        "USER": "leo",
        "SHELL": "/bin/bash",
        "HOME": "/home/leo"
      },
      "working_directory": ".",
      "timeout_seconds": 120,
      "max_retries": 3
    }
  },
  "environment_state": {
    "environment_variables": {
      "HOME": "/home/leo",
      "PWD": "/home/leo/Infra",
      "PATH": "/home/leo/.wasmtime/bin:/home/leo/.pixi/bin:/home/leo/.local/bin:/home/leo/.local/bin:/home/leo/\n.wasmedge/bin:/home/leo/.wasmtime/bin:/home/leo/.pixi/bin:/home/leo/.nvm/versions/node/v24.4.1/bin:/home/leo/.cargo/bin:/home/leo/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games:/home/leo/minio-binaries/:/opt/android-studio/bin/:/home/leo/.modular/bin:/home/leo/.lmstudio/bin:/home/leo/.lmstudio/bin:/home/leo/minio-binaries/:/opt/android-studio/bin/:/home/leo/.modular/bin:/home/leo/.lmstudio/bin",
      "TERM": "foot",
      "SHELL": "/bin/bash",
      "USER": "leo",
      "LANG": "en_US.UTF-8"
    },
    "system_info": {
      "os_name": "Linux",
      "os_version": "6.12.41+deb13-amd64",
      "architecture": "x86_64",
      "hostname": "pouvoir",
      "cpu_count": 16,
      "total_memory": 50275577856
    },
    "process_info": {
      "pid": 327292,
      "parent_pid": null,
      "start_time": "2025-12-09T17:01:52.414448420Z",
      "command_line": [],
      "working_directory": "."
    },
    "resource_usage": {
      "cpu_usage": 0.0,
      "memory_usage": 0,
      "disk_usage": 0,
      "network_bytes_sent": 0,
      "network_bytes_received": 0
    }
  }
}

Notes (as implemented):
- Top-level keys in `*_analyze.json` files are: `metadata`, `agent_state`, `conversation_state`, `file_system_state`, `tool_state`, and `environment_state`.
- `metadata` contains checkpoint bookkeeping fields such as `checkpoint_id`, `session_id`, `iteration`, `workflow_step`, `checkpoint_version`, and `tags`.
- The `conversation_state.messages` array stores full messages with fields: `role`, `content`, `timestamp`, `token_count`, `tool_calls`, `tool_call_id`, and `name` (where present).
- `agent_state` contains the runtime configuration and agent progress counters (exact field names are shown above).

Rules (as implemented):
- Filenames use zero-padded numeric prefixes (`001_analyze.json`, `002_analyze.json`, ...). The agent code writes these files sequentially.
- Session folders live under `sessions/` and contain `metadata.json`, `checkpoints.json`, and `*_analyze.json` files (and possible artifact files referenced by `checkpoints.json`).

4) `checkpoints.json` — checkpoint index

Purpose: index of checkpoints recorded during the session. Keeps a catalog of checkpoint metadata keyed by checkpoint ID.

Example structure (actual format):

{
  "001_analyze": {
    "checkpoint_id": "001_analyze",
    "session_id": "session_001_analyze",
    "project_hash": "unknown_project",
    "created_at": "2025-12-09T17:01:52.332795344Z",
    "iteration": 1,
    "workflow_step": "Analyze",
    "checkpoint_version": "1.0",
    "compressed_size": 0,
    "uncompressed_size": 0,
    "description": "Checkpoint at analyze step, iteration 1",
    "tags": ["analyze", "iter_1"]
  },
  "002_analyze": {
    "checkpoint_id": "002_analyze",
    "session_id": "session_002_analyze",
    "project_hash": "unknown_project",
    "created_at": "2025-12-09T17:01:58.427059782Z",
    "iteration": 2,
    "workflow_step": "Analyze",
    "checkpoint_version": "1.0",
    "compressed_size": 0,
    "uncompressed_size": 0,
    "description": "Checkpoint at analyze step, iteration 2",
    "tags": ["analyze", "iter_2"]
  }
}

Structure:
- Top-level object with checkpoint IDs as keys.
- Each value is a `CheckpointMetadata` object containing:
  - `checkpoint_id` (string): unique identifier for the checkpoint.
  - `session_id` (string): parent session identifier.
  - `project_hash` (string): project identifier.
  - `created_at` (ISO 8601): creation timestamp.
  - `iteration` (integer): agent workflow iteration.
  - `workflow_step` (string): current step ("Analyze", "Reproduce", "Propose", "Apply", "Verify", "Complete", "Error", "Paused").
  - `checkpoint_version` (string): format version.
  - `compressed_size` / `uncompressed_size` (integers): size information.
  - `description` (string): human-readable description.
  - `tags` (array of strings): categorization tags.

Files for large snapshots
------------------------

- Large snapshot artifacts (workspace tarballs, zipped artifacts) should be stored in the session directory and referenced from `checkpoints.json` rather than embedding large blobs within JSON.
- Use `snapshot-<id>.tar.gz` (or `.zip`) and prefer streaming-friendly formats. Consider storing checksums alongside the index for integrity checks.

Versioning & migration
----------------------

- Each top-level JSON should have a `version` string. When changing schema, increment this `version` and add a migration guideline.
- Backwards compatibility: readers SHOULD ignore unknown fields and should validate `version` before relying on fields that might change.

Security and privacy
--------------------

- Checkpoint directories may contain sensitive data (API keys, tokens, environment dumps). Do not store secrets in plaintext. The agent should never write unredacted secrets into the project/session directory.
- File permissions: restrict access to the user (e.g., `0700` for dirs, `0600` for files) where practical.

Implementation notes (reader/writer suggestions)
----------------------------------------------

- Use atomic writes when updating JSON indexes: write to a temp file then rename into place.
- Ensure operations are crash-safe: partial writes should not corrupt `checkpoints.json` — use replace/rename semantics.
- Maintain a small in-memory cache of the index when performing frequent updates, but persist after every meaningful change.

Examples and utilities
----------------------

Include small helper functions in your agent codebase to:
- Create the project folder and initial `metadata.json`.
- Create a session folder and session `metadata.json`.
- Append a new `NNN_analyze.json` file safely.
- Create a checkpoint snapshot and update `checkpoints.json` atomically.

----

Document maintained by the ABK team. If you change any on-disk formats, update `version` fields and add migration steps here.
