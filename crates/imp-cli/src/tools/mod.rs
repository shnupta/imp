//! Tool system for Imp agents.
//!
//! This module provides a flexible tool registry that supports both built-in tools
//! (implemented in Rust) and custom tools (defined in TOML files). Tools can execute
//! shell commands, read/write files, and perform other system operations.

use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub mod builtin;

#[derive(Debug, Deserialize)]
pub struct ToolDefinition {
    pub tool: ToolMeta,
    pub handler: ToolHandler,
}

#[derive(Debug, Deserialize)]
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, ParameterDef>,
}

#[derive(Debug, Deserialize)]
pub struct ParameterDef {
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToolHandler {
    pub kind: String,
    pub command: Option<String>,
    pub script: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub error: Option<String>,
}

pub struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn load_from_directory<P: AsRef<Path>>(&mut self, tools_dir: P) -> Result<()> {
        let tools_dir = tools_dir.as_ref();
        
        // Load builtin tools
        self.load_builtin_tools();
        
        // Load tools from TOML files if directory exists
        if tools_dir.exists() {
            for entry in fs::read_dir(tools_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_file() && path.extension().map_or(false, |ext| ext == "toml") {
                    let content = fs::read_to_string(&path)?;
                    let tool_def: ToolDefinition = toml::from_str(&content)?;
                    self.tools.insert(tool_def.tool.name.clone(), tool_def);
                }
            }
        }

        Ok(())
    }

    fn load_builtin_tools(&mut self) {
        // Add builtin tools (including agent management tools)
        let builtins = vec![
            self.create_exec_tool(),
            self.create_file_read_tool(),
            self.create_file_write_tool(),
            self.create_file_edit_tool(),
            self.create_search_code_tool(),
            self.create_list_files_tool(),
            self.create_spawn_agent_tool(),
            self.create_check_agents_tool(),
        ];

        for tool in builtins {
            self.tools.insert(tool.tool.name.clone(), tool);
        }
    }

    /// Load only the core builtin tools (no spawn_agent / check_agents).
    /// Used by sub-agents to prevent recursive spawning.
    pub fn load_subagent_builtins(&mut self) {
        let builtins = vec![
            self.create_exec_tool(),
            self.create_file_read_tool(),
            self.create_file_write_tool(),
            self.create_file_edit_tool(),
            self.create_search_code_tool(),
            self.create_list_files_tool(),
        ];

        for tool in builtins {
            self.tools.insert(tool.tool.name.clone(), tool);
        }
    }

    pub fn get_tool_schemas(&self) -> Value {
        let mut schemas = Vec::new();

        for tool_def in self.tools.values() {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for (param_name, param_def) in &tool_def.tool.parameters {
                if param_def.required {
                    required.push(param_name.clone());
                }

                let mut param_schema = serde_json::Map::new();
                param_schema.insert("type".to_string(), Value::String(param_def.param_type.clone()));
                
                if let Some(ref desc) = param_def.description {
                    param_schema.insert("description".to_string(), Value::String(desc.clone()));
                }

                properties.insert(param_name.clone(), Value::Object(param_schema));
            }

            let schema = json!({
                "name": tool_def.tool.name,
                "description": tool_def.tool.description,
                "input_schema": {
                    "type": "object",
                    "properties": properties,
                    "required": required
                }
            });

            schemas.push(schema);
        }

        Value::Array(schemas)
    }

