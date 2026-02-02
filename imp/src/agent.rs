use crate::client::{ClaudeClient, Message};
use crate::config::Config;
use crate::context::ContextManager;
use crate::error::{ImpError, Result};
use crate::tools::ToolRegistry;
use console::style;

pub struct Agent {
    client: ClaudeClient,
    context: ContextManager,
    tools: ToolRegistry,
    messages: Vec<Message>,
}

impl Agent {
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;
        
        let client = ClaudeClient::new(
            config.llm.api_key.clone(),
            Some(config.llm.model.clone()),
        );

        // Set up context directory in current working directory
        let context_dir = std::env::current_dir()?.join("context");
        let mut context = ContextManager::new(context_dir);
        context.load_all()?;

        let mut tools = ToolRegistry::new();
        let tools_dir = std::env::current_dir()?.join("tools");
        tools.load_from_directory(tools_dir)?;

        Ok(Self {
            client,
            context,
            tools,
            messages: Vec::new(),
        })
    }

    pub async fn process_message(&mut self, user_message: &str, stream: bool) -> Result<String> {
        // Add user message to conversation
        self.messages.push(Message {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        let mut iteration_count = 0;
        let max_iterations = 10;

        while iteration_count < max_iterations {
            iteration_count += 1;

            // Get system prompt from context
            let system_prompt = self.context.assemble_system_prompt();
            
            // Get tool schemas
            let tools = Some(self.tools.get_tool_schemas());

            // Send message to Claude
            let response = self.client.send_message(
                self.messages.clone(),
                Some(system_prompt),
                tools,
                stream,
            ).await?;

            // Extract text content
            let text_content = self.client.extract_text_content(&response);
            
            // Check for tool calls
            let tool_calls = self.client.extract_tool_calls(&response);

            // Add assistant's response to messages
            if !text_content.is_empty() || !tool_calls.is_empty() {
                // For now, we'll combine text and tool calls in the content
                // In a more sophisticated implementation, we'd handle this better
                let mut content = text_content.clone();
                
                if !tool_calls.is_empty() {
                    if !content.is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(&format!("I need to use {} tool(s) to help with this.", tool_calls.len()));
                }

                self.messages.push(Message {
                    role: "assistant".to_string(),
                    content,
                });
            }

            // If no tool calls, we're done
            if tool_calls.is_empty() {
                return Ok(text_content);
            }

            // Execute tool calls
            let mut tool_results = Vec::new();
            for tool_call in tool_calls {
                println!("{}", style(format!("🔧 Using tool: {}", tool_call.name)).dim());
                
                let result = self.tools.execute_tool(&crate::tools::ToolCall {
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    arguments: tool_call.input.clone(),
                }).await?;

                tool_results.push(result);

                if let Some(ref error) = tool_results.last().unwrap().error {
                    println!("{}", style(format!("❌ Tool error: {}", error)).red());
                } else {
                    println!("{}", style("✅ Tool completed successfully").green());
                }
            }

            // Add tool results to conversation
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

            // Continue the loop to let Claude respond to tool results
        }

        Err(ImpError::Agent("Maximum iteration limit reached".to_string()))
    }

    pub fn clear_conversation(&mut self) {
        self.messages.clear();
    }

    pub fn get_context_files(&self) -> Vec<&str> {
        self.context.list_files()
    }

    pub fn reload_context(&mut self) -> Result<()> {
        self.context.load_all()?;
        Ok(())
    }

    pub fn reload_tools(&mut self) -> Result<()> {
        let tools_dir = std::env::current_dir()?.join("tools");
        self.tools.load_from_directory(tools_dir)?;
        Ok(())
    }
}