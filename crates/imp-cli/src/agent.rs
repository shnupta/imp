use crate::client::{ClaudeClient, Message};
use crate::compaction;
use crate::config::{imp_home, Config};
use crate::highlight;
use crate::context::ContextManager;
use crate::db::Database;
use crate::error::{ImpError, Result};
use crate::project::{self, ProjectInfo, ProjectRegistry};
use crate::subagent::{SubAgent, SubAgentHandle, SubAgentResult};
use crate::tools::ToolRegistry;
use crate::usage::UsageTracker;
use tracing::warn;
use chrono::Local;
use console::style;
use rustyline::ExternalPrinter as RustylineExternalPrinter;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
// termimad used via highlight module

/// Shared handle to a rustyline ExternalPrinter for output that doesn't garble
/// the readline prompt. When the printer is set, all agent output routes through
/// it; otherwise falls back to regular stdout.
pub type SharedPrinter = Arc<Mutex<Box<dyn RustylineExternalPrinter + Send>>>;

/// When false, `maybe_distill_insights` is a no-op. Users have `imp reflect`
/// for proper distillation; the automatic version was too noisy.
const AUTO_INSIGHTS: bool = false;

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
    /// Handles for spawned sub-agents running as background tokio tasks.
    sub_agents: Vec<SubAgentHandle>,
    /// Shared flag for Ctrl+C interrupt support.
    interrupt_flag: Option<Arc<AtomicBool>>,
    /// External printer for readline-safe output.
    printer: Option<SharedPrinter>,
    /// Knowledge graph for entity/relationship storage and retrieval.
    /// Wrapped in Arc<OnceLock> so it can be initialized in a background thread
    /// without blocking the first chat message.
    knowledge: Arc<OnceLock<crate::knowledge::KnowledgeGraph>>,
}

impl Agent {
    /// Render markdown text to a string for emission through the printer.
    /// Code blocks with language tags get syntax highlighting via syntect.
    fn render_markdown_to_string(text: &str, theme: &str) -> String {
        if text.trim().is_empty() {
            return String::new();
        }
        let highlighted = highlight::highlight_code_blocks(text, theme);
        let skin = termimad::MadSkin::default();
        format!("{}", skin.term_text(&highlighted))
    }

    /// Emit a line of output through the ExternalPrinter (readline-safe) if
    /// available, otherwise fall back to stdout.
    pub fn emit(&self, msg: impl std::fmt::Display) {
        emit_line(&self.printer, msg);
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
        tools.load_from_directory(tools_dir).await?;

        // Open SQLite database and create a new session
        let db = Database::open()?;
        let session_id = db.create_session(project_info.as_ref().map(|p| p.name.as_str()))?;

        let mut usage = UsageTracker::new();
        usage.set_model(&config.llm.model);

        // Disable embeddings if configured, otherwise start loading in background
        if !config.knowledge.embeddings_enabled {
            crate::embeddings::Embedder::disable();
        } else if config.knowledge.enabled {
            crate::embeddings::Embedder::init_background();
        }

        // Open knowledge graph in background thread so it doesn't block first message.
        // CozoDB + RocksDB open + schema check can take a moment on cold start.
        let knowledge = Arc::new(OnceLock::new());
        if config.knowledge.enabled {
            let kg_cell = knowledge.clone();
            std::thread::spawn(move || {
                match crate::knowledge::KnowledgeGraph::open() {
                    Ok(kg) => { let _ = kg_cell.set(kg); }
                    Err(e) => eprintln!("⚠ Knowledge graph unavailable: {e}"),
                }
            });
        }

        Ok(Self {
            client,
            config,
            context,
            tools,
            messages: Vec::new(),
            project: project_info,
            session_start: std::time::Instant::now(),
            total_tool_calls: 0,
            usage,
            db,
            session_id,
            sub_agents: Vec::new(),
            interrupt_flag: None,
            printer: None,
            knowledge,
        })
    }

    pub fn project_name(&self) -> Option<&str> {
        self.project.as_ref().map(|p| p.name.as_str())
    }

