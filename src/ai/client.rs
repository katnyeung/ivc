use anyhow::{Context, Result};
use serde_json::json;

pub struct ClaudeClient {
    client: reqwest::Client,
    model: String,
    api_key: String,
}

impl ClaudeClient {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").context(
            "ANTHROPIC_API_KEY environment variable is not set. \
             Set it with: export ANTHROPIC_API_KEY=your-key",
        )?;

        Ok(Self {
            client: reqwest::Client::new(),
            model: model.to_string(),
            api_key,
        })
    }

    pub async fn extract_intentions(&self, prompt: &str) -> Result<String> {
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": self.model,
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send()
            .await
            .context("Failed to send request to Claude API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            anyhow::bail!("Claude API returned status {status}: {body}");
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Claude API response")?;

        let text = body["content"][0]["text"]
            .as_str()
            .context("Unexpected response format from Claude API")?;

        Ok(text.to_string())
    }
}
