use crate::agent::Agent;
use crate::error::Result;
use console::style;
use dialoguer::Select;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::collections::VecDeque;
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

/// Build the prompt string, showing queue count when items are pending.
fn make_prompt(queued: usize) -> String {
    if queued > 0 {
        format!(
            "{} {} ",
            style("You:").bold().green(),
            style(format!("[{} queued]", queued)).dim()
        )
    } else {
        format!("{} ", style("You:").bold().green())
    }
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

    // ── Input queue ──────────────────────────────────────────────────
    let mut pending_queue: VecDeque<String> = VecDeque::new();
    let mut multiline_buffer = String::new();
    let mut readline_pending = false;

    // ── Main chat loop ───────────────────────────────────────────────
    'outer: loop {
        // Check for completed sub-agents before prompting
        let completed = agent.collect_completed_subagents().await;
        if !completed.is_empty() {
            auto_summarize_subagents(&mut agent, completed).await;
        }

        // ── Phase 1: Get next input (from queue or readline) ─────────
        let input: String = if let Some(queued) = pending_queue.pop_front() {
            // Process queued input immediately, no prompting
            eprintln!(
                "{}",
                style(format!(
                    "▶ Processing queued input{}",
                    if pending_queue.is_empty() {
                        String::new()
                    } else {
                        format!(" ({} remaining)", pending_queue.len())
                    }
                ))
                .dim()
            );
            queued
        } else {
            // Wait for user input (with sub-agent interleaving)
            'input: loop {
                // Ensure readline is pending
                if !readline_pending {
                    let prompt = if multiline_buffer.is_empty() {
                        make_prompt(pending_queue.len())
                    } else {
                        format!("{}  ", style("...").dim())
                    };
                    if cmd_tx.send(InputCommand::Readline(prompt)).is_err() {
                        break 'outer;
                    }
                    readline_pending = true;
                }

                let has_subagents = agent.has_active_subagents();

                if has_subagents {
                    tokio::select! {
                        biased;
                        result = result_rx.recv() => {
                            readline_pending = false;
                            match result {
                                Some(InputResult::Line(line)) => {
                                    // Handle multiline continuation
                                    if line.ends_with('\\') {
                                        multiline_buffer.push_str(&line[..line.len() - 1]);
                                        multiline_buffer.push('\n');
                                        continue 'input;
                                    }
                                    multiline_buffer.push_str(&line);
                                    let input = std::mem::take(&mut multiline_buffer);
                                    let trimmed = input.trim().to_string();
                                    if trimmed.is_empty() {
                                        continue 'input;
                                    }
                                    let _ = cmd_tx.send(InputCommand::AddHistory(trimmed.clone()));
                                    break 'input trimmed;
                                }
                                Some(InputResult::Interrupted) => {
                                    if !multiline_buffer.is_empty() {
                                        multiline_buffer.clear();
                                        println!("{}", style("(input cancelled)").dim());
                                    } else {
                                        println!("{}", style("(Ctrl+C — type /quit to exit)").dim());
                                    }
                                    continue 'input;
                                }
                                Some(InputResult::Eof) | None => break 'outer,
                            }
                        }
                        completed = agent.wait_for_subagent() => {
                            auto_summarize_subagents(&mut agent, completed).await;
                            continue 'input;
                        }
                    }
                } else {
                    match result_rx.recv().await {
                        Some(InputResult::Line(line)) => {
                            readline_pending = false;
                            if line.ends_with('\\') {
                                multiline_buffer.push_str(&line[..line.len() - 1]);
                                multiline_buffer.push('\n');
                                continue 'input;
                            }
                            multiline_buffer.push_str(&line);
                            let input = std::mem::take(&mut multiline_buffer);
                            let trimmed = input.trim().to_string();
                            if trimmed.is_empty() {
                                continue 'input;
                            }
                            let _ = cmd_tx.send(InputCommand::AddHistory(trimmed.clone()));
                            break 'input trimmed;
                        }
                        Some(InputResult::Interrupted) => {
                            readline_pending = false;
                            if !multiline_buffer.is_empty() {
                                multiline_buffer.clear();
                                println!("{}", style("(input cancelled)").dim());
                            } else {
                                println!("{}", style("(Ctrl+C — type /quit to exit)").dim());
                            }
                            continue 'input;
                        }
                        Some(InputResult::Eof) | None => break 'outer,
                    }
                }
            }
        };

        // ── Phase 2: Handle commands ─────────────────────────────────
        match input.to_lowercase().as_str() {
            "/quit" | "/exit" | "/q" | "quit" | "exit" | "bye" | "q" => {
                let aborted = agent.abort_subagents();
                if aborted > 0 {
                    println!(
                        "{}",
                        style(format!("⚠️  Aborted {} running sub-agent(s)", aborted)).yellow()
                    );
                }
                // Clear any remaining queued inputs
                if !pending_queue.is_empty() {
                    println!(
                        "{}",
                        style(format!("🗑️  Discarded {} queued input(s)", pending_queue.len())).dim()
                    );
                    pending_queue.clear();
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
            "/cancel" => {
                let count = pending_queue.len();
                if count > 0 {
                    pending_queue.clear();
                    println!(
                        "{}",
                        style(format!("🗑️  Cleared {} queued input(s)", count)).yellow()
                    );
                } else {
                    println!("{}", style("No queued inputs to cancel.").dim());
                }
                continue;
            }
            "/queue" => {
                if pending_queue.is_empty() {
                    println!("{}", style("Queue is empty.").dim());
                } else {
                    println!(
                        "{}",
                        style(format!("📋 {} queued input(s):", pending_queue.len())).bold()
                    );
                    for (i, q) in pending_queue.iter().enumerate() {
                        let preview: String = q.chars().take(70).collect();
                        let ellipsis = if q.chars().count() > 70 { "…" } else { "" };
                        println!("  {}. {}{}", i + 1, preview, ellipsis);
                    }
                }
                continue;
            }
            _ => {}
        }

        // ── Phase 3: Process with agent ──────────────────────────────
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

        // Start readline so user can queue inputs during processing
        if !readline_pending {
            let prompt = make_prompt(pending_queue.len());
            if cmd_tx.send(InputCommand::Readline(prompt)).is_ok() {
                readline_pending = true;
            }
        }

        // Process the message, collecting any inputs that arrive during processing
        let mut agent_fut = std::pin::pin!(agent.process_message_with_markdown(&input));

        let result = loop {
            tokio::select! {
                biased;
                res = &mut agent_fut => break res,
                input_res = result_rx.recv(), if readline_pending => {
                    readline_pending = false;
                    match input_res {
                        Some(InputResult::Line(line)) => {
                            let trimmed = line.trim().to_string();
                            if trimmed.eq_ignore_ascii_case("/cancel") {
                                let count = pending_queue.len();
                                if count > 0 {
                                    pending_queue.clear();
                                    eprintln!(
                                        "\n{}",
                                        style(format!("🗑️  Cleared {} queued input(s)", count))
                                            .yellow()
                                    );
                                }
                            } else if trimmed.eq_ignore_ascii_case("/stop")
                                || trimmed.eq_ignore_ascii_case("/interrupt")
                            {
                                interrupted.store(true, Ordering::SeqCst);
                                eprintln!("\n{}", style("⚡ Interrupting…").yellow());
                            } else if !trimmed.is_empty() {
                                pending_queue.push_back(trimmed.clone());
                                let _ = cmd_tx.send(InputCommand::AddHistory(trimmed));
                                eprintln!(
                                    "{}",
                                    style(format!(
                                        "📥 Queued ({} pending)",
                                        pending_queue.len()
                                    ))
                                    .dim()
                                );
                            }
                            // Re-prompt immediately
                            let prompt = make_prompt(pending_queue.len());
                            if cmd_tx.send(InputCommand::Readline(prompt)).is_ok() {
                                readline_pending = true;
                            }
                        }
                        Some(InputResult::Interrupted) => {
                            // Ctrl+C during processing — interrupt the agent
                            interrupted.store(true, Ordering::SeqCst);
                            let prompt = make_prompt(pending_queue.len());
                            if cmd_tx.send(InputCommand::Readline(prompt)).is_ok() {
                                readline_pending = true;
                            }
                        }
                        Some(InputResult::Eof) | None => {
                            // Let agent finish, then exit
                            let res = (&mut agent_fut).await;
                            let _ = res; // discard
                            break 'outer;
                        }
                    }
                }
            }
        };

        ctrlc_handler.abort();

        match result {
            Ok(_) => {
                println!("{}", style("─".repeat(50)).dim());
                println!();
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("interrupted") {
                    println!("\n{}", style("⚡ Interrupted").yellow());
                } else {
                    println!("{}", style(format!("❌ Error: {}", e)).red());
                }
                println!("{}", style("─".repeat(50)).dim());
                println!();
            }
        }
    }

    // Clean shutdown
    let _ = cmd_tx.send(InputCommand::Shutdown);
    Ok(())
}

/// Auto-summarize completed sub-agent results with markdown rendering.
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

    match agent.process_message_with_markdown(&synthetic_msg).await {
        Ok(_) => {
            println!("{}", style("─".repeat(50)).dim());
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
        let display_name = s
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(short_id);
        items.push(format!(
            "🔄 {} — {} msgs ({})",
            display_name, s.message_count, age
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
    println!("  {}         — Show queued inputs", style("/queue").cyan());
    println!("  {}        — Cancel all queued inputs", style("/cancel").cyan());
    println!("  {}          — Show this help", style("/help").cyan());
    println!();
    println!("{}", style("Input:").bold());
    println!("  {}      — Multiline input (backslash at end of line)", style("line \\").cyan());
    println!("  {}        — Interrupt current agent action", style("Ctrl+C").cyan());
    println!("  {}        — Exit chat", style("Ctrl+D").cyan());
    println!();
    println!("{}", style("Input Queue:").bold());
    println!("  While the agent is working, you can type more messages.");
    println!("  They'll be queued and processed in order when the agent is free.");
    println!("  Use {} or {} during processing to interrupt.", style("/stop").cyan(), style("Ctrl+C").cyan());
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
