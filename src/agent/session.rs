//! Agent session management - thin wrapper over ABK orchestration
//!
//! This module delegates workflow execution to abk::orchestration while
//! maintaining the agent-specific session lifecycle methods.

use anyhow::Result;

impl super::Agent {
    /// Start an agent session with optional task classification.
    ///
    /// This method delegates to the SessionManager from abk[checkpoint].
    pub async fn start_session(
        &mut self,
        task_description: &str,
        additional_context: Option<&str>,
    ) -> Result<String> {
        let mut session_manager = self.session_manager
            .take()
            .ok_or_else(|| anyhow::anyhow!("SessionManager not initialized"))?;

        let result = session_manager
            .start_session(self, task_description, additional_context)
            .await;

        self.session_manager = Some(session_manager);
        result
    }

    /// Resume session from a checkpoint, restoring conversation history.
    ///
    /// This method delegates to the SessionManager from abk[checkpoint].
    pub async fn resume_from_checkpoint(
        &mut self,
        project_path: &std::path::Path,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Result<String> {
        let mut session_manager = self.session_manager
            .take()
            .ok_or_else(|| anyhow::anyhow!("SessionManager not initialized"))?;

        let result = session_manager
            .resume_from_checkpoint(self, project_path, session_id, checkpoint_id)
            .await;

        self.session_manager = Some(session_manager);
        result
    }

    /// Stop the agent session.
    pub async fn stop_session(&mut self, reason: &str) -> Result<String> {
        self.is_running = false;

        if let Some(turn_id) = &self.current_turn_id {
            println!(
                "🔑 Ending conversation turn: {} (Total requests: {})",
                turn_id, self.turn_request_count
            );
        }
        self.end_conversation_turn();

        if let Err(e) = self.finalize_checkpoint_session().await {
            self.logger.log_error(
                &format!("Warning: Failed to finalize checkpoint session: {}", e),
                None,
            )?;
        }

        self.logger.log_completion(reason)?;
        Ok(format!("Session completed: {}", reason))
    }

    /// Run the agent workflow loop.
    ///
    /// Delegates to ABK orchestration which handles:
    /// - Iteration loop and state management
    /// - LLM generation with retries
    /// - Tool call execution
    /// - Checkpoint creation
    /// - Classification and template loading
    /// - Completion detection
    pub async fn run_workflow(&mut self, max_iterations: u32) -> Result<String> {
        crate::orchestration::run_workflow(self, max_iterations).await
    }

    /// Run workflow using streaming approach.
    ///
    /// Delegates to ABK orchestration streaming function which handles:
    /// - Unified streaming workflow
    /// - Tool calls within streaming responses
    /// - Classification and template loading
    /// - Completion detection via submit or markers
    pub async fn run_workflow_streaming(&mut self, max_iterations: u32) -> Result<String> {
        crate::orchestration::run_workflow_streaming(self, max_iterations).await
    }

    /// Finalize the current checkpoint session when workflow completes or is interrupted.
    async fn finalize_checkpoint_session(&mut self) -> Result<()> {
        if let Some(ref mut session_storage) = self.current_session {
            if let Err(e) = session_storage.synchronize_metadata().await {
                self.logger.log_error(
                    &format!("Warning: Failed to synchronize session metadata: {}", e),
                    None,
                )?;
            }
        }
        Ok(())
    }
}
