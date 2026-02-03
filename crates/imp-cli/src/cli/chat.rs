use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Select;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Commands sent from the main loop to the dedicated readline thread.
enum InputCommand {
    Readline(String), // prompt
    AddHistory(String),
    Shutdown,
}

/// Results sent from the readline thread back to the main loop.
enum InputResult {
    Line(String),
    Eof,
    Interrupted,
}

pub async fn run(resume: bool, continue_last: bool, session: Option<String>) -> Result<()> {
    let mut agent = Agent::new().await?;

    // --session <id>: resume a specific session
    if let Some(ref sid) = session {
        agent.resume(sid)?;
        println!(
            "{}",
            style(format!("🔄 Resumed session: {}", &sid[..sid.len().min(8)])).yellow()
        );
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
    } else if resume {
        maybe_show_session_picker(&mut agent)?;
    }

    println!(
        "{}",
        style(format!("🤖 {} - Interactive Chat", agent.display_name()))
            .bold()
            .blue()
    );
    println!("Type /help for commands, Ctrl+C to interrupt, Ctrl+D to exit.");
    println!("{}", style("─".repeat(50)).dim());

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

    // Ctrl+C interrupt flag for agent work
    let interrupted = Arc::new(AtomicBool::new(false));
    agent.set_interrupt_flag(interrupted.clone());

    // ── Dedicated readline thread ────────────────────────────────────
    // The thread owns the editor exclusively. We communicate via channels.
    // This lets us select! between user input and sub-agent completion
    // without ever having two things reading stdin.
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<InputCommand>();
    let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel::<InputResult>();

    std::thread::spawn(move || {
        let mut editor = DefaultEditor::new().expect("Failed to initialize line editor");
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                InputCommand::Readline(prompt) => {
                    let result = match editor.readline(&prompt) {
                        Ok(line) => InputResult::Line(line),
                        Err(ReadlineError::Interrupted) => InputResult::Interrupted,
                        Err(ReadlineError::Eof) => InputResult::Eof,
                        Err(_) => InputResult::Eof,
                    };
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
                InputCommand::AddHistory(entry) => {
                    let _ = editor.add_history_entry(&entry);
                }
                InputCommand::Shutdown => break,
            }
        }
    });

    // ── Main chat loop ───────────────────────────────────────────────
    'outer: loop {
        // Check for completed sub-agents before prompting
        let completed = agent.collect_completed_subagents().await;
        if !completed.is_empty() {
            auto_summarize_subagents(&mut agent, completed).await;
        }

        // Collect multiline input (backslash continuation)
        let mut full_input = String::new();
        let mut first_line = true;

        let input = 'input: loop {
            let prompt = if first_line {
                format!("{} ", style("You:").bold().green())
            } else {
                format!("{}  ", style("...").dim())
            };

            // Request a readline from the dedicated thread
            if cmd_tx.send(InputCommand::Readline(prompt)).is_err() {
                break 'outer; // Thread died
            }

            // Wait for input OR sub-agent completion
            let line = loop {
                let has_subagents = agent.has_active_subagents();

                if has_subagents {
                    tokio::select! {
                        biased;
                        // Prefer user input when both are ready
                        result = result_rx.recv() => {
                            match result {
                                Some(InputResult::Line(line)) => break line,
                                Some(InputResult::Interrupted) => {
                                    if full_input.is_empty() && first_line {
                                        println!("{}", style("(Ctrl+C — type /quit to exit)").dim());
                                        full_input.clear();
                                        break 'input String::new();
                                    } else {
                                        println!("{}", style("(input cancelled)").dim());
                                        full_input.clear();
                                        break 'input String::new();
                                    }
                                }
                                Some(InputResult::Eof) | None => break 'outer,
                            }
                        }
                        completed = agent.wait_for_subagent() => {
                            // Sub-agent finished! The readline thread is undisturbed.
                            // Auto-summarize, then loop back to await readline again.
                            auto_summarize_subagents(&mut agent, completed).await;
                            continue; // Re-enter select!, same readline is still running
                        }
                    }
                } else {
                    // No sub-agents — just await readline normally
                    match result_rx.recv().await {
                        Some(InputResult::Line(line)) => break line,
                        Some(InputResult::Interrupted) => {
                            if full_input.is_empty() && first_line {
                                println!("{}", style("(Ctrl+C — type /quit to exit)").dim());
                                full_input.clear();
                                break 'input String::new();
                            } else {
                                println!("{}", style("(input cancelled)").dim());
                                full_input.clear();
                                break 'input String::new();
                            }
                        }
                        Some(InputResult::Eof) | None => break 'outer,
                    }
                }
            };

            // Multiline: backslash continuation
            if line.ends_with('\\') {
                full_input.push_str(&line[..line.len() - 1]);
                full_input.push('\n');
                first_line = false;
            } else {
                full_input.push_str(&line);
                break 'input full_input;
            }
        };

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Add to history
        let _ = cmd_tx.send(InputCommand::AddHistory(input.to_string()));

        // Command handling
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

        // Clear interrupt flag before agent work
        interrupted.store(false, Ordering::SeqCst);

        // Ctrl+C handler for interrupting agent work
        let int_flag = interrupted.clone();
        let ctrlc_handler = tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                int_flag.store(true, Ordering::SeqCst);
            }
        });

        match agent.process_message_with_markdown(input).await {
            Ok(_) => {
                println!("{}", style("─".repeat(50)).dim());
                println!();
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("interrupted") {
                    println!("\n{}", style("⚡ Interrupted").yellow());
                    println!("{}", style("─".repeat(50)).dim());
                    println!();
                } else {
                    println!("{}", style(format!("❌ Error: {}", e)).red());
                    println!("{}", style("─".repeat(50)).dim());
                    println!();
                }
            }
        }

        ctrlc_handler.abort();
    }

    // Clean shutdown
    let _ = cmd_tx.send(InputCommand::Shutdown);
    Ok(())
}

/// Auto-summarize completed sub-agent results.
async fn auto_summarize_subagents(agent: &mut Agent, completed: Vec<crate::subagent::SubAgentResult>) {
    let results_text = completed
        .iter()
        .map(|r| r.format_report())
        .collect::<Vec<_>>()
        .join("\n---\n");

    eprintln!(
        "\n{}",
        style(format!("📬 {} sub-agent(s) completed", completed.len())).yellow()
    );

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
}

fn maybe_show_session_picker(agent: &mut Agent) -> Result<()> {
    let project_name = match agent.project_name() {
        Some(name) => name.to_string(),
        None => return Ok(()),
    };

    let current_session_id = agent.session_id().to_string();
    let sessions = agent
        .db()
        .list_sessions_for_project(&project_name, &current_session_id, 5)?;

    if sessions.is_empty() {
        return Ok(());
    }

    println!(
        "{}",
        style(format!("📂 Project: {}", project_name)).dim()
    );
    println!("{}", style("📎 New session").dim());
    println!();

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

    let default = items.len() - 1;

    let selection = Select::new()
        .with_prompt("Recent sessions")
        .items(&items)
        .default(default)
        .interact_opt()
        .unwrap_or(Some(default));

    match selection {
        Some(idx) if idx < sessions.len() => {
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
        _ => {}
    }

    Ok(())
}

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
    println!();
    println!("{}", style("Input:").bold());
    println!("  {}      — Multiline input (backslash at end of line)", style("line \\").cyan());
    println!("  {}        — Interrupt current agent action", style("Ctrl+C").cyan());
    println!("  {}        — Exit chat", style("Ctrl+D").cyan());
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