    pub fn loaded_sections(&self) -> Vec<&str> {
        self.context.loaded_sections()
    }

    /// The agent's display name, parsed from SOUL.md. Falls back to "Imp".
    pub fn display_name(&self) -> String {
        self.context.agent_name().unwrap_or_else(|| "Imp".to_string())
    }

    /// Set the shared ExternalPrinter for readline-safe output.
    pub fn set_printer(&mut self, printer: SharedPrinter) {
        self.printer = Some(printer);
    }

    pub async fn process_message(&mut self, user_message: &str, stream: bool) -> Result<String> {
        self.process_message_with_options(user_message, stream, false).await
    }

    pub async fn process_message_with_markdown(&mut self, user_message: &str) -> Result<String> {
        self.process_message_with_options(user_message, false, true).await
    }

    async fn process_message_with_options(&mut self, user_message: &str, stream: bool, render_markdown: bool) -> Result<String> {
        // Before processing, check if any sub-agents have completed and enrich the message
        let completed = self.collect_completed_subagents().await;
        let effective_message = if !completed.is_empty() {
            let results_text = completed
                .iter()
                .map(|r| r.format_report())
                .collect::<Vec<_>>()
                .join("\n---\n");
            self.emit(
                style(format!("📬 {} sub-agent(s) completed", completed.len())).yellow()
            );
            format!(
                "[Sub-agent results — {} task(s) completed]\n\n{}\n\n---\n\n{}",
                completed.len(),
                results_text,
                user_message
            )
        } else {
            user_message.to_string()
        };

        self.messages.push(Message::text("user", &effective_message));
        // Persist user message
        if let Err(e) = self.db.save_message(
            &self.session_id,
            "user",
            &serde_json::Value::String(effective_message.clone()),
            0,
        ) {
            self.emit(style(format!("⚠ DB write failed: {}", e)).dim());
        }

        let mut turn_tool_count: usize = 0;

        loop {
            // Check for interrupt before each iteration
            if self.is_interrupted() {
                return Err(ImpError::Agent("interrupted".to_string()));
            }

            let mut system_prompt = self.context.assemble_system_prompt();
            
            // Retrieve and append relevant knowledge from knowledge graph
            // (non-blocking — skipped if background init hasn't finished yet)
            if let Some(kg) = self.knowledge.get() {
                if let Ok(context) = kg.retrieve_context(&effective_message, 5, 5) {
                    if !context.is_empty() {
                        system_prompt.push_str("\n\n---\n\n");
                        system_prompt.push_str(&context);
                    }
                }
            }
            
            let system_tokens = system_prompt.len() / 4;
            let tools = Some(self.tools.get_tool_schemas().await);
            let tool_tokens = tools.as_ref().map_or(0, |t| t.to_string().len() / 4);
            self.messages = compaction::compact_if_needed(&self.messages, system_tokens, tool_tokens);

            // Show thinking indicator for non-streaming mode
            let show_thinking = !stream && self.config.thinking.enabled;
            if show_thinking {
                self.emit(style("💭 Thinking...").dim());
            }

            let response = match self
                .client
                .send_message(self.messages.clone(), Some(system_prompt.clone()), tools.clone(), stream)
                .await
            {
                Ok(r) => r,
                Err(ref e) if Self::is_context_overflow_error(e) => {
                    // Context too long — compact and retry
                    self.emit(style("⚠ Context too long — compacting and retrying...").yellow());
                    self.messages = compaction::compact(&self.messages);
                    let retry_tools = Some(self.tools.get_tool_schemas().await);
                    self.client
                        .send_message(self.messages.clone(), Some(system_prompt), retry_tools, stream)
                        .await?
                }
                Err(e) => return Err(e),
            };

            if show_thinking {
                // No "done" message needed — the response output is the indication
            }

            // Record and display token usage
            if let Some(ref usage) = response.usage {
                self.usage.record(usage.input_tokens, usage.output_tokens);
                self.usage.record_cache(usage.cache_creation_input_tokens, usage.cache_read_input_tokens);
                self.emit(style(UsageTracker::format_response_usage(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_creation_input_tokens,
                    usage.cache_read_input_tokens,
                    Some(&self.config.llm.model),
                )).dim());
            }

            let text_content = self.client.extract_text_content(&response);
            let tool_calls = self.client.extract_tool_calls(&response);

            // CRITICAL: Preserve raw content blocks (text + tool_use) for proper protocol
            let content_blocks = self.client.extract_content_blocks(&response);
            if !content_blocks.is_empty() {
                let assistant_content = json!(content_blocks);
                self.messages.push(Message::with_content("assistant", assistant_content.clone()));
                // Persist assistant message
                if let Err(e) = self.db.save_message(
                    &self.session_id,
                    "assistant",
                    &assistant_content,
                    tool_calls.len(),
                ) {
                    self.emit(style(format!("⚠ DB write failed: {}", e)).dim());
                }
            }

            if tool_calls.is_empty() {
                if render_markdown && !stream {
                    let rendered = Self::render_markdown_to_string(&text_content, &self.config.display.theme);
                    if !rendered.is_empty() {
                        self.emit(rendered.trim_end());
                    }
                }
                // Auto-generate session title after the first exchange
                if self.messages.len() <= 2 {
                    let title = generate_session_title(user_message);
                    if let Err(e) = self.db.update_session_title(&self.session_id, &title) {
                        self.emit(style(format!("⚠ Failed to set session title: {}", e)).dim());
                    }
                }
                // Distill structured insights from this turn
                self.maybe_distill_insights(user_message, &text_content, turn_tool_count);
                return Ok(text_content);
            }

            turn_tool_count += tool_calls.len();

            let mut tool_results = Vec::new();
            for tool_call in tool_calls {
                // Check for interrupt between tool calls
                if self.is_interrupted() {
                    return Err(ImpError::Agent("interrupted".to_string()));
                }

                self.emit(
                    style(format_tool_call(&tool_call.name, &tool_call.input)).dim()
                );

                // Intercept tools that need Agent state (KG, sub-agents)
                let result = match tool_call.name.as_str() {
                    "spawn_agent" => self.handle_spawn_agent(&tool_call),
                    "check_agents" => {
                        let mut r = self.handle_check_agents().await;
                        r.tool_use_id = tool_call.id.clone();
                        r
                    }
                    "store_knowledge" => {
                        let mut r = self.handle_store_knowledge(&tool_call.input);
                        r.tool_use_id = tool_call.id.clone();
                        r
                    }
                    "search_knowledge" => {
                        let mut r = self.handle_search_knowledge(&tool_call.input);
                        r.tool_use_id = tool_call.id.clone();
                        r
                    }
                    _ => {
                        self.tools
                            .execute_tool(&crate::tools::ToolCall {
                                id: tool_call.id.clone(),
                                name: tool_call.name.clone(),
                                arguments: tool_call.input.clone(),
                            })
                            .await?
                    }
                };

                // Convert to proper ToolResult format for Anthropic
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

                if let Some(ref error) = result.error {
                    self.emit(style(format!("❌ Tool error: {}", error)).red());
                } else {
                    self.emit(style("✅ Tool completed successfully").green());
                }
            }

            // CRITICAL: Send tool results as proper tool_result content blocks
            if !tool_results.is_empty() {
                let tool_msg = Message::tool_results(tool_results);
                // Persist tool results
                if let Err(e) = self.db.save_message(
                    &self.session_id,
                    "user",
                    &tool_msg.content,
                    0,
                ) {
                    self.emit(style(format!("⚠ DB write failed: {}", e)).dim());
                }
                self.messages.push(tool_msg);
            }
        }
    }

