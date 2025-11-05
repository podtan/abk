// Implement checkpoint-related helpers for Agent in a separate module
// Keep visibility as pub(crate) to allow intra-crate usage while keeping API surface minimal
impl super::Agent {
    /// Convert Agent WorkflowStep to checkpoint WorkflowStep
    pub(crate) fn agent_step_to_checkpoint_step(
        &self,
        step: &super::WorkflowStep,
    ) -> crate::checkpoint::models::WorkflowStep {
        use crate::checkpoint::models::WorkflowStep as CheckpointWorkflowStep;
        match step {
            super::WorkflowStep::Analyze => CheckpointWorkflowStep::Analyze,
            super::WorkflowStep::Reproduce => CheckpointWorkflowStep::Reproduce,
            super::WorkflowStep::Propose => CheckpointWorkflowStep::Propose,
            super::WorkflowStep::Apply => CheckpointWorkflowStep::Apply,
            super::WorkflowStep::Verify => CheckpointWorkflowStep::Verify,
            super::WorkflowStep::Complete => CheckpointWorkflowStep::Complete,
        }
    }

    /// Convert checkpoint WorkflowStep to agent WorkflowStep
    pub(crate) fn checkpoint_step_to_agent_step(
        &self,
        step: &crate::checkpoint::models::WorkflowStep,
    ) -> super::WorkflowStep {
        use crate::checkpoint::models::WorkflowStep as CheckpointWorkflowStep;
        match step {
            CheckpointWorkflowStep::Analyze => super::WorkflowStep::Analyze,
            CheckpointWorkflowStep::Reproduce => super::WorkflowStep::Reproduce,
            CheckpointWorkflowStep::Propose => super::WorkflowStep::Propose,
            CheckpointWorkflowStep::Apply => super::WorkflowStep::Apply,
            CheckpointWorkflowStep::Verify => super::WorkflowStep::Verify,
            CheckpointWorkflowStep::Complete => super::WorkflowStep::Complete,
            CheckpointWorkflowStep::Error => super::WorkflowStep::Analyze, // Default fallback
            CheckpointWorkflowStep::Paused => super::WorkflowStep::Analyze, // Default fallback
        }
    }

    /// Estimate token count for a message (simple heuristic)
    pub(crate) fn estimate_token_count(&self, content: &str) -> usize {
        // Simple estimation: roughly 4 characters per token for English text
        (content.len() + 3) / 4
    }
}
