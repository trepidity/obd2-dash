use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Anthropic,
    #[serde(alias = "openai")]
    OpenAI,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
}

impl AiConfig {
    /// Resolved model name, falling back to sensible defaults.
    pub fn model_name(&self) -> &str {
        self.model.as_deref().unwrap_or(match self.provider {
            AiProvider::Anthropic => "claude-sonnet-4-20250514",
            AiProvider::OpenAI => "gpt-4o",
        })
    }

    /// Resolved max tokens.
    pub fn resolved_max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(4096)
    }

    /// Load config from a JSON file, returning None if missing or invalid.
    pub fn load(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<AiConfig>(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded AI config from {}", path.display());
                    Some(config)
                }
                Err(e) => {
                    tracing::warn!("Invalid AI config at {}: {}", path.display(), e);
                    None
                }
            },
            Err(e) => {
                tracing::warn!("Could not read AI config at {}: {}", path.display(), e);
                None
            }
        }
    }

    /// Provider display name.
    pub fn provider_name(&self) -> &str {
        match self.provider {
            AiProvider::Anthropic => "Anthropic",
            AiProvider::OpenAI => "OpenAI",
        }
    }
}
