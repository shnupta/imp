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
    pub content: Value,
}

impl Message {
    /// Create a simple text message
    pub fn text(role: &str, text: &str) -> Self {
        Self {
            role: role.to_string(),
            content: Value::String(text.to_string()),
        }
    }

    /// Create a message with structured content blocks
    pub fn with_content(role: &str, content: Value) -> Self {
        Self {
            role: role.to_string(),
            content,
        }
    }

    /// Create a tool result message
    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        let content_blocks: Vec<Value> = results
            .into_iter()
            .map(|result| {
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": result.tool_use_id,
                    "content": result.content
                });
                if result.is_error.unwrap_or(false) {
                    block["is_error"] = Value::Bool(true);
                }
                block
            })
            .collect();

        Self {
            role: "user".to_string(),
            content: Value::Array(content_blocks),
        }
    }

    /// Get the text content from this message (for display purposes)
    pub fn text_content(&self) -> String {
        match &self.content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => {
                blocks
                    .iter()
                    .filter_map(|block| {
                        if let Some(text_block) = block.get("text") {
                            text_block.as_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            }
            _ => String::new(),
        }
    }
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
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
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
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: Option<String> },
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
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
    thinking: Option<String>,
    signature: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
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

        let base_url = config.llm.base_url.clone()
            .unwrap_or_else(|| "https://api.anthropic.com".to_string())
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            client,
            model: config.llm.model.clone(),
            base_url,
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
        self.send_message_with_options(messages, system_prompt, tools, stream, None).await
    }

    pub async fn send_message_with_options(
        &mut self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Option<Value>,
        stream: bool,
        max_tokens_override: Option<u32>,
    ) -> Result<AnthropicResponse> {
        self.send_message_inner(messages, system_prompt, tools, stream, max_tokens_override, None).await
    }

    /// Full-control message send with all overrides.
    /// `thinking_override`: Some(true) = force on, Some(false) = force off, None = use config.
    pub async fn send_message_inner(
        &mut self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Option<Value>,
        stream: bool,
        max_tokens_override: Option<u32>,
        thinking_override: Option<bool>,
    ) -> Result<AnthropicResponse> {
        // Ensure we have a valid token (refresh if necessary)
        self.ensure_valid_token().await?;

        let headers = self.prepare_auth_headers()?;

        let use_thinking = thinking_override.unwrap_or(self.config.thinking.enabled);
        let base_max = max_tokens_override.unwrap_or(self.config.llm.max_tokens);
        // When thinking is enabled, max_tokens must exceed budget_tokens
        let max_tokens = if use_thinking {
            let min_required = self.config.thinking.budget_tokens + 4096;
            std::cmp::max(base_max, min_required)
        } else {
            base_max
        };

        let mut request_body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": messages,
        });

        // Add thinking configuration
        if use_thinking {
            request_body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": self.config.thinking.budget_tokens
            });
            // Temperature must not be set when thinking is enabled
            request_body.as_object_mut().unwrap().remove("temperature");
        }

        if let Some(system) = system_prompt {
            // OAuth tokens require Claude Code identity prefix
            if self.config.auth_method() == &AuthMethod::OAuth {
                request_body["system"] = json!([
                    {
                        "type": "text",
                        "text": "You are Claude Code, Anthropic's official CLI for Claude."
                    },
                    {
                        "type": "text",
                        "text": system,
                        "cache_control": { "type": "ephemeral" }
                    }
                ]);
            } else {
                // Use array format for cache_control support
                request_body["system"] = json!([{
                    "type": "text",
                    "text": system,
                    "cache_control": { "type": "ephemeral" }
                }]);
            }
        } else if self.config.auth_method() == &AuthMethod::OAuth {
            request_body["system"] = json!([{
                "type": "text",
                "text": "You are Claude Code, Anthropic's official CLI for Claude.",
                "cache_control": { "type": "ephemeral" }
            }]);
        }

        if let Some(tools_value) = tools {
            // Add cache_control to the last tool for prompt caching
            if let Value::Array(mut tools_array) = tools_value {
                if let Some(last_tool) = tools_array.last_mut() {
                    if let Value::Object(ref mut obj) = last_tool {
                        obj.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
                    }
                }
                request_body["tools"] = Value::Array(tools_array);
            } else {
                request_body["tools"] = tools_value;
            }
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
        let mut tool_calls_in_progress: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new(); // index -> (id, name, accumulated_input)
        let mut finalized_tool_calls: Vec<ContentBlock> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut thinking_in_progress: std::collections::HashMap<usize, (String, Option<String>)> = std::collections::HashMap::new(); // (thinking_text, signature)
        let mut finalized_thinking: Vec<ContentBlock> = Vec::new();
        let mut thinking_announced = false;
        let mut usage_input_tokens: u32 = 0;
        let mut usage_output_tokens: u32 = 0;
        let mut usage_cache_creation: u32 = 0;
        let mut usage_cache_read: u32 = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let chunk_str = String::from_utf8_lossy(&chunk);
            
            for line in chunk_str.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..]; // Remove "data: " prefix
                    if data == "[DONE]" {
                        break;
                    }

                    // Parse usage from message_start and message_delta before
                    // attempting to deserialize into StreamEvent (different shape).
                    if let Ok(raw) = serde_json::from_str::<Value>(data) {
                        match raw.get("type").and_then(|t| t.as_str()) {
                            Some("message_start") => {
                                if let Some(usage) = raw.pointer("/message/usage") {
                                    usage_input_tokens = usage.get("input_tokens")
                                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                    usage_output_tokens = usage.get("output_tokens")
                                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                    usage_cache_creation = usage.get("cache_creation_input_tokens")
                                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                    usage_cache_read = usage.get("cache_read_input_tokens")
                                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                }
                                continue;
                            }
                            Some("message_delta") => {
                                // Capture output tokens from top-level usage
                                if let Some(usage) = raw.get("usage") {
                                    if let Some(out) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                        usage_output_tokens = out as u32;
                                    }
                                }
                                // Also check for stop_reason in delta
                                if let Some(delta) = raw.get("delta") {
                                    if let Some(reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                        stop_reason = Some(reason.to_string());
                                    }
                                }
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match serde_json::from_str::<StreamEvent>(data) {
                        Ok(event) => {
                            match event.event_type.as_str() {
                                "content_block_start" => {
                                    if let Some(content_block) = event.content_block {
                                        match content_block {
                                            ContentBlock::ToolUse { id, name, .. } => {
                                                // Start tracking this tool call
                                                if let Some(index) = event.index {
                                                    tool_calls_in_progress.insert(index, (id, name, String::new()));
                                                }
                                            }
                                            ContentBlock::Thinking { .. } => {
                                                if let Some(index) = event.index {
                                                    thinking_in_progress.insert(index, (String::new(), None));
                                                    if !thinking_announced {
                                                        eprint!("{}", console::style("💭 Thinking...").dim());
                                                        thinking_announced = true;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    if let Some(delta) = event.delta {
                                        match delta.delta_type.as_str() {
                                            "text_delta" => {
                                                if let Some(text) = delta.text {
                                                    full_text.push_str(&text);
                                                    print!("{}", text); // Stream to stdout
                                                }
                                            }
                                            "thinking_delta" => {
                                                if let Some(thinking_text) = delta.thinking {
                                                    if let Some(index) = event.index {
                                                        if let Some((ref mut accumulated, _)) = thinking_in_progress.get_mut(&index) {
                                                            accumulated.push_str(&thinking_text);
                                                        }
                                                    }
                                                }
                                            }
                                            "input_json_delta" => {
                                                if let Some(partial_json) = delta.partial_json {
                                                    if let Some(index) = event.index {
                                                        if let Some((_id, _name, ref mut accumulated_input)) = tool_calls_in_progress.get_mut(&index) {
                                                            accumulated_input.push_str(&partial_json);
                                                        }
                                                    }
                                                }
                                            }
                                            "signature_delta" => {
                                                if let Some(sig) = delta.signature {
                                                    if let Some(index) = event.index {
                                                        if let Some((_, ref mut signature)) = thinking_in_progress.get_mut(&index) {
                                                            *signature = Some(sig);
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    // Finalize thinking block if it was in progress
                                    if let Some(index) = event.index {
                                        if let Some((accumulated, signature)) = thinking_in_progress.remove(&index) {
                                            if thinking_announced {
                                                eprintln!(" {}", console::style("done").dim());
                                            }
                                            finalized_thinking.push(ContentBlock::Thinking { thinking: accumulated, signature });
                                        }
                                    }
                                    // Finalize tool call if it was in progress
                                    if let Some(index) = event.index {
                                        if let Some((id, name, accumulated_input)) = tool_calls_in_progress.remove(&index) {
                                            // Parse the accumulated JSON input
                                            let input = if accumulated_input.is_empty() {
                                                Value::Object(serde_json::Map::new()) // Empty object
                                            } else {
                                                match serde_json::from_str(&accumulated_input) {
                                                    Ok(parsed) => parsed,
                                                    Err(_) => {
                                                        // If parsing fails, wrap the string as-is
                                                        Value::String(accumulated_input)
                                                    }
                                                }
                                            };
                                            
                                            finalized_tool_calls.push(ContentBlock::ToolUse { id, name, input });
                                        }
                                    }
                                }
                                "message_delta" => {
                                    if let Some(delta) = event.delta {
                                        if let Some(reason) = delta.stop_reason {
                                            stop_reason = Some(reason);
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

        // Construct response — thinking blocks come first (mirrors API order)
        let mut content_blocks = Vec::new();
        content_blocks.extend(finalized_thinking);
        if !full_text.is_empty() {
            content_blocks.push(ContentBlock::Text { text: full_text });
        }
        content_blocks.extend(finalized_tool_calls);

        let usage = if usage_input_tokens > 0 || usage_output_tokens > 0 {
            Some(Usage {
                input_tokens: usage_input_tokens,
                output_tokens: usage_output_tokens,
                cache_creation_input_tokens: usage_cache_creation,
                cache_read_input_tokens: usage_cache_read,
            })
        } else {
            None
        };

        Ok(AnthropicResponse {
            message_type: "message".to_string(),
            content: content_blocks,
            stop_reason: stop_reason.or(Some("end_turn".to_string())),
            usage,
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

    /// Extract raw content blocks from response (preserves tool_use and thinking blocks)
    pub fn extract_content_blocks(&self, response: &AnthropicResponse) -> Vec<Value> {
        response
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => json!({
                    "type": "text",
                    "text": text
                }),
                ContentBlock::ToolUse { id, name, input } => json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }),
                ContentBlock::Thinking { thinking, signature } => {
                    let mut block = json!({
                        "type": "thinking",
                        "thinking": thinking
                    });
                    if let Some(sig) = signature {
                        block["signature"] = json!(sig);
                    }
                    block
                },
            })
            .collect()
    }
}