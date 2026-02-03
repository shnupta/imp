//! Sub-agent infrastructure for parallel task execution.
//!
//! A sub-agent is a lightweight, autonomous agent spawned as a tokio task.
//! It gets its own ClaudeClient, tool set, message history, and database session.
//! Sub-agents run the same agentic loop as the parent but non-interactively,
//! and they cannot spawn further sub-agents (no recursive spawning).

use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};
use crate::db::Database;
use crate::error::Result;
use crate::tools::ToolRegistry;
use crate::usage::UsageTracker;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for unique sub-agent IDs.
static NEXT_SUBAGENT_ID: AtomicU64 = AtomicU64::new(1);

/// Result returned when a sub-agent completes (or fails).
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub id: u64,
    pub task: String,
    pub summary: String,
    pub files_changed: Vec<String>,
    pub input_tokens_used: u64,
    pub output_tokens_used: u64,
    pub success: bool,
    pub error: Option<String>,
}

impl SubAgentResult {
    /// Format this result for display / injection into conversation.
    pub fn format_report(&self) -> String {
        let status = if self.success { "✅ Completed" } else { "❌ Failed" };
        let mut report = format!(
            "Sub-agent #{} — {}\nTask: {}\n",
            self.id, status, self.task
        );
        if !self.summary.is_empty() {
            report.push_str(&format!("Summary: {}\n", self.summary));
        }
        if !self.files_changed.is_empty() {
            report.push_str(&format!("Files changed: {}\n", self.files_changed.join(", ")));
        }
        let total_tokens = self.input_tokens_used + self.output_tokens_used;
        report.push_str(&format!(
            "Tokens used: {} (in: {}, out: {})\n",
            total_tokens, self.input_tokens_used, self.output_tokens_used
        ));
        if let Some(ref err) = self.error {
            report.push_str(&format!("Error: {}\n", err));
        }
        report
    }
}

/// Handle for tracking a spawned sub-agent.
pub struct SubAgentHandle {
    pub id: u64,
    pub task: String,
    pub handle: tokio::task::JoinHandle<SubAgentResult>,
    pub spawned_at: std::time::Instant,
}

/// A sub-agent that runs autonomously to complete a task.
pub struct SubAgent {
    id: u64,
    task: String,
    working_directory: String,
    max_tokens_budget: u64,
    config: Config,
}

