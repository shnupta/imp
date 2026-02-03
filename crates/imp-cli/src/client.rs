use crate::config::{AuthMethod, Config};
use crate::error::{ImpError, Result};
use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    #[serde(rename = "type")]
    message_type: String,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    index: Option<usize>,
    delta: Option<Delta>,
    content_block: Option<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(rename = "type")]
    delta_type: String,
    text: Option<String>,
}

pub struct ClaudeClient {
    client: reqwest::Client,
    model: String,
    base_url: String,
    config: Config,
}

impl ClaudeClient {
    pub fn new(config: Config) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        Ok(Self {
            client,
            model: config.llm.model.clone(),
            base_url: "https://api.anthropic.com".to_string(),
            config,
        })
    }

    /// Ensure we have a valid token (setup-tokens are long-lived, no refresh needed)
    async fn ensure_valid_token(&mut self) -> Result<()> {
        // Setup-tokens from `claude setup-token` are long-lived and don't need refresh
        Ok(())
    }

    /// Prepare authorization headers based on the current auth method
    fn prepare_auth_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        match self.config.auth_method() {
            AuthMethod::ApiKey => {
                let api_key = self.config.api_key()
                    .ok_or_else(|| ImpError::Config("API key not found in config".to_string()))?;
                headers.insert(
                    "x-api-key",
                    HeaderValue::from_str(api_key)?,
                );
            }
            AuthMethod::OAuth => {
                let oauth_config = self.config.oauth_config()
                    .ok_or_else(|| ImpError::Config("OAuth configuration missing".to_string()))?;
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", oauth_config.access_token))?,
                );
                // Add required OAuth headers
                headers.insert(
                    "anthropic-beta",
                    HeaderValue::from_static("claude-code-20250219,oauth-2025-04-20"),
                );
                headers.insert(
                    "user-agent",
                    HeaderValue::from_static("claude-cli/1.0.0 (external, cli)"),
                );
                headers.insert(
                    "x-app",
                    HeaderValue::from_static("cli"),
                );
                headers.insert(
                    "anthropic-dangerous-direct-browser-access",
                    HeaderValue::from_static("true"),
                );
            }
        }

        Ok(headers)
    }

    pub async fn send_message(
        &mut self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Option<Value>,
        stream: bool,
    ) -> Result<AnthropicResponse> {
        // Ensure we have a valid token (refresh if necessary)
        self.ensure_valid_token().await?;

        let headers = self.prepare_auth_headers()?;

        let mut request_body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": messages,
        });

        if let Some(system) = system_prompt {
            request_body["system"] = json!(system);
        }

        if let Some(tools_value) = tools {
            request_body["tools"] = tools_value;
        }

        if stream {
            request_body["stream"] = json!(true);
            return self.send_streaming_request(headers, request_body).await;
        }

        let response = self
            .client
            .post(&format!("{}/v1/messages", self.base_url))
            .headers(headers)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(ImpError::Agent(format!("API error: {}", error_text)));
        }

        let response_data: AnthropicResponse = response.json().await?;
        Ok(response_data)
    }

    async fn send_streaming_request(
        &self,
        headers: HeaderMap,
        request_body: Value,
    ) -> Result<AnthropicResponse> {
        let response = self
            .client
            .post(&format!("{}/v1/messages", self.base_url))
            .headers(headers)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(ImpError::Agent(format!("API error: {}", error_text)));
        }

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut tool_calls: Vec<ContentBlock> = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let chunk_str = String::from_utf8_lossy(&chunk);
            
            for line in chunk_str.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..]; // Remove "data: " prefix
                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamEvent>(data) {
                        Ok(event) => {
                            match event.event_type.as_str() {
                                "content_block_start" => {
                                    if let Some(content_block) = event.content_block {
                                        if let ContentBlock::ToolUse { .. } = content_block {
                                            tool_calls.push(content_block);
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    if let Some(delta) = event.delta {
                                        if let Some(text) = delta.text {
                                            full_text.push_str(&text);
                                            print!("{}", text); // Stream to stdout
                                        }
                                    }
                                }
                                _ => {
                                    // Ignore other event types
                                }
                            }
                        }
                        Err(_) => {
                            // Skip malformed JSON events
                            continue;
                        }
                    }
                }
            }
        }

        println!(); // New line after streaming

        // Construct response
        let mut content_blocks = Vec::new();
        if !full_text.is_empty() {
            content_blocks.push(ContentBlock::Text { text: full_text });
        }
        content_blocks.extend(tool_calls);

        Ok(AnthropicResponse {
            message_type: "message".to_string(),
            content: content_blocks,
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        })
    }

    pub fn extract_text_content(&self, response: &AnthropicResponse) -> String {
        response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn extract_tool_calls(&self, response: &AnthropicResponse) -> Vec<ToolCall> {
        response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}