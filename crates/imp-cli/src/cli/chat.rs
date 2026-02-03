use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Select;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

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
    println!("Type /help for commands, /quit or Ctrl+D to exit.");
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

    // Set up rustyline editor for input with history
    let mut rl = DefaultEditor::new().expect("Failed to initialize line editor");

    loop {
        // Race: wait for user input OR sub-agent completion
        let input = loop {
            let has_subagents = agent.has_active_subagents();

            if has_subagents {
                // Use spawn_blocking so we can select! against sub-agent completion
                let prompt = format!("{} ", style("You:").bold().green());
                let readline_future = {
                    // Move editor into blocking thread, get it back after
                    let mut editor = std::mem::replace(&mut rl, DefaultEditor::new().unwrap());
                    tokio::task::spawn_blocking(move || {
                        let result = editor.readline(&prompt);
                        (editor, result)
                    })
                };

                tokio::select! {
                    readline_result = readline_future => {
                        let (editor, result) = readline_result.unwrap();
                        rl = editor;
                        match result {
                            Ok(line) => break line,
                            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                                // Can't break outer loop from here, use sentinel
                                break "\x04".to_string(); // EOT sentinel
                            }
                            Err(_) => break "\x04".to_string(),
                        }
                    }
                    completed = agent.wait_for_subagent() => {
                        // Sub-agent finished! Print notification and auto-summarize.
                        let results_text = completed
                            .iter()
                            .map(|r| r.format_report())
                            .collect::<Vec<_>>()
                            .join("\n---\n");

                        eprintln!(
                            "\n{}",
                            style(format!("📬 {} sub-agent(s) completed — generating summary...", completed.len())).yellow()
                        );

                        // Auto-trigger agent response with sub-agent results
                        let synthetic_msg = format!(
                            "[Sub-agent results — {} task(s) completed]\n\n{}\n\n\
                            Summarize what the sub-agent accomplished. Be concise.",
                            completed.len(),
                            results_text
                        );

                        println!(
                            "\n{}",
                            style(format!("{}:", agent.display_name())).bold().blue()
                        );
                        println!("{}", style("─".repeat(20)).dim());

                        match agent.process_message(&synthetic_msg, true).await {
                            Ok(_) => {
                                println!("\n{}", style("─".repeat(50)).dim());
                                println!();
                            }
                            Err(e) => {
                                eprintln!("{}", style(format!("⚠ Failed to summarize: {}", e)).dim());
                            }
                        }

                        // Continue loop — readline future was cancelled, restart it
                        continue;
                    }
                }
            } else {
                // No sub-agents running — simple blocking readline
                let readline = rl.readline(&format!("{} ", style("You:").bold().green()));
                match readline {
                    Ok(line) => break line,
                    Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                        break "\x04".to_string();
                    }
                    Err(_) => break "\x04".to_string(),
                }
            }
        };

        // Handle EOT sentinel (Ctrl+C / Ctrl+D)
        if input == "\x04" {
            break;
        }

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Add to history
        let _ = rl.add_history_entry(input);

        // Command handling (/ prefix and bare words for backward compat)
        match input.to_lowercase().as_str() {
            "/quit" | "/exit" | "/q" | "quit" | "exit" | "bye" | "q" => {
                let aborted = agent.abort_subagents();
                if aborted > 0 {
                    println!(
                        "{}",
                        style(format!("⚠️  Aborted {} running sub-agent(s)", aborted)).yellow()
                    );
                }
                agent.write_session_summary();
                println!("{}", style(agent.usage().format_session_total()).dim());
                println!("👋 Goodbye!");
                break;
            }
            "/clear" | "clear" => {
                agent.clear_conversation();
                println!("🧹 Conversation cleared.");
                continue;
            }
            "/help" | "help" => {
                show_help();
                continue;
            }
            "/compact" => {
                let compacted = agent.compact_now();
                if compacted {
                    println!("{}", style("📦 Conversation compacted.").green());
                } else {
                    println!("{}", style("No compaction needed yet.").dim());
                }
                continue;
            }
            "/session" => {
                show_session_info(&agent);
                continue;
            }
            "/agents" => {
                let status = agent.check_agents_status().await;
                println!("{}", status);
                continue;
            }
            _ => {}
        }

        println!(
            "\n{}",
            style(format!("{}:", agent.display_name())).bold().blue()
        );
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
    println!("{}", style("Commands:").bold());
    println!("  {}  — Exit the chat", style("/quit, /exit, /q").cyan());
    println!("  {}         — Clear conversation history", style("/clear").cyan());
    println!("  {}       — Manually compact conversation", style("/compact").cyan());
    println!("  {}       — Show session info", style("/session").cyan());
    println!("  {}        — Show sub-agent status", style("/agents").cyan());
    println!("  {}          — Show this help", style("/help").cyan());
    println!("  {}  — Ask anything!", style("<your message>").cyan());
}

fn show_session_info(agent: &Agent) {
    let duration_secs = agent.session_start_elapsed().as_secs();
    let mins = duration_secs / 60;
    let secs = duration_secs % 60;

    let short_id = &agent.session_id()[..agent.session_id().len().min(8)];
    println!("{}", style("Session Info:").bold());
    println!("  ID:       {}", short_id);
    if let Some(name) = agent.project_name() {
        println!("  Project:  {}", name);
    }
    println!("  Messages: {}", agent.message_count());
    println!("  Duration: {}m {}s", mins, secs);
    println!("  {}", agent.usage().format_session_total());
}