    pub async fn execute_tool(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        let tool_def = self.tools.get(&tool_call.name)
            .ok_or_else(|| ImpError::Tool(format!("Unknown tool: {}", tool_call.name)))?;

        let result = match tool_def.handler.kind.as_str() {
            "builtin" => {
                builtin::execute_builtin(&tool_call.name, &tool_call.arguments).await
            }
            "shell" => {
                if let Some(ref command_template) = tool_def.handler.command {
                    let command = self.render_template(command_template, &tool_call.arguments)?;
                    execute_shell_command(&command).await
                } else {
                    Err(ImpError::Tool("Shell handler missing command".to_string()))
                }
            }
            _ => {
                Err(ImpError::Tool(format!("Unknown handler kind: {}", tool_def.handler.kind)))
            }
        };

        match result {
            Ok(content) => Ok(ToolResult {
                tool_use_id: tool_call.id.clone(),
                content,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                tool_use_id: tool_call.id.clone(),
                content: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }

    fn render_template(&self, template: &str, args: &Value) -> Result<String> {
        let mut result = template.to_string();
        
        if let Value::Object(map) = args {
            for (key, value) in map {
                let placeholder = format!("{{{{{}}}}}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    _ => value.to_string().trim_matches('"').to_string(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }

        Ok(result)
    }
}

async fn execute_shell_command(command: &str) -> Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(stdout.to_string())
    } else {
        Err(ImpError::Tool(format!("Command failed: {}\nStderr: {}", command, stderr)))
    }
}

// Tool definition helpers
impl ToolRegistry {
    fn create_exec_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "exec".to_string(),
                description: "Execute a shell command".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("command".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("The shell command to execute".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_file_read_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "file_read".to_string(),
                description: "Read the contents of a file".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("path".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Path to the file to read".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_file_write_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "file_write".to_string(),
                description: "Write content to a file (creates or overwrites)".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("path".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Path to the file to write".to_string()),
                    });
                    params.insert("content".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Content to write to the file".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_file_edit_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "file_edit".to_string(),
                description: "Edit a file by finding and replacing text".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("path".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Path to the file to edit".to_string()),
                    });
                    params.insert("old_text".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Text to find and replace".to_string()),
                    });
                    params.insert("new_text".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("New text to replace with".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_search_code_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "search_code".to_string(),
                description: "Search for text in code files using ripgrep".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("query".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Text to search for".to_string()),
                    });
                    params.insert("path".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: false,
                        default: Some(Value::String(".".to_string())),
                        description: Some("Directory to search in (default: current directory)".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_list_files_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "list_files".to_string(),
                description: "List files and directories".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("path".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: false,
                        default: Some(Value::String(".".to_string())),
                        description: Some("Directory to list (default: current directory)".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_spawn_agent_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "spawn_agent".to_string(),
                description: "Spawn a sub-agent to work on a task in parallel. The sub-agent gets its own conversation context and tools. Use for tasks that can be done independently while you continue the conversation. The sub-agent runs in the background and results are returned when complete.".to_string(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("task".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: true,
                        default: None,
                        description: Some("Clear, complete description of what the sub-agent should do. Include ALL context needed — sub-agents cannot ask clarifying questions.".to_string()),
                    });
                    params.insert("max_tokens_budget".to_string(), ParameterDef {
                        param_type: "integer".to_string(),
                        required: false,
                        default: Some(Value::Number(serde_json::Number::from(200000))),
                        description: Some("Maximum total token budget (input + output) for this sub-agent. Default: 200000. Scale based on task complexity: ~50k for small edits, ~200k for moderate work, ~500k+ for large codebase exploration or multi-file refactors.".to_string()),
                    });
                    params.insert("working_directory".to_string(), ParameterDef {
                        param_type: "string".to_string(),
                        required: false,
                        default: None,
                        description: Some("Working directory for the sub-agent's shell commands. Defaults to current directory.".to_string()),
                    });
                    params
                },
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }

    fn create_check_agents_tool(&self) -> ToolDefinition {
        ToolDefinition {
            tool: ToolMeta {
                name: "check_agents".to_string(),
                description: "Check on spawned sub-agents. Lists active sub-agents and their status, and returns results from any that have completed.".to_string(),
                parameters: HashMap::new(),
            },
            handler: ToolHandler {
                kind: "builtin".to_string(),
                command: None,
                script: None,
            },
        }
    }
}