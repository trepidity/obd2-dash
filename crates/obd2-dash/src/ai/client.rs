use anyhow::{bail, Context, Result};
use serde_json::json;

use super::config::{AiConfig, AiProvider};
use super::insights::AiInsights;
use super::prompt::build_system_prompt;
use super::summary::SessionSummary;

pub struct AiClient {
    http: reqwest::Client,
}

impl Default for AiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    /// Send a session summary to the configured AI provider and return parsed insights.
    pub async fn analyze(
        &self,
        config: &AiConfig,
        summary: &SessionSummary,
        session_id: &str,
    ) -> Result<AiInsights> {
        let summary_json =
            serde_json::to_string_pretty(summary).context("Failed to serialize summary")?;

        let user_message = format!(
            "Analyze the following vehicle OBD-II session data and provide a diagnostic report:\n\n```json\n{}\n```",
            summary_json
        );

        let raw_markdown = match config.provider {
            AiProvider::Anthropic => self.call_anthropic(config, &user_message).await?,
            AiProvider::OpenAI => self.call_openai(config, &user_message).await?,
        };

        Ok(AiInsights::from_response(
            session_id.to_string(),
            config.provider_name().to_string(),
            config.model_name().to_string(),
            raw_markdown,
        ))
    }

    async fn call_anthropic(&self, config: &AiConfig, user_message: &str) -> Result<String> {
        let body = json!({
            "model": config.model_name(),
            "max_tokens": config.resolved_max_tokens(),
            "system": build_system_prompt(),
            "messages": [
                { "role": "user", "content": user_message }
            ]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        // Extract text from content array
        let text = resp_body["content"]
            .as_array()
            .and_then(|arr| arr.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(text)
    }

    async fn call_openai(&self, config: &AiConfig, user_message: &str) -> Result<String> {
        let body = json!({
            "model": config.model_name(),
            "max_tokens": config.resolved_max_tokens(),
            "messages": [
                { "role": "system", "content": build_system_prompt() },
                { "role": "user", "content": user_message }
            ]
        });

        let resp = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            bail!("OpenAI API error ({}): {}", status, error_msg);
        }

        let text = resp_body["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(text)
    }
}
