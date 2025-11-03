//! Workflow management - task execution coordination

use anyhow::Result;

/// Workflow step in agent execution
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowStep {
    /// Initial analysis phase
    Analyze,
    /// Problem reproduction phase
    Reproduce,
    /// Solution proposal phase
    Propose,
    /// Solution application phase
    Apply,
    /// Verification/testing phase
    Verify,
    /// Completion phase
    Complete,
}

impl WorkflowStep {
    /// Get human-readable name for the step
    pub fn name(&self) -> &str {
        match self {
            Self::Analyze => "analyze",
            Self::Reproduce => "reproduce",
            Self::Propose => "propose",
            Self::Apply => "apply",
            Self::Verify => "verify",
            Self::Complete => "complete",
        }
    }

    /// Check if this is a terminal step
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

impl std::fmt::Display for WorkflowStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Agent execution modes
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Execute bash commands only
    BashOnly,
    /// Execute tools only
    ToolsOnly,
    /// Hybrid mode (default) - both bash and tools
    Hybrid,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionMode::BashOnly => write!(f, "bash_only"),
            ExecutionMode::ToolsOnly => write!(f, "tools_only"),
            ExecutionMode::Hybrid => write!(f, "hybrid"),
        }
    }
}

impl std::str::FromStr for ExecutionMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "bash_only" => Ok(ExecutionMode::BashOnly),
            "tools_only" => Ok(ExecutionMode::ToolsOnly),
            "hybrid" => Ok(ExecutionMode::Hybrid),
            _ => Err(anyhow::anyhow!("Invalid execution mode: {}", s)),
        }
    }
}

/// Agent interaction modes
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    /// Confirm before actions
    Confirm,
    /// Execute without confirmation (YOLO mode)
    Yolo,
    /// Human-in-the-loop mode
    Human,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Confirm => write!(f, "confirm"),
            AgentMode::Yolo => write!(f, "yolo"),
            AgentMode::Human => write!(f, "human"),
        }
    }
}

impl std::str::FromStr for AgentMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "confirm" => Ok(AgentMode::Confirm),
            "yolo" => Ok(AgentMode::Yolo),
            "human" => Ok(AgentMode::Human),
            _ => Err(anyhow::anyhow!("Invalid agent mode: {}", s)),
        }
    }
}


/// Workflow coordinator
///
/// Manages the progression through workflow steps and transitions.
pub struct WorkflowCoordinator {
    current_step: WorkflowStep,
    step_history: Vec<WorkflowStep>,
}

impl WorkflowCoordinator {
    /// Create a new workflow coordinator
    pub fn new() -> Self {
        Self {
            current_step: WorkflowStep::Analyze,
            step_history: Vec::new(),
        }
    }

    /// Get current workflow step
    pub fn current_step(&self) -> &WorkflowStep {
        &self.current_step
    }

    /// Get step history
    pub fn step_history(&self) -> &[WorkflowStep] {
        &self.step_history
    }

    /// Transition to next workflow step
    pub fn transition_to(&mut self, step: WorkflowStep) -> Result<()> {
        self.step_history.push(self.current_step.clone());
        self.current_step = step;
        Ok(())
    }

    /// Check if workflow is complete
    pub fn is_complete(&self) -> bool {
        self.current_step.is_terminal()
    }

    /// Reset workflow to initial state
    pub fn reset(&mut self) {
        self.current_step = WorkflowStep::Analyze;
        self.step_history.clear();
    }
}

impl Default for WorkflowCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_progression() {
        let mut coordinator = WorkflowCoordinator::new();
        
        assert_eq!(coordinator.current_step(), &WorkflowStep::Analyze);
        assert!(coordinator.step_history().is_empty());
        assert!(!coordinator.is_complete());

        coordinator.transition_to(WorkflowStep::Propose).unwrap();
        assert_eq!(coordinator.current_step(), &WorkflowStep::Propose);
        assert_eq!(coordinator.step_history().len(), 1);

        coordinator.transition_to(WorkflowStep::Complete).unwrap();
        assert!(coordinator.is_complete());
    }

    #[test]
    fn test_workflow_reset() {
        let mut coordinator = WorkflowCoordinator::new();
        
        coordinator.transition_to(WorkflowStep::Propose).unwrap();
        coordinator.transition_to(WorkflowStep::Apply).unwrap();
        
        coordinator.reset();
        assert_eq!(coordinator.current_step(), &WorkflowStep::Analyze);
        assert!(coordinator.step_history().is_empty());
    }
}
