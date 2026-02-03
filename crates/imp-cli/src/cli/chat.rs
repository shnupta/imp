use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Select;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    println!("Type /help for commands, Ctrl+C to interrupt, Ctrl+D to exit.");
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

    // Ctrl+C interrupt flag — shared between signal handler and agent loop
    let interrupted = Arc::new(AtomicBool::new(false));
    agent.set_interrupt_flag(interrupted.clone());

    // Set up rustyline editor for input with history
    let mut rl = DefaultEditor::new().expect("Failed to initialize line editor");

    // Shared flag: background watcher sets this when a sub-agent completes.
    // Allows us to print a notification to stderr without touching stdin.
    let subagent_done = Arc::new(AtomicBool::new(false));

    loop {
        // Before prompting, check for completed sub-agents and auto-summarize
        let completed = agent.collect_completed_subagents().await;
        if !completed.is_empty() {
            auto_summarize_subagents(&mut agent, completed).await;
        }

        // Start a background sub-agent watcher if any are active.
        // This ONLY prints to stderr — never touches stdin.
        let watcher_handle = if agent.has_active_subagents() {
            let done_flag = subagent_done.clone();
            done_flag.store(false, Ordering::SeqCst);
            Some(tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    // We check a simple flag; the actual handle check happens in the main loop
                    // This is just a timer that fires a notification
                    if !done_flag.load(Ordering::SeqCst) {
                        done_flag.store(true, Ordering::SeqCst);
                        // Print notification to stderr — safe while readline is active.
                        // Rustyline redraws the prompt on the next keypress.
                        eprintln!(
                            "\n{}",
                            style("📬 Sub-agent work may be done — press Enter to see results").yellow()
                        );
                    }
                    break;
                }
            }))
        } else {
            None
        };

        // Multiline input: lines ending with \ continue on the next line
        let mut full_input = String::new();
        let mut first_line = true;
        loop {
            let prompt = if first_line {
                format!("{} ", style("You:").bold().green())
            } else {
                format!("{}  ", style("...").dim())
            };

            // Clear interrupt flag before readline
            interrupted.store(false, Ordering::SeqCst);

            let readline = rl.readline(&prompt);
            match readline {
                Ok(line) => {
                    if line.ends_with('\\') {
                        // Continuation: strip trailing backslash, add newline
                        full_input.push_str(&line[..line.len() - 1]);
                        full_input.push('\n');
                        first_line = false;
                    } else {
                        full_input.push_str(&line);
                        break;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    if full_input.is_empty() && first_line {
                        // Ctrl+C on empty prompt — just show a hint
                        println!("{}", style("(Ctrl+C — type /quit to exit)").dim());
                        full_input.clear();
                        break;
                    } else {
                        // Ctrl+C during multiline — cancel the input
                        println!("{}", style("(input cancelled)").dim());
                        full_input.clear();
                        break;
                    }
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D — exit
                    full_input = "\x04".to_string();
                    break;
                }
                Err(_) => {
                    full_input = "\x04".to_string();
                    break;
                }
            }
        }

        // Kill the background watcher if it's still running
        if let Some(handle) = watcher_handle {
            handle.abort();
        }

        // Handle Ctrl+D
        if full_input == "\x04" {
            break;
        }

        let input = full_input.trim();

        // Empty input: check for sub-agent completions (user pressed Enter after notification)
        if input.is_empty() {
            let completed = agent.collect_completed_subagents().await;
            if !completed.is_empty() {
                auto_summarize_subagents(&mut agent, completed).await;
            }
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

        // Clear interrupt flag before agent work
        interrupted.store(false, Ordering::SeqCst);

        // Set up Ctrl+C handler for interrupting agent work
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

        // Cancel the Ctrl+C handler so it doesn't interfere with readline
        ctrlc_handler.abort();
    }

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
