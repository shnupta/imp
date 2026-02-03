//! Model Context Protocol (MCP) client implementation for Imp.
//!
//! This module provides MCP support for Imp's tool system. MCP servers are configured
//! via TOML files in ~/.imp/mcp/ directory. Each server is spawned as a subprocess
//! and communicates via JSON-RPC over stdin/stdout.
//!
//! Configuration example (~/.imp/mcp/github.toml):
//! ```toml
//! [server]
//! name = "github"
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-github"]
//!
//! [server.env]
//! GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}"
//! ```

use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Configuration for an MCP server loaded from a TOML file.
#[derive(Debug, Deserialize)]
pub struct McpServerConfig {
    pub server: ServerInfo,
}

#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

/// MCP tool schema as returned by the server.
#[derive(Debug, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// JSON-RPC request structure for MCP protocol.
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC response structure for MCP protocol.
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

/// Active MCP server process with communication channels.
pub struct McpServer {
    pub config: McpServerConfig,
    process: Arc<Mutex<Option<Child>>>,
    next_request_id: Arc<Mutex<u64>>,
}

impl McpServer {
    /// Create a new MCP server instance from configuration.
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            process: Arc::new(Mutex::new(None)),
            next_request_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Start the MCP server subprocess if not already running.
    pub async fn ensure_started(&self) -> Result<()> {
        let mut process_guard = self.process.lock().await;
        
        if process_guard.is_none() {
            let mut cmd = Command::new(&self.config.server.command);
            
            if let Some(ref args) = self.config.server.args {
                cmd.args(args);
            }
            
            // Set environment variables, expanding ${VAR} syntax
            if let Some(ref env_vars) = self.config.server.env {
                for (key, value) in env_vars {
                    let expanded_value = expand_env_var(value);
                    cmd.env(key, expanded_value);
                }
            }
            
            let mut child = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| ImpError::Tool(format!(
                    "Failed to spawn MCP server '{}': {}", 
                    self.config.server.name, e
                )))?;
            
            // Initialize the MCP protocol
            self.initialize_protocol(&mut child).await?;
            
            *process_guard = Some(child);
        }
        
        Ok(())
    }
    
    /// Initialize the MCP protocol by sending the initialize request.
    async fn initialize_protocol(&self, child: &mut Child) -> Result<()> {
        let init_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "imp",
                    "version": "0.1.0"
                }
            }),
        };
        
        self.send_request_to_process(child, &init_request).await?;
        let _response = self.read_response_from_process(child).await?;
        
        // TODO: Validate the initialize response
        
        Ok(())
    }
    
    /// Send a JSON-RPC request to the MCP server process.
    async fn send_request_to_process(&self, child: &mut Child, request: &JsonRpcRequest) -> Result<()> {
        let request_json = serde_json::to_string(request)
            .map_err(|e| ImpError::Tool(format!("Failed to serialize MCP request: {}", e)))?;
        
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ImpError::Tool("MCP server stdin not available".to_string()))?;
        
        stdin.write_all(format!("{}\n", request_json).as_bytes()).await
            .map_err(|e| ImpError::Tool(format!("Failed to write to MCP server stdin: {}", e)))?;
        
        stdin.flush().await
            .map_err(|e| ImpError::Tool(format!("Failed to flush MCP server stdin: {}", e)))?;
        
        Ok(())
    }
    
    /// Read a JSON-RPC response from the MCP server process.
    async fn read_response_from_process(&self, child: &mut Child) -> Result<JsonRpcResponse> {
        let stdout = child.stdout.as_mut()
            .ok_or_else(|| ImpError::Tool("MCP server stdout not available".to_string()))?;
        
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        
        reader.read_line(&mut line).await
            .map_err(|e| ImpError::Tool(format!("Failed to read from MCP server stdout: {}", e)))?;
        
        let response: JsonRpcResponse = serde_json::from_str(&line)
            .map_err(|e| ImpError::Tool(format!("Failed to parse MCP response: {}", e)))?;
        
        if let Some(ref error) = response.error {
            return Err(ImpError::Tool(format!("MCP server error: {}", error.message)));
        }
        
        Ok(response)
    }
    
    /// List all available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        self.ensure_started().await?;
        
        let mut request_id = self.next_request_id.lock().await;
        *request_id += 1;
        let id = *request_id;
        drop(request_id);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/list".to_string(),
            params: json!({}),
        };
        
        let mut process_guard = self.process.lock().await;
        let process = process_guard.as_mut()
            .ok_or_else(|| ImpError::Tool("MCP server not running".to_string()))?;
        
        self.send_request_to_process(process, &request).await?;
        let response = self.read_response_from_process(process).await?;
        
        let tools_array = response.result
            .and_then(|r| r.get("tools").cloned())
            .ok_or_else(|| ImpError::Tool("Invalid tools/list response format".to_string()))?;
        
        let tools: Vec<McpTool> = serde_json::from_value(tools_array)
            .map_err(|e| ImpError::Tool(format!("Failed to parse MCP tools: {}", e)))?;
        
        Ok(tools)
    }
    
    /// Call a tool on the MCP server.
    pub async fn call_tool(&self, tool_name: &str, arguments: &Value) -> Result<String> {
        self.ensure_started().await?;
        
        let mut request_id = self.next_request_id.lock().await;
        *request_id += 1;
        let id = *request_id;
        drop(request_id);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: "tools/call".to_string(),
            params: json!({
                "name": tool_name,
                "arguments": arguments
            }),
        };
        
        let mut process_guard = self.process.lock().await;
        let process = process_guard.as_mut()
            .ok_or_else(|| ImpError::Tool("MCP server not running".to_string()))?;
        
        self.send_request_to_process(process, &request).await?;
        let response = self.read_response_from_process(process).await?;
        
        // Extract the result content from MCP tool call response
        let content = response.result
            .and_then(|r| r.get("content").cloned())
            .and_then(|c| {
                // Handle both string content and array of content objects
                match c {
                    Value::String(s) => Some(s),
                    Value::Array(ref arr) => {
                        // MCP often returns array of content objects with text field
                        let text_parts: Vec<String> = arr.iter()
                            .filter_map(|item| {
                                item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                            })
                            .collect();
                        if text_parts.is_empty() {
                            Some(serde_json::to_string_pretty(&c).unwrap_or_default())
                        } else {
                            Some(text_parts.join("\n"))
                        }
                    }
                    _ => Some(serde_json::to_string_pretty(&c).unwrap_or_default())
                }
            })
            .unwrap_or_else(|| "No content returned".to_string());
        
        Ok(content)
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        // Note: This is a sync drop, so we can't await. In practice, the processes
        // will be cleaned up when the program exits. For graceful shutdown, we'd
        // need a separate async cleanup method.
    }
}

