//! Configuration for LLM generation requests.
//!
//! This module defines the configuration options for generating responses from LLMs,
//! including model selection, temperature, tools, and provider-specific options.

use crate::provider::types::tools::{InternalToolDefinition, ToolChoice};
use serde::{Deserialize, Serialize};

/// Configuration for a generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateConfig {
    /// Model to use (None = use provider default)
    pub model: Option<String>,
    /// Temperature for sampling (0.0 = deterministic, 2.0 = very random)
    pub temperature: f32,
    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,
    /// Tools available for the LLM to call
    pub tools: Option<Vec<InternalToolDefinition>>,
    /// Tool choice strategy
    pub tool_choice: Option<ToolChoice>,
    /// Whether to enable streaming
    pub enable_streaming: bool,
    /// X-Request-Id for GitHub Copilot conversation turn grouping
    pub x_request_id: Option<String>,
}

impl GenerateConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            model: None,
            temperature: 0.7,
            max_tokens: Some(4000),
            tools: None,
            tool_choice: None,
            enable_streaming: false,
            x_request_id: None,
        }
    }

    /// Set the model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the temperature
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set tools
    pub fn with_tools(mut self, tools: Vec<InternalToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set tool choice
    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Enable streaming
    pub fn with_streaming(mut self, enable: bool) -> Self {
        self.enable_streaming = enable;
        self
    }

    /// Set X-Request-Id
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.x_request_id = Some(request_id.into());
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.temperature < 0.0 || self.temperature > 2.0 {
            anyhow::bail!("Temperature must be between 0.0 and 2.0");
        }

        if let Some(max_tokens) = self.max_tokens {
            if max_tokens == 0 {
                anyhow::bail!("Max tokens must be greater than 0");
            }
        }

        // Validate tools if present
        if let Some(ref tools) = self.tools {
            for tool in tools {
                tool.validate()?;
            }
        }

        Ok(())
    }
}

impl Default for GenerateConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::tools::InternalToolDefinition;

    #[test]
    fn test_config_creation() {
        let config = GenerateConfig::new();
        assert_eq!(config.temperature, 0.7);
        assert_eq!(config.max_tokens, Some(4000));
        assert!(!config.enable_streaming);
        assert!(config.model.is_none());
    }

    #[test]
    fn test_config_builder() {
        let config = GenerateConfig::new()
            .with_model("gpt-4")
            .with_temperature(0.5)
            .with_max_tokens(2000)
            .with_streaming(true)
            .with_request_id("req_123");

        assert_eq!(config.model, Some("gpt-4".to_string()));
        assert_eq!(config.temperature, 0.5);
        assert_eq!(config.max_tokens, Some(2000));
        assert!(config.enable_streaming);
        assert_eq!(config.x_request_id, Some("req_123".to_string()));
    }

    #[test]
    fn test_config_validation() {
        // Valid config
        let config = GenerateConfig::new();
        assert!(config.validate().is_ok());

        // Invalid temperature (too low)
        let mut config = GenerateConfig::new();
        config.temperature = -0.1;
        assert!(config.validate().is_err());

        // Invalid temperature (too high)
        let mut config = GenerateConfig::new();
        config.temperature = 2.1;
        assert!(config.validate().is_err());

        // Invalid max tokens
        let mut config = GenerateConfig::new();
        config.max_tokens = Some(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_with_tools() {
        let tool = InternalToolDefinition::new(
            "test_tool",
            "A test tool",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        );

        let config = GenerateConfig::new()
            .with_tools(vec![tool])
            .with_tool_choice(ToolChoice::Auto);

        assert!(config.tools.is_some());
        assert!(config.tool_choice.is_some());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = GenerateConfig::new()
            .with_model("gpt-4")
            .with_temperature(0.8);

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GenerateConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, Some("gpt-4".to_string()));
        assert_eq!(deserialized.temperature, 0.8);
    }
}
