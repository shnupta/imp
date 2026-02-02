use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Input;
use std::io::{self, Write};

pub async fn run() -> Result<()> {
    println!("{}", style("🤖 Imp - Interactive Chat").bold().blue());
    println!("Type 'quit', 'exit', or Ctrl+C to end the session.");
    println!("{}", style("─".repeat(50)).dim());
    
    let mut agent = Agent::new().await?;
    
    // Show context files being used
    let context_files = agent.get_context_files();
    if !context_files.is_empty() {
        println!("{}", style(format!("📚 Loaded context: {}", context_files.join(", "))).dim());
        println!();
    }

    loop {
        // Get user input
        print!("{} ", style("You:").bold().green());
        io::stdout().flush()?;
        
        let input: String = match Input::new()
            .with_prompt("")
            .allow_empty(true)
            .interact()
        {
            Ok(input) => input,
            Err(_) => break, // User pressed Ctrl+C
        };

        let input = input.trim();
        
        // Check for exit commands
        if input.is_empty() {
            continue;
        }
        
        match input.to_lowercase().as_str() {
            "quit" | "exit" | "bye" | "q" => {
                println!("👋 Goodbye!");
                break;
            }
            "clear" => {
                agent.clear_conversation();
                println!("🧹 Conversation cleared.");
                continue;
            }
            "reload" => {
                agent.reload_context()?;
                agent.reload_tools()?;
                println!("🔄 Context and tools reloaded.");
                continue;
            }
            "help" => {
                show_help();
                continue;
            }
            _ => {}
        }

        // Process the message
        println!("\n{}", style("Imp:").bold().blue());
        println!("{}", style("─".repeat(20)).dim());
        
        match agent.process_message(input, true).await {
            Ok(_) => {
                println!("\n{}", style("─".repeat(50)).dim());
                println!();
            }
            Err(e) => {
                println!("{}", style(format!("❌ Error: {}", e)).red());
                println!("{}", style("─".repeat(50)).dim());
                println!();
            }
        }
    }

    Ok(())
}

fn show_help() {
    println!("{}", style("Available commands:").bold());
    println!("  {} - Exit the chat", style("quit/exit/q").cyan());
    println!("  {} - Clear conversation history", style("clear").cyan());
    println!("  {} - Reload context files and tools", style("reload").cyan());
    println!("  {} - Show this help message", style("help").cyan());
    println!("  {} - Ask anything else!", style("<your message>").cyan());
}