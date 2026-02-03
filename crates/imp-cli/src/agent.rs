use crate::client::{ClaudeClient, Message};
use crate::compaction;
use crate::config::{imp_home, Config};
use crate::context::ContextManager;
use crate::db::Database;
use crate::error::{ImpError, Result};
use crate::project::{self, ProjectInfo, ProjectRegistry};
use crate::tools::ToolRegistry;
use crate::usage::UsageTracker;
use chrono::Local;
use console::style;
use serde_json::json;
use std::fs;
use std::io::Write;
use termimad::*;

pub struct Agent {
    client: ClaudeClient,
    config: Config,
    context: ContextManager,
    tools: ToolRegistry,
    messages: Vec<Message>,
    project: Option<ProjectInfo>,
    session_start: std::time::Instant,
    total_tool_calls: usize,
    usage: UsageTracker,
    db: Database,
    session_id: String,
}

impl Agent {
    /// Render markdown text nicely in the terminal
    fn render_markdown(text: &str) {
        if text.trim().is_empty() {
            return;
        }
        
        let skin = MadSkin::default();
        let _ = skin.print_text(text);
    }

    /// Create an agent. Automatically detects the project from cwd and loads
    /// two-layer context (global + per-project).
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;

        let client = ClaudeClient::new(config.clone())?;

        // Detect and auto-register project
        let cwd = std::env::current_dir()?;
        let project_info = project::detect_project(&cwd);
        if let Some(ref info) = project_info {
            let mut registry = ProjectRegistry::load()?;
            registry.register_project(info)?;
        }

        // Load two-layer context
        let context = ContextManager::load(project_info.as_ref())?;

        let mut tools = ToolRegistry::new();
        let tools_dir = crate::config::imp_home()?.join("tools");
        tools.load_from_directory(tools_dir)?;

        // Open SQLite database and create a new session
        let db = Database::open()?;
        let session_id = db.create_session(project_info.as_ref().map(|p| p.name.as_str()))?;

