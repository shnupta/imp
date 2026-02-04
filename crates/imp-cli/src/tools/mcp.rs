//! MCP (Model Context Protocol) client for Imp.
//!
//! Supports two transports, auto-detected from config:
//! - **HTTP/SSE**: Remote server at a URL
//! - **Stdio**: Local subprocess (JSON-RPC over stdin/stdout)
//!
//! Configured via `~/.imp/.mcp.json` (Claude-compatible format):
//! ```json
//! {
//!   "mcpServers": {
//!     "github": {
//!       "command": "npx",
//!       "args": ["-y", "@modelcontextprotocol/server-github"],
//!       "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" }
//!     },
//!     "api": {
//!       "type": "http",
//!       "url": "https://mcp.example.com/mcp",
//!       "headers": { "Authorization": "Bearer ${API_KEY}" }
//!     }
//!   }
//! }
//! ```

use crate::config::imp_home;
use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ── MCP config types ─────────────────────────────────────────────────

/// Claude-compatible `.mcp.json` file format.
#[derive(Debug, Deserialize)]
pub struct McpJsonFile {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// Configuration for an MCP server.
/// Transport is auto-detected: `url` → HTTP/SSE, `command` → stdio.
#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    /// Transport type hint (optional). Values: "http", "sse", "stdio".
    /// Auto-detected from url/command if omitted.
    #[serde(rename = "type")]
    pub transport_type: Option<String>,
    /// HTTP/SSE transport: URL of the MCP server
    #[serde(default)]
    pub url: Option<String>,
    /// Stdio transport: command to spawn
    #[serde(default)]
    pub command: Option<String>,
    /// Stdio transport: arguments for the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables (supports ${VAR} and ${VAR:-default} expansion)
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// HTTP headers for HTTP/SSE transport (supports ${VAR} expansion)
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl McpServerConfig {
    /// Whether this is an HTTP/SSE (remote) or stdio (subprocess) server.
    pub fn is_remote(&self) -> bool {
        self.url.is_some()
            || self.transport_type.as_deref() == Some("http")
            || self.transport_type.as_deref() == Some("sse")
    }
}

/// Load MCP server configs from `~/.imp/.mcp.json`.
/// Returns an empty map if the file doesn't exist.
pub fn load_mcp_config() -> Result<HashMap<String, McpServerConfig>> {
    let path = imp_home()?.join(".mcp.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| ImpError::Config(format!("Failed to read .mcp.json: {}", e)))?;

    let file: McpJsonFile = serde_json::from_str(&content)
        .map_err(|e| ImpError::Config(format!("Failed to parse .mcp.json: {}", e)))?;

    Ok(file.mcp_servers)
}

// ── JSON-RPC types ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i32,
    message: String,
}

/// MCP tool schema as returned by a server.
#[derive(Debug, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
}

// ── MCP Server ───────────────────────────────────────────────────────

/// A running MCP server connection (either SSE or stdio).
pub struct McpServer {
    name: String,
    config: McpServerConfig,
    /// Stdio: the running child process
    child: Option<tokio::process::Child>,
    next_id: AtomicU64,
}

impl McpServer {
    pub fn new(name: String, config: McpServerConfig) -> Self {
        Self {
            name,
            config,
            child: None,
            next_id: AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Start the server connection and run the MCP initialize handshake.
    pub async fn start(&mut self) -> Result<()> {
        if self.config.is_remote() {
            // SSE: just do the initialize handshake via HTTP
            self.sse_initialize().await?;
        } else {
            // Stdio: spawn the subprocess, then initialize
            self.stdio_spawn().await?;
            self.stdio_initialize().await?;
        }
        Ok(())
    }

    /// List available tools from the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>> {
        let id = self.next_request_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/list".to_string(),
            params: json!({}),
        };

        let response = self.send_request(&request).await?;
        let tools_value = response
            .result
            .and_then(|r| r.get("tools").cloned())
            .ok_or_else(|| ImpError::Tool(format!("MCP '{}': invalid tools/list response", self.name)))?;

        serde_json::from_value(tools_value)
            .map_err(|e| ImpError::Tool(format!("MCP '{}': failed to parse tools: {}", self.name, e)))
    }