    pub fn clear_conversation(&mut self) {
        self.messages.clear();
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Elapsed time since session start.
    pub fn session_start_elapsed(&self) -> std::time::Duration {
        self.session_start.elapsed()
    }

    /// Number of messages in the current conversation.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Manually trigger compaction (/compact). Always compacts.
    /// Returns true if compaction was performed.
    pub fn compact_now(&mut self) -> bool {
        let before = self.messages.len();
        if before <= 4 {
            return false; // Nothing meaningful to compact
        }
        self.messages = compaction::compact(&self.messages);
        self.messages.len() < before
    }

    /// Check if an error indicates the context/input is too long for the model.
    fn is_context_overflow_error(error: &ImpError) -> bool {
        let msg = error.to_string().to_lowercase();
        msg.contains("too long")
            || msg.contains("too large")
            || msg.contains("too many tokens")
            || msg.contains("context_length")
            || msg.contains("max_tokens")
            || msg.contains("prompt is too")
            || msg.contains("exceeds the maximum")
            || msg.contains("request too large")
    }

    /// Get a human-readable status string for sub-agents (for /agents command).
    pub async fn check_agents_status(&mut self) -> String {
        let result = self.handle_check_agents().await;
        result.content
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

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Distill structured insights from a conversation turn into the daily memory file.
    /// Only writes if the turn was substantive (had tool calls or a long response).
    /// NOTE: Knowledge extraction happens in `imp reflect`, NOT here.
    /// The agent flags content via the `queue_knowledge` tool during conversation;
    /// reflect processes the queue later with full LLM extraction.
    fn maybe_distill_insights(&mut self, user_message: &str, response_text: &str, tool_count: usize) {
        if tool_count > 0 {
            self.total_tool_calls += tool_count;
        }

        if !AUTO_INSIGHTS {
            return;
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

        if let Err(e) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filepath)
            .and_then(|mut f| f.write_all(entry.as_bytes()))
        {
            warn!(error = %e, "Memory write failed");
        }
    }

    // ── Knowledge graph tools ─────────────────────────────────────────

    /// Handle `store_knowledge` using the agent's existing KG instance
    /// (avoids opening a second RocksDB handle which would fail with a lock error).
    fn handle_store_knowledge(&self, arguments: &serde_json::Value) -> crate::tools::ToolResult {
        let kg = match self.knowledge.get() {
            Some(kg) => kg,
            None => {
                return crate::tools::ToolResult {
                    tool_use_id: String::new(),
                    content: String::new(),
                    error: Some("Knowledge graph not available (still initializing or disabled)".to_string()),
                };
            }
        };

        let mut entities_added = 0usize;
        let mut relationships_added = 0usize;
        let mut chunks_stored = 0usize;

        // Process entities
        if let Some(entities) = arguments.get("entities").and_then(|v| v.as_array()) {
            for entity_val in entities {
                let name = match entity_val.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let entity_type = entity_val
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("concept");
                let properties = entity_val
                    .get("properties")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                // Skip if entity already exists
                if let Ok(Some(_)) = kg.find_entity_by_name(name) {
                    continue;
                }

                let entity = crate::knowledge::Entity {
                    id: String::new(),
                    entity_type: entity_type.to_string(),
                    name: name.to_string(),
                    properties,
                    created_at: 0.0,
                    updated_at: 0.0,
                };

                if kg.store_entity(entity).is_ok() {
                    entities_added += 1;
                }
            }
        }

        // Process relationships
        if let Some(rels) = arguments.get("relationships").and_then(|v| v.as_array()) {
            for rel_val in rels {
                let from_name = match rel_val.get("from").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let to_name = match rel_val.get("to").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let rel_type = match rel_val.get("rel").and_then(|v| v.as_str()) {
                    Some(r) => r,
                    None => continue,
                };

                let from = match kg.find_entity_by_name(from_name) {
                    Ok(Some(e)) => e,
                    _ => continue,
                };
                let to = match kg.find_entity_by_name(to_name) {
                    Ok(Some(e)) => e,
                    _ => continue,
                };

                let relationship = crate::knowledge::Relationship {
                    id: String::new(),
                    from_id: from.id,
                    rel_type: rel_type.to_string(),
                    to_id: to.id,
                    properties: serde_json::Value::Object(serde_json::Map::new()),
                    created_at: 0.0,
                };

                if kg.store_relationship(relationship).is_ok() {
                    relationships_added += 1;
                }
            }
        }

        // Process chunks
        if let Some(chunks) = arguments.get("chunks").and_then(|v| v.as_array()) {
            for chunk_val in chunks {
                let content = match chunk_val.get("content").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => continue,
                };
                let source = chunk_val
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("conversation");

                if kg.store_chunk(content, source, "live").is_ok() {
                    chunks_stored += 1;
                }
            }
        }

        let total = entities_added + relationships_added + chunks_stored;
        let content = if total == 0 {
            "No new knowledge stored (entities may already exist).".to_string()
        } else {
            format!(
                "Stored: {} entities, {} relationships, {} chunks",
                entities_added, relationships_added, chunks_stored
            )
        };

        crate::tools::ToolResult {
            tool_use_id: String::new(),
            content,
            error: None,
        }
    }

    /// Handle `search_knowledge` — explicit knowledge graph lookup.
    fn handle_search_knowledge(&self, arguments: &serde_json::Value) -> crate::tools::ToolResult {
        let kg = match self.knowledge.get() {
            Some(kg) => kg,
            None => {
                return crate::tools::ToolResult {
                    tool_use_id: String::new(),
                    content: String::new(),
                    error: Some("Knowledge graph not available (still initializing or disabled)".to_string()),
                };
            }
        };

        let query = arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let max_results = arguments
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let mut output = String::new();

        // 1. Search memory chunks (semantic or text fallback)
        match kg.search_similar(query, max_results) {
            Ok(chunks) if !chunks.is_empty() => {
                output.push_str(&format!("## Memory Chunks ({} results)\n\n", chunks.len()));
                for (i, chunk) in chunks.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. [{}] {}\n",
                        i + 1,
                        chunk.source_type,
                        chunk.content
                    ));
                }
            }
            Ok(_) => {
                output.push_str("No matching memory chunks found.\n");
            }
            Err(e) => {
                output.push_str(&format!("Chunk search error: {}\n", e));
            }
        }

        // 2. Entity lookup — check if query matches any entity names
        if let Some(entity_name) = arguments.get("entity").and_then(|v| v.as_str()) {
            // Explicit entity lookup
            match kg.find_entity_by_name(entity_name) {
                Ok(Some(entity)) => {
                    output.push_str(&format!(
                        "\n## Entity: {} ({})\n",
                        entity.name, entity.entity_type
                    ));
                    if entity.properties != serde_json::json!(null)
                        && entity.properties != serde_json::json!({})
                    {
                        output.push_str(&format!("Properties: {}\n", entity.properties));
                    }

                    // Get relationships
                    if let Ok(related) = kg.get_related(&entity.name, 1) {
                        if !related.is_empty() {
                            output.push_str("Relationships:\n");
                            for r in &related {
                                let arrow = if r.direction == "->" {
                                    format!("{} → {} → {}", entity.name, r.rel_type, r.entity.name)
                                } else {
                                    format!("{} → {} → {}", r.entity.name, r.rel_type, entity.name)
                                };
                                output.push_str(&format!("  - {} ({})\n", arrow, r.entity.entity_type));
                            }
                        }
                    }
                }
                Ok(None) => {
                    output.push_str(&format!("\nNo entity found matching '{}'.\n", entity_name));
                }
                Err(e) => {
                    output.push_str(&format!("\nEntity lookup error: {}\n", e));
                }
            }
        }

        // 3. Stats summary
        if let Ok(stats) = kg.stats() {
            output.push_str(&format!(
                "\n---\nKnowledge graph: {} entities, {} relationships, {} chunks\n",
                stats.entity_count, stats.relationship_count, stats.chunk_count
            ));
        }

        if output.trim().is_empty() {
            output = "No results found.".to_string();
        }

        crate::tools::ToolResult {
            tool_use_id: String::new(),
            content: output,
            error: None,
        }
    }

    // ── Sub-agent management ─────────────────────────────────────────

    /// Handle the `spawn_agent` tool call: spawn a sub-agent and return immediately.
    fn handle_spawn_agent(&mut self, tool_call: &crate::client::ToolCall) -> crate::tools::ToolResult {
        let task = match tool_call.input.get("task").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                return crate::tools::ToolResult {
                    tool_use_id: tool_call.id.clone(),
                    content: String::new(),
                    error: Some("Missing required 'task' parameter".to_string()),
                };
            }
        };

        let max_tokens = tool_call
            .input
            .get("max_tokens_budget")
            .and_then(|v| v.as_u64());

        let working_dir = tool_call
            .input
            .get("working_directory")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let timeout_secs = tool_call
            .input
            .get("timeout_secs")
            .and_then(|v| v.as_u64());

        let subagent = SubAgent::new(task, working_dir, max_tokens, timeout_secs, self.config.clone());
        let handle = subagent.spawn();

        let id = handle.id;
        let task_preview = if handle.task.chars().count() > 100 {
            let preview: String = handle.task.chars().take(100).collect();
            format!("{}...", preview)
        } else {
            handle.task.clone()
        };

        self.emit(
            style(format!("🚀 Sub-agent #{} spawned", id)).yellow()
        );

        self.sub_agents.push(handle);

        crate::tools::ToolResult {
            tool_use_id: tool_call.id.clone(),
            content: format!(
                "Sub-agent #{} spawned for: {}\n\
                The sub-agent is working in the background. Do NOT call check_agents immediately — \
                it takes time to complete. Return to the user and let them know the task is running. \
                Results will be automatically injected when they're ready (on the user's next message).",
                id, task_preview
            ),
            error: None,
        }
    }

    /// Handle the `check_agents` tool call: report on sub-agent status and collect results.
    async fn handle_check_agents(&mut self) -> crate::tools::ToolResult {
        // Dummy tool_use_id — will be overwritten by caller, but we need a placeholder
        // Actually the caller passes the real id. Let me return content only.
        let completed = self.collect_completed_subagents().await;
        let running_count = self.sub_agents.len();

        let mut output = String::new();

        if completed.is_empty() && running_count == 0 {
            output.push_str("No sub-agents (active or completed).");
        } else {
            if !completed.is_empty() {
                output.push_str(&format!(
                    "=== {} Completed Sub-Agent(s) ===\n\n",
                    completed.len()
                ));
                for result in &completed {
                    output.push_str(&result.format_report());
                    output.push_str("\n---\n");
                }
            }

            if running_count > 0 {
                output.push_str(&format!(
                    "\n=== {} Active Sub-Agent(s) ===\n",
                    running_count
                ));
                for handle in &self.sub_agents {
                    let elapsed = handle.spawned_at.elapsed().as_secs();
                    let task_preview = if handle.task.chars().count() > 80 {
                        let preview: String = handle.task.chars().take(80).collect();
                        format!("{}...", preview)
                    } else {
                        handle.task.clone()
                    };
                    output.push_str(&format!(
                        "  #{} — running for {}s — {}\n",
                        handle.id, elapsed, task_preview
                    ));
                }
                output.push_str(
                    "\nSub-agents are still working. Do NOT call check_agents again — \
                    return to the user. Results will auto-inject on the next message."
                );
            }
        }

        // Note: tool_use_id is set to empty here; the caller in process_message_with_options
        // uses the actual tool_call.id when building the Anthropic result. But since we
        // return from match, let's just set it properly.
        crate::tools::ToolResult {
            // This will be replaced by the anthropic_result conversion which uses result.tool_use_id
            tool_use_id: String::new(),
            content: output,
            error: None,
        }
    }

    /// Collect results from all finished sub-agents, removing them from the tracking list.
    pub async fn collect_completed_subagents(&mut self) -> Vec<SubAgentResult> {
        let mut completed = Vec::new();
        let mut remaining = Vec::new();

        for handle in self.sub_agents.drain(..) {
            if handle.handle.is_finished() {
                match handle.handle.await {
                    Ok(result) => completed.push(result),
                    Err(e) => completed.push(SubAgentResult {
                        id: handle.id,
                        task: handle.task,
                        summary: String::new(),
                        files_changed: Vec::new(),
                        input_tokens_used: 0,
                        output_tokens_used: 0,
                        success: false,
                        error: Some(format!("Sub-agent task panicked: {}", e)),
                    }),
                }
            } else {
                remaining.push(handle);
            }
        }

        self.sub_agents = remaining;
        completed
    }

    /// Whether there are any active (still running) sub-agents.
    pub fn has_active_subagents(&self) -> bool {
        !self.sub_agents.is_empty()
    }

    /// Get IDs of active sub-agents (for notification display).
    pub fn active_subagent_ids(&self) -> Vec<u64> {
        self.sub_agents.iter().map(|h| h.id).collect()
    }

    /// Wait until at least one sub-agent completes. Returns the completed results.
    /// If no sub-agents are active, returns an empty vec immediately.
    pub async fn wait_for_subagent(&mut self) -> Vec<SubAgentResult> {
        if self.sub_agents.is_empty() {
            return Vec::new();
        }

        // Poll every 500ms until something finishes
        loop {
            let any_finished = self.sub_agents.iter().any(|h| h.handle.is_finished());
            if any_finished {
                return self.collect_completed_subagents().await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// Set the interrupt flag (shared with Ctrl+C signal handler).
    pub fn set_interrupt_flag(&mut self, flag: Arc<AtomicBool>) {
        self.interrupt_flag = Some(flag);
    }

    /// Check if an interrupt has been requested.
    fn is_interrupted(&self) -> bool {
        self.interrupt_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Abort all running sub-agents. Called when the chat session ends.
    /// Returns the number of sub-agents that were still running.
    pub fn abort_subagents(&mut self) -> usize {
        let count = self.sub_agents.len();
        for handle in self.sub_agents.drain(..) {
            handle.handle.abort();
        }
        count
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

        if let Err(e) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filepath)
            .and_then(|mut f| f.write_all(entry.as_bytes()))
        {
            warn!(error = %e, "Memory write failed");
        }
    }
}

/// Emit a line of output through the ExternalPrinter (readline-safe) if
/// available, otherwise fall back to stdout. Public so chat.rs can use it too.
pub fn emit_line(printer: &Option<SharedPrinter>, msg: impl std::fmt::Display) {
    let text = format!("{}\n", msg);
    if let Some(ref p) = printer {
        if let Ok(mut guard) = p.lock() {
            let _ = guard.print(text);
            return;
        }
    }
    print!("{}", text);
    let _ = std::io::stdout().flush();
}

/// Generate a session title from the first user message.
/// Truncates to ~60 chars at a word boundary.
fn generate_session_title(user_message: &str) -> String {
    let clean = user_message.trim().replace('\n', " ");
    let char_count = clean.chars().count();
    if char_count <= 60 {
        return clean;
    }
    // Find last word boundary before 60 chars
    let end_byte: usize = clean.char_indices()
        .nth(60)
        .map(|(i, _)| i)
        .unwrap_or(clean.len());
    let slice = &clean[..end_byte];
    match slice.rfind(char::is_whitespace) {
        Some(pos) if pos > 20 => format!("{}…", &slice[..pos]),
        _ => format!("{}…", slice),
    }
}

/// Truncate a string to a maximum character count, appending "..." if truncated.
/// Uses char boundaries to avoid panicking on multi-byte UTF-8.
fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else {
        let end: usize = s.char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
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
                let char_count = s.chars().count();
                if char_count > 100 {
                    // Show a preview of the string, then the total length
                    let preview: String = s.chars().take(60).collect();
                    // Cut at last whitespace for cleaner preview
                    let cut = preview.rfind(char::is_whitespace)
                        .filter(|&pos| pos > 20)
                        .unwrap_or(preview.len());
                    format!("\"{}…\" ({char_count} chars)", &preview[..cut])
                } else {
                    format!("\"{}\"", s)
                }
            }
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(obj) => format!("{{{} keys}}", obj.len()),
            other => {
                let s = other.to_string();
                if s.chars().count() > 100 {
                    let preview: String = s.chars().take(97).collect();
                    format!("{}...", preview)
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
