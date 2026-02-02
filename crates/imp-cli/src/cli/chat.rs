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

    if let Some(name) = agent.project_name() {
        println!("{}", style(format!("📂 Project: {}", name)).dim());
    }

    let sections = agent.loaded_sections();
    if !sections.is_empty() {
        println!(
            "{}",
            style(format!("📚 Context: {}", sections.join(", "))).dim()
        );
    }
    println!();

    loop {
        print!("{} ", style("You:").bold().green());
        io::stdout().flush()?;

        let input: String = match Input::new()
            .with_prompt("")
            .allow_empty(true)
            .interact()
        {
            Ok(input) => input,
            Err(_) => break,
        };

        let input = input.trim();

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
            "help" => {
                show_help();
                continue;
            }
            _ => {}
        }

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
    println!(
        "  {} - Clear conversation history",
        style("clear").cyan()
    );
    println!("  {} - Show this help message", style("help").cyan());
    println!("  {} - Ask anything else!", style("<your message>").cyan());
}
