use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Input;
use std::io::{self, Write};

pub async fn run(resume: bool, session: Option<String>) -> Result<()> {
    let mut agent = Agent::new().await?;

    // Handle --session <id> or --resume (latest)
    if let Some(ref sid) = session {
        agent.resume(sid)?;
        println!(
            "{}",
            style(format!("🔄 Resumed session: {}", &sid[..sid.len().min(8)])).yellow()
        );
    } else if resume {
        let project = agent.project_name().map(|s| s.to_string());
        if let Some(info) = agent.db().get_latest_session(project.as_deref())? {
            agent.resume(&info.id)?;
            println!(
                "{}",
                style(format!(
                    "🔄 Resumed session: {} ({} messages)",
                    &info.id[..info.id.len().min(8)],
                    info.message_count
                ))
                .yellow()
            );
        } else {
            println!("{}", style("No previous session found — starting fresh.").dim());
        }
    }

    println!(
        "{}",
        style(format!("🤖 {} - Interactive Chat", agent.display_name()))
            .bold()
            .blue()
    );
    println!("Type 'quit', 'exit', or Ctrl+C to end the session.");
    println!("{}", style("─".repeat(50)).dim());

    // Print session ID (first 8 chars)
    let short_id = &agent.session_id()[..agent.session_id().len().min(8)];
    println!("{}", style(format!("📎 Session: {}", short_id)).dim());

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
                agent.write_session_summary();
                println!("{}", style(agent.usage().format_session_total()).dim());
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
            "resume" => {
                show_recent_sessions(&agent);
                continue;
            }
            _ => {}
        }

        println!(
            "\n{}",
            style(format!("{}:", agent.display_name())).bold().blue()
        );
        println!("{}", style("─".repeat(20)).dim());

        match agent.process_message_with_markdown(input).await {
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

fn show_recent_sessions(agent: &Agent) {
    match agent.db().list_sessions(10) {
        Ok(sessions) if sessions.is_empty() => {
            println!("{}", style("No previous sessions found.").dim());
        }
        Ok(sessions) => {
            println!("{}", style("Recent sessions:").bold());
            for s in &sessions {
                let short_id = &s.id[..s.id.len().min(8)];
                let project = s.project.as_deref().unwrap_or("-");
                let title = s.title.as_deref().unwrap_or("(untitled)");
                println!(
                    "  {} {} {} ({} msgs)",
                    style(short_id).cyan(),
                    style(project).dim(),
                    title,
                    s.message_count
                );
            }
            println!(
                "{}",
                style("Use `imp chat --session <id>` to resume.").dim()
            );
        }
        Err(e) => {
            println!("{}", style(format!("Error listing sessions: {}", e)).red());
        }
    }
}

fn show_help() {
    println!("{}", style("Available commands:").bold());
    println!("  {} - Exit the chat", style("quit/exit/q").cyan());
    println!(
        "  {} - Clear conversation history",
        style("clear").cyan()
    );
    println!(
        "  {} - List recent sessions",
        style("resume").cyan()
    );
    println!("  {} - Show this help message", style("help").cyan());
    println!("  {} - Ask anything else!", style("<your message>").cyan());
}