impl SubAgent {
    /// Create a new sub-agent. Does not start execution — call `spawn()` for that.
    pub fn new(
        task: String,
        working_directory: Option<String>,
        max_tokens_budget: Option<u64>,
        config: Config,
    ) -> Self {
        let id = NEXT_SUBAGENT_ID.fetch_add(1, Ordering::SeqCst);
        let cwd = working_directory.unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });

        Self {
            id,
            task,
            working_directory: cwd,
            max_tokens_budget: max_tokens_budget.unwrap_or(50_000),
            config,
        }
    }

    /// Spawn this sub-agent as a background tokio task. Returns a handle for tracking.
    pub fn spawn(self) -> SubAgentHandle {
        let id = self.id;
        let task = self.task.clone();

        let handle = tokio::spawn(async move {
            self.run().await
        });

        SubAgentHandle {
            id,
            task,
            handle,
            spawned_at: std::time::Instant::now(),
        }
    }

    /// Entry point for the tokio task. Catches panics/errors and returns a result.
    async fn run(self) -> SubAgentResult {
        let id = self.id;
        let task = self.task.clone();

        match self.run_inner().await {
            Ok(result) => result,
            Err(e) => SubAgentResult {
                id,
                task,
                summary: String::new(),
                files_changed: Vec::new(),
                input_tokens_used: 0,
                output_tokens_used: 0,
                success: false,
                error: Some(format!("Sub-agent error: {}", e)),
            },
        }
    }

    /// The actual agentic loop. Mirrors Agent::process_message_with_options but simplified:
    /// no streaming, no markdown rendering, no insight distillation, hard iteration limit.
    async fn run_inner(self) -> Result<SubAgentResult> {
        // Each sub-agent gets its own client, tools, and database session
        let mut client = ClaudeClient::new(self.config.clone())?;

        let mut tools = ToolRegistry::new();
        tools.load_subagent_builtins();

        let db = Database::open()?;
        let session_id = db.create_session(Some(&format!("subagent-{}", self.id)))?;

        // Load identity and user context so sub-agents share the parent's personality
        let identity_context = if let Ok(home) = imp_home() {
            let mut parts = Vec::new();
            if let Ok(identity) = fs::read_to_string(home.join("IDENTITY.md")) {
                let trimmed = identity.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("# Identity\n\n{}", trimmed));
                }
            }
            if let Ok(user) = fs::read_to_string(home.join("USER.md")) {
                let trimmed = user.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("# About Your Human\n\n{}", trimmed));
                }
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("{}\n\n---\n\n", parts.join("\n\n---\n\n"))
            }
        } else {
            String::new()
        };

        let system_prompt = format!(
            "{identity_context}\
            # Sub-Agent Task\n\n\
            You are a sub-agent spawned to complete a specific task. You share the same identity and \
            values as the parent agent, but work independently on this task.\n\n\
            Your task: {}\n\n\
            Working directory: {}\n\n\
            You have access to file and shell tools. Complete the task and provide a clear summary of what you did.\n\
            Write to files, don't just describe changes. Be thorough but efficient.\n\
            When you are done, provide a final summary starting with 'TASK COMPLETE:' that lists:\n\
            1. What you accomplished\n\
            2. Files you created or modified\n\
            3. Any issues or caveats",
            self.task, self.working_directory
        );

        let mut messages: Vec<Message> = Vec::new();
        messages.push(Message::text("user", &self.task));

        // Persist the initial user message
        let _ = db.save_message(
            &session_id,
            "user",
            &serde_json::Value::String(self.task.clone()),
            0,
        );

        let mut usage = UsageTracker::new();
        let mut files_changed: Vec<String> = Vec::new();
        let mut final_text = String::new();

        let max_iterations = 25;

        for _iteration in 0..max_iterations {
            // Check token budget before each API call
            if usage.total_tokens() >= self.max_tokens_budget {
                final_text = format!(
                    "Sub-agent stopped: token budget exhausted ({}/{} tokens used)",
                    usage.total_tokens(),
                    self.max_tokens_budget
                );
                break;
            }

            let tool_schemas = Some(tools.get_tool_schemas());

            let response = client
                .send_message(
                    messages.clone(),
                    Some(system_prompt.clone()),
                    tool_schemas,
                    false, // no streaming
                )
                .await?;

            // Record token usage
            if let Some(ref resp_usage) = response.usage {
                usage.record(resp_usage.input_tokens, resp_usage.output_tokens);
            }

            let text_content = client.extract_text_content(&response);
            let tool_calls = client.extract_tool_calls(&response);
            let content_blocks = client.extract_content_blocks(&response);

            // Push the assistant's response
            if !content_blocks.is_empty() {
                let assistant_content = json!(content_blocks);
                messages.push(Message::with_content("assistant", assistant_content.clone()));
                let _ = db.save_message(
                    &session_id,
                    "assistant",
                    &assistant_content,
                    tool_calls.len(),
                );
            }

            // No tool calls → agent is done
            if tool_calls.is_empty() {
                final_text = text_content;
                break;
            }

            // Execute each tool call
            let mut tool_results = Vec::new();
            for tool_call in &tool_calls {
                // Track file modifications
                match tool_call.name.as_str() {
                    "file_write" | "file_edit" => {
                        if let Some(path) = tool_call.input.get("path").and_then(|v| v.as_str()) {
                            if !files_changed.contains(&path.to_string()) {
                                files_changed.push(path.to_string());
                            }
                        }
                    }
                    _ => {}
                }

                let result = tools
                    .execute_tool(&crate::tools::ToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.input.clone(),
                    })
                    .await?;

                let anthropic_result = crate::client::ToolResult {
                    tool_use_id: result.tool_use_id,
                    content: if let Some(ref error) = result.error {
                        error.clone()
                    } else {
                        result.content
                    },
                    is_error: result.error.is_some().then_some(true),
                };

                tool_results.push(anthropic_result);
            }

            // Send tool results back for the next iteration
            if !tool_results.is_empty() {
                let tool_msg = Message::tool_results(tool_results);
                let _ = db.save_message(&session_id, "user", &tool_msg.content, 0);
                messages.push(tool_msg);
            }
        }

        // Extract a concise summary from the final text
        let summary = extract_summary(&final_text);

        Ok(SubAgentResult {
            id: self.id,
            task: self.task,
            summary,
            files_changed,
            input_tokens_used: usage.total_input_tokens,
            output_tokens_used: usage.total_output_tokens,
            success: true,
            error: None,
        })
    }
}

/// Extract a summary from the agent's final response.
/// Looks for "TASK COMPLETE:" prefix; falls back to first ~500 chars.
fn extract_summary(text: &str) -> String {
    // Try to find our structured summary marker
    if let Some(idx) = text.find("TASK COMPLETE:") {
        return text[idx..].to_string();
    }

    // Fallback: return the full text, truncated
    if text.len() > 500 {
        format!("{}...", &text[..500])
    } else {
        text.to_string()
    }
}
