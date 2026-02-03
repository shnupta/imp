use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Select;
use std::io::{self, Write};

pub async fn run(resume: bool, continue_last: bool, session: Option<String>) -> Result<()> {
    let mut agent = Agent::new().await?;

    // --session <id>: resume a specific session
    if let Some(ref sid) = session {
        agent.resume(sid)?;
        println!(
            "{}",
            style(format!("🔄 Resumed session: {}", &sid[..sid.len().min(8)])).yellow()
        );
    // --continue: auto-resume the most recent session for this project
    } else if continue_last {
        let project = agent.project_name().map(|s| s.to_string());
        if let Some(info) = agent.db().get_latest_session(project.as_deref())? {
            agent.resume(&info.id)?;
            println!(
                "{}",
                style(format!(
                    "🔄 Continued session: {} ({} messages)",
                    &info.id[..info.id.len().min(8)],
                    info.message_count
                ))
                .yellow()
            );
        } else {
            println!("{}", style("No previous session found — starting fresh.").dim());
        }
    // --resume: show interactive session picker
    } else if resume {
        maybe_show_session_picker(&mut agent)?;
    }
    // bare `imp chat`: start fresh (no picker)

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

        let mut input_buf = String::new();
        match io::stdin().read_line(&mut input_buf) {
            Ok(0) => break,  // EOF
            Ok(_) => {}
            Err(_) => break,
        }

        let input = input_buf.trim();

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

/// Show an interactive session picker if previous sessions exist for this project.
fn maybe_show_session_picker(agent: &mut Agent) -> Result<()> {
    let project_name = match agent.project_name() {
        Some(name) => name.to_string(),
        None => return Ok(()), // No project detected, skip picker
    };

    let current_session_id = agent.session_id().to_string();
    let sessions = agent
        .db()
        .list_sessions_for_project(&project_name, &current_session_id, 5)?;

    if sessions.is_empty() {
        return Ok(()); // No previous sessions, continue with new
    }

    println!(
        "{}",
        style(format!("📂 Project: {}", project_name)).dim()
    );
    println!("{}", style("📎 New session").dim());
    println!();

    // Build picker items
    let mut items: Vec<String> = Vec::new();
    for s in &sessions {
        let short_id = &s.id[..s.id.len().min(8)];
        let age = format_relative_time(&s.updated_at);
        items.push(format!(
            "🔄 {} — {} messages ({})",
            short_id, s.message_count, age
        ));
    }
    items.push("✨ Start new session".to_string());

    let default = items.len() - 1; // Default to "new session"

    let selection = Select::new()
        .with_prompt("Recent sessions")
        .items(&items)
        .default(default)
        .interact_opt()
        .unwrap_or(Some(default)); // On error, default to new session

    match selection {
        Some(idx) if idx < sessions.len() => {
            // User picked a previous session
            let chosen = &sessions[idx];
            agent.resume(&chosen.id)?;
            println!(
                "{}",
                style(format!(
                    "🔄 Resumed session: {} ({} messages)",
                    &chosen.id[..chosen.id.len().min(8)],
                    chosen.message_count
                ))
                .yellow()
            );
        }
        _ => {
            // New session (or cancelled)
        }
    }

    Ok(())
}

/// Format an RFC3339 timestamp as a human-friendly relative time string.
fn format_relative_time(rfc3339: &str) -> String {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return rfc3339.to_string();
    };
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(ts);

    let minutes = duration.num_minutes();
    if minutes < 1 {
        return "just now".to_string();
    }
    if minutes < 60 {
        return format!("{} min ago", minutes);
    }
    let hours = duration.num_hours();
    if hours < 24 {
        if hours == 1 {
            return "1 hour ago".to_string();
        }
        return format!("{} hours ago", hours);
    }
    let days = duration.num_days();
    if days == 1 {
        return "yesterday".to_string();
    }
    if days < 7 {
        return format!("{} days ago", days);
    }
    format!("{} weeks ago", days / 7)
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
