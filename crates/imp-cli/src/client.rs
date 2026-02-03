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
    api_key: String,
    model: String,
    base_url: String,
}

impl ClaudeClient {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        Self {
            client,
            api_key,
            model: model.unwrap_or_else(|| "claude-opus-4-5-20251101".to_string()),
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    pub async fn send_message(
        &self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Option<Value>,
        stream: bool,
    ) -> Result<AnthropicResponse> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

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