    /// Call a tool on this server.
    pub async fn call_tool(&mut self, tool_name: &str, arguments: &Value) -> Result<String> {
        let id = self.next_request_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/call".to_string(),
            params: json!({ "name": tool_name, "arguments": arguments }),
        };

        let response = self.send_request(&request).await?;
        let content = response
            .result
            .and_then(|r| r.get("content").cloned())
            .map(|c| match c {
                Value::String(s) => s,
                Value::Array(arr) => arr
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()).map(String::from))
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => serde_json::to_string_pretty(&other).unwrap_or_default(),
            })
            .unwrap_or_else(|| "No content returned".to_string());

        Ok(content)
    }

    /// Route a request to the appropriate transport.
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let response = if self.config.is_remote() {
            self.sse_send(request).await?
        } else {
            self.stdio_send(request).await?
        };

        if let Some(ref err) = response.error {
            return Err(ImpError::Tool(format!(
                "MCP '{}' error: {}",
                self.name, err.message
            )));
        }
        Ok(response)
    }

    // ── SSE transport ────────────────────────────────────────────────

    async fn sse_initialize(&mut self) -> Result<()> {
        let id = self.next_request_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "imp", "version": "0.1.0" }
            }),
        };
        self.sse_send(&request).await?;
        Ok(())
    }

    async fn sse_send(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let url = self.config.url.as_ref().ok_or_else(|| {
            ImpError::Tool(format!("MCP '{}': missing url for SSE transport", self.name))
        })?;

        let client = reqwest::Client::new();
        let mut req_builder = client.post(url);

        // Add headers with env var expansion
        for (key, value) in &self.config.headers {
            req_builder = req_builder.header(key, expand_env_var(value));
        }

        let body = serde_json::to_string(request)
            .map_err(|e| ImpError::Tool(format!("MCP '{}': serialize error: {}", self.name, e)))?;

        let resp = req_builder
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| ImpError::Tool(format!("MCP '{}': HTTP error: {}", self.name, e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ImpError::Tool(format!(
                "MCP '{}': HTTP {}: {}",
                self.name, status, body
            )));
        }

        // Parse response — try direct JSON first, then scan SSE lines
        let text = resp.text().await.map_err(|e| {
            ImpError::Tool(format!("MCP '{}': failed to read response: {}", self.name, e))
        })?;

        // Try direct JSON parse (simple HTTP response)
        if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(&text) {
            return Ok(parsed);
        }

        // Try SSE format: look for "data: {...}" lines
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(data) {
                    return Ok(parsed);
                }
            }
        }

        Err(ImpError::Tool(format!(
            "MCP '{}': could not parse response: {}",
            self.name,
            &text[..text.len().min(200)]
        )))
    }

    // ── Stdio transport ──────────────────────────────────────────────

    async fn stdio_spawn(&mut self) -> Result<()> {
        let command = self.config.command.as_ref().ok_or_else(|| {
            ImpError::Tool(format!("MCP '{}': missing command for stdio transport", self.name))
        })?;

        use tokio::process::Command;
        let mut cmd = Command::new(command);
        cmd.args(&self.config.args);

        for (key, value) in &self.config.env {
            cmd.env(key, expand_env_var(value));
        }

        let child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| ImpError::Tool(format!("MCP '{}': failed to spawn: {}", self.name, e)))?;

        self.child = Some(child);
        Ok(())
    }

    async fn stdio_initialize(&mut self) -> Result<()> {
        let id = self.next_request_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "imp", "version": "0.1.0" }
            }),
        };
        self.stdio_send(&request).await?;
        Ok(())
    }

    async fn stdio_send(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let child = self.child.as_mut().ok_or_else(|| {
            ImpError::Tool(format!("MCP '{}': process not running", self.name))
        })?;

        let stdin = child.stdin.as_mut().ok_or_else(|| {
            ImpError::Tool(format!("MCP '{}': stdin not available", self.name))
        })?;

        let body = serde_json::to_string(request)
            .map_err(|e| ImpError::Tool(format!("MCP '{}': serialize error: {}", self.name, e)))?;

        stdin
            .write_all(format!("{}\n", body).as_bytes())
            .await
            .map_err(|e| ImpError::Tool(format!("MCP '{}': write error: {}", self.name, e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| ImpError::Tool(format!("MCP '{}': flush error: {}", self.name, e)))?;

        let stdout = child.stdout.as_mut().ok_or_else(|| {
            ImpError::Tool(format!("MCP '{}': stdout not available", self.name))
        })?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| ImpError::Tool(format!("MCP '{}': read error: {}", self.name, e)))?;

        serde_json::from_str(&line)
            .map_err(|e| ImpError::Tool(format!("MCP '{}': parse error: {}", self.name, e)))
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

// ── MCP Registry ─────────────────────────────────────────────────────

/// Manages all MCP server connections and routes tool calls.
pub struct McpRegistry {
    servers: Vec<McpServer>,
    /// Maps tool name → index into servers vec
    tool_routing: HashMap<String, usize>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
            tool_routing: HashMap::new(),
        }
    }

    /// Initialize MCP servers from config. Starts each server and discovers tools.
    pub async fn load_from_config(
        &mut self,
        mcp_configs: &HashMap<String, McpServerConfig>,
    ) -> Result<()> {
        for (name, config) in mcp_configs {
            // Validate: must have either url or command
            if config.url.is_none() && config.command.is_none() {
                eprintln!(
                    "⚠ MCP '{}': needs either 'url' (SSE) or 'command' (stdio), skipping",
                    name
                );
                continue;
            }

            let mut server = McpServer::new(name.clone(), config.clone());

            match server.start().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("⚠ MCP '{}': failed to start: {}", name, e);
                    continue;
                }
            }

            match server.list_tools().await {
                Ok(tools) => {
                    let server_idx = self.servers.len();
                    let tool_count = tools.len();

                    for tool in &tools {
                        self.tool_routing.insert(tool.name.clone(), server_idx);
                    }

                    self.servers.push(server);
                    eprintln!("✅ MCP '{}': {} tool(s) loaded", name, tool_count);
                }
                Err(e) => {
                    eprintln!("⚠ MCP '{}': failed to list tools: {}", name, e);
                }
            }
        }

        Ok(())
    }

    /// Get Anthropic-formatted tool schemas for all MCP tools.
    pub async fn get_tool_schemas(&mut self) -> Vec<Value> {
        let mut schemas = Vec::new();

        for server in &mut self.servers {
            if let Ok(tools) = server.list_tools().await {
                for tool in tools {
                    schemas.push(json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.input_schema
                    }));
                }
            }
        }

        schemas
    }

    /// Call a tool, routing to the correct MCP server.
    pub async fn call_tool(&mut self, tool_name: &str, arguments: &Value) -> Result<String> {
        let server_idx = self
            .tool_routing
            .get(tool_name)
            .copied()
            .ok_or_else(|| ImpError::Tool(format!("MCP tool '{}' not found", tool_name)))?;

        self.servers[server_idx].call_tool(tool_name, arguments).await
    }

    /// Check if a tool name belongs to an MCP server.
    pub fn has_tool(&self, tool_name: &str) -> bool {
        self.tool_routing.contains_key(tool_name)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Expand `${VAR}` patterns in a string from environment variables.
fn expand_env_var(value: &str) -> String {
    let mut result = value.to_string();
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let env_value = std::env::var(var_name).unwrap_or_default();
            result.replace_range(start..start + end + 1, &env_value);
        } else {
            break;
        }
    }
    result
}
