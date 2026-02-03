use crate::client::{ClaudeClient, Message};
use crate::config::Config;
use crate::context::ContextManager;
use crate::error::{ImpError, Result};
use crate::project::{self, ProjectInfo, ProjectRegistry};
use crate::tools::ToolRegistry;
use console::style;
use termimad::*;

pub struct Agent {
    client: ClaudeClient,
    context: ContextManager,
    tools: ToolRegistry,
    messages: Vec<Message>,
    project: Option<ProjectInfo>,
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

        Ok(Self {
            client,
            context,
            tools,
            messages: Vec::new(),
            project: project_info,
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
        self.messages.push(Message {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        let mut iteration_count = 0;
        let max_iterations = 10;

        while iteration_count < max_iterations {
            iteration_count += 1;

            let system_prompt = self.context.assemble_system_prompt();
            let tools = Some(self.tools.get_tool_schemas());

            let response = self
                .client
                .send_message(self.messages.clone(), Some(system_prompt), tools, stream)
                .await?;

            let text_content = self.client.extract_text_content(&response);
            let tool_calls = self.client.extract_tool_calls(&response);

            if !text_content.is_empty() || !tool_calls.is_empty() {
                let mut content = text_content.clone();
                if !tool_calls.is_empty() {
                    if !content.is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(&format!(
                        "I need to use {} tool(s) to help with this.",
                        tool_calls.len()
                    ));
                }

                self.messages.push(Message {
                    role: "assistant".to_string(),
                    content,
                });
            }

            if tool_calls.is_empty() {
                if render_markdown && !stream {
                    Self::render_markdown(&text_content);
                }
                return Ok(text_content);
            }

            let mut tool_results = Vec::new();
            for tool_call in tool_calls {
                println!(
                    "{}",
                    style(format!("🔧 Using tool: {}", tool_call.name)).dim()
                );

                let result = self
                    .tools
                    .execute_tool(&crate::tools::ToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.input.clone(),
                    })
                    .await?;

                tool_results.push(result);

                if let Some(ref error) = tool_results.last().unwrap().error {
                    println!("{}", style(format!("❌ Tool error: {}", error)).red());
                } else {
                    println!("{}", style("✅ Tool completed successfully").green());
                }
            }

            for tool_result in tool_results {
                let result_content = if let Some(ref error) = tool_result.error {
                    format!("Error: {}", error)
                } else {
                    tool_result.content
                };

                self.messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result: {}", result_content),
                });
            }
        }

        Err(ImpError::Agent(
            "Maximum iteration limit reached".to_string(),
        ))
    }

    pub fn clear_conversation(&mut self) {
        self.messages.clear();
    }
}