/// Registry for managing MCP servers and their tools.
pub struct McpRegistry {
    servers: HashMap<String, Arc<McpServer>>,
    tool_to_server: HashMap<String, String>,
}

impl McpRegistry {
    /// Create a new empty MCP registry.
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            tool_to_server: HashMap::new(),
        }
    }
    
    /// Load MCP servers from configuration directory.
    pub async fn load_from_directory<P: AsRef<Path>>(&mut self, mcp_dir: P) -> Result<()> {
        let mcp_dir = mcp_dir.as_ref();
        
        if !mcp_dir.exists() {
            return Ok(()); // No MCP directory, nothing to load
        }
        
        for entry in fs::read_dir(mcp_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "toml") {
                match self.load_server_config(&path).await {
                    Ok(_) => {
                        // Successfully loaded server
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load MCP server from {:?}: {}", path, e);
                        // Continue loading other servers
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Load a single MCP server configuration file.
    async fn load_server_config<P: AsRef<Path>>(&mut self, config_path: P) -> Result<()> {
        let content = fs::read_to_string(&config_path)?;
        let config: McpServerConfig = toml::from_str(&content)
            .map_err(|e| ImpError::Tool(format!("Failed to parse MCP config: {}", e)))?;
        
        let server_name = config.server.name.clone();
        let server = Arc::new(McpServer::new(config));
        
        // Try to start the server and get its tools
        match server.list_tools().await {
            Ok(tools) => {
                // Register tools with the server mapping
                for tool in tools {
                    self.tool_to_server.insert(tool.name, server_name.clone());
                }
                
                // Register the server
                self.servers.insert(server_name.clone(), server);
                
                println!("Loaded MCP server: {}", server_name);
            }
            Err(e) => {
                eprintln!("Warning: Failed to connect to MCP server '{}': {}", server_name, e);
                // Still register the server in case it starts working later
                self.servers.insert(server_name, server);
            }
        }
        
        Ok(())
    }
    
    /// Get all available tools from all MCP servers.
    pub async fn get_all_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let mut all_tools = Vec::new();
        
        for (server_name, server) in &self.servers {
            match server.list_tools().await {
                Ok(tools) => {
                    for tool in tools {
                        all_tools.push(McpToolDefinition {
                            server_name: server_name.clone(),
                            mcp_tool: tool,
                        });
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to list tools from MCP server '{}': {}", server_name, e);
                    // Continue with other servers
                }
            }
        }
        
        Ok(all_tools)
    }
    
    /// Call a tool on the appropriate MCP server.
    pub async fn call_tool(&self, tool_name: &str, arguments: &Value) -> Result<String> {
        let server_name = self.tool_to_server.get(tool_name)
            .ok_or_else(|| ImpError::Tool(format!("MCP tool '{}' not found", tool_name)))?;
        
        let server = self.servers.get(server_name)
            .ok_or_else(|| ImpError::Tool(format!("MCP server '{}' not found", server_name)))?;
        
        server.call_tool(tool_name, arguments).await
    }
}

/// Wrapper for MCP tool with server information.
pub struct McpToolDefinition {
    pub server_name: String,
    pub mcp_tool: McpTool,
}

/// Convert MCP tool schema to Anthropic tool schema format.
pub fn convert_mcp_to_anthropic_schema(mcp_tool: &McpTool) -> Value {
    json!({
        "name": mcp_tool.name,
        "description": mcp_tool.description,
        "input_schema": mcp_tool.input_schema
    })
}

/// Expand environment variables in a string using ${VAR} syntax.
fn expand_env_var(value: &str) -> String {
    let mut result = value.to_string();
    
    // Simple expansion of ${VAR} patterns
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let env_value = std::env::var(var_name).unwrap_or_default();
            result.replace_range(start..start + end + 1, &env_value);
        } else {
            break; // No closing brace, stop processing
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_var() {
        std::env::set_var("TEST_VAR", "test_value");
        
        assert_eq!(expand_env_var("${TEST_VAR}"), "test_value");
        assert_eq!(expand_env_var("prefix_${TEST_VAR}_suffix"), "prefix_test_value_suffix");
        assert_eq!(expand_env_var("${NONEXISTENT}"), "");
        assert_eq!(expand_env_var("no_vars_here"), "no_vars_here");
    }
}