        Ok(Self {
            client,
            config,
            context,
            tools,
            messages: Vec::new(),
            project: project_info,
            session_start: std::time::Instant::now(),
            total_tool_calls: 0,
            usage: UsageTracker::new(),
            db,
            session_id,
        })
    }

    pub fn project_name(&self) -> Option<&str> {
        self.project.as_ref().map(|p| p.name.as_str())
    }

    pub fn loaded_sections(&self) -> Vec<&str> {
        self.context.loaded_sections()
    }

    /// The agent's display name, parsed from IDENTITY.md. Falls back to "Imp".
    pub fn display_name(&self) -> String {
        self.context.agent_name().unwrap_or_else(|| "Imp".to_string())
    }

    pub async fn process_message(&mut self, user_message: &str, stream: bool) -> Result<String> {
        self.process_message_with_options(user_message, stream, false).await
    }

    pub async fn process_message_with_markdown(&mut self, user_message: &str) -> Result<String> {
        self.process_message_with_options(user_message, false, true).await
    }

    async fn process_message_with_options(&mut self, user_message: &str, stream: bool, render_markdown: bool) -> Result<String> {
        self.messages.push(Message::text("user", user_message));
        // Persist user message
        let _ = self.db.save_message(
            &self.session_id,
            "user",
            &serde_json::Value::String(user_message.to_string()),
            0,
        );

        let mut iteration_count = 0;
        let max_iterations = 10;
        let mut turn_tool_count: usize = 0;

        while iteration_count < max_iterations {
            iteration_count += 1;

            let system_prompt = self.context.assemble_system_prompt();
            let system_tokens = system_prompt.len() / 4; // rough estimate
            self.messages = compaction::compact_if_needed(&self.messages, system_tokens);
            let tools = Some(self.tools.get_tool_schemas());

            // Show thinking indicator for non-streaming mode
            let show_thinking = !stream && self.config.thinking.enabled;
            if show_thinking {
                eprint!("{}", style("💭 Thinking...").dim());
                let _ = std::io::stderr().flush();
            }

            let response = self
                .client
                .send_message(self.messages.clone(), Some(system_prompt), tools, stream)
                .await?;

            if show_thinking {
                eprintln!(" {}", style("done").dim());
            }

            // Record and display token usage
            if let Some(ref usage) = response.usage {
                self.usage.record(usage.input_tokens, usage.output_tokens);
                eprintln!("{}", style(UsageTracker::format_response_usage(usage.input_tokens, usage.output_tokens)).dim());
            }

            let text_content = self.client.extract_text_content(&response);
            let tool_calls = self.client.extract_tool_calls(&response);

            // CRITICAL: Preserve raw content blocks (text + tool_use) for proper protocol
            let content_blocks = self.client.extract_content_blocks(&response);
            if !content_blocks.is_empty() {
                let assistant_content = json!(content_blocks);
                self.messages.push(Message::with_content("assistant", assistant_content.clone()));
                // Persist assistant message
                let _ = self.db.save_message(
                    &self.session_id,
                    "assistant",
                    &assistant_content,
                    tool_calls.len(),
                );
            }

            if tool_calls.is_empty() {
                if render_markdown && !stream {
                    Self::render_markdown(&text_content);
                }
                // Distill structured insights from this turn
                self.maybe_distill_insights(user_message, &text_content, turn_tool_count);
                return Ok(text_content);
            }

            turn_tool_count += tool_calls.len();

            let mut tool_results = Vec::new();
            for tool_call in tool_calls {
                println!(
                    "{}",
                    style(format_tool_call(&tool_call.name, &tool_call.input)).dim()
                );

                let result = self
                    .tools
                    .execute_tool(&crate::tools::ToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.input.clone(),
                    })
                    .await?;

                // Convert to proper ToolResult format for Anthropic
                let anthropic_result = crate::client::ToolResult {
                    tool_use_id: result.tool_use_id,
                    content: if let Some(ref error) = result.error {
                        error.clone()
                    } else {
                        result.content
                    },
                    is_error: result.error.is_some().then(|| true),
                };

                tool_results.push(anthropic_result);

                if let Some(ref error) = result.error {
                    println!("{}", style(format!("❌ Tool error: {}", error)).red());
                } else {
                    println!("{}", style("✅ Tool completed successfully").green());
                }
            }

            // CRITICAL: Send tool results as proper tool_result content blocks
            if !tool_results.is_empty() {
                let tool_msg = Message::tool_results(tool_results);
                // Persist tool results
                let _ = self.db.save_message(
                    &self.session_id,
                    "user",
                    &tool_msg.content,
                    0,
                );
                self.messages.push(tool_msg);
            }
        }

        Err(ImpError::Agent(
            "Maximum iteration limit reached".to_string(),
        ))
    }

    pub fn clear_conversation(&mut self) {
        self.messages.clear();
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Resume a previous session: load its messages from the database.
    pub fn resume(&mut self, session_id: &str) -> Result<()> {
        let messages = self.db.load_session_messages(session_id)?;
        self.messages = messages;
        self.session_id = session_id.to_string();
        Ok(())
    }

    /// Access the underlying database (for listing sessions, etc.).
    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn usage(&self) -> &UsageTracker {
        &self.usage
    }

    /// Distill structured insights from a conversation turn into the daily memory file.
    /// Only writes if the turn was substantive (had tool calls or a long response).
    fn maybe_distill_insights(&mut self, user_message: &str, response_text: &str, tool_count: usize) {
        if tool_count > 0 {
            self.total_tool_calls += tool_count;
        }
        // Only distill if the turn was substantive
        if tool_count == 0 && response_text.len() < 200 {
            return;
        }

        let home = match imp_home() {
            Ok(h) => h,
            Err(_) => return,
        };
        let memory_dir = home.join("memory");
        let _ = fs::create_dir_all(&memory_dir);

        let date = Local::now().format("%Y-%m-%d").to_string();
        let time = Local::now().format("%H:%M").to_string();
        let filepath = memory_dir.join(format!("{}.md", date));

        // Write structured context, not noise
        let entry = format!(
            "\n## {} — Interaction\n**User asked:** {}\n**Summary:** {}\n",
            time,
            truncate(user_message, 150),
            truncate(response_text, 300),
        );

        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filepath)
            .and_then(|mut f| f.write_all(entry.as_bytes()));
    }

    /// Write a session summary to the daily memory file. Called when the chat ends.
    pub fn write_session_summary(&self) {
        let home = match imp_home() {
            Ok(h) => h,
            Err(_) => return,
        };
        let memory_dir = home.join("memory");
        let _ = fs::create_dir_all(&memory_dir);

        let date = Local::now().format("%Y-%m-%d").to_string();
        let time = Local::now().format("%H:%M").to_string();
        let filepath = memory_dir.join(format!("{}.md", date));

        let duration = self.session_start.elapsed().as_secs() / 60;

        let entry = format!(
            "\n## {} — Session End\n- Duration: {}m, {} messages, {} tool calls\n",
            time,
            duration,
            self.messages.len(),
            self.total_tool_calls,
        );

        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filepath)
            .and_then(|mut f| f.write_all(entry.as_bytes()));
    }
}

/// Truncate a string to a maximum character length, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Format a tool call with its arguments for display.
/// Keeps output compact: inline for simple calls, summarized for complex ones.
fn format_tool_call(name: &str, input: &serde_json::Value) -> String {
    let args = match input.as_object() {
        Some(map) if !map.is_empty() => map,
        _ => return format!("🔧 {name}"),
    };

    let mut parts: Vec<String> = Vec::new();
    for (key, val) in args {
        let formatted = match val {
            serde_json::Value::String(s) => {
                let len = s.len();
                if len > 100 {
                    format!("({len} chars)")
                } else {
                    format!("\"{}\"", s)
                }
            }
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(obj) => format!("{{{} keys}}", obj.len()),
            other => {
                let s = other.to_string();
                if s.len() > 100 {
                    format!("{}...", &s[..97])
                } else {
                    s
                }
            }
        };
        parts.push(format!("{key}={formatted}"));
    }

    let inline = format!("🔧 {name}: {}", parts.join(" "));
    if inline.len() <= 120 {
        inline
    } else {
        // Truncate to fit: show as many args as fit on one line
        let prefix = format!("🔧 {name}: ");
        let mut out = prefix.clone();
        for (i, part) in parts.iter().enumerate() {
            let sep = if i > 0 { " " } else { "" };
            if out.len() + sep.len() + part.len() > 117 {
                out.push_str("...");
                break;
            }
            out.push_str(sep);
            out.push_str(part);
        }
        out
    }
}
