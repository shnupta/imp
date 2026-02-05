use anyhow::Result;
use console::style;

use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};
use crate::db::Database;
use crate::extraction::{extract_knowledge_llm, process_extraction, ExtractionStats};
use crate::knowledge::{KnowledgeGraph, read_queue, clear_queue, append_to_queue};

pub async fn run(date: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config.clone())?;
    let home = imp_home()?;

    let target_date =
        date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    println!("🧠 Reflecting on {}...\n", target_date);

    // ══════════════════════════════════════════════════════════════════
    // PHASE 1: PULL CONVERSATIONS FROM DB
    // ══════════════════════════════════════════════════════════════════

    let db = Database::open()?;
    let conversations = db.load_conversations_for_date(&target_date)?;

    let has_conversations = !conversations.is_empty();
    let conversation_text = if has_conversations {
        let total_sessions = conversations.len();
        println!(
            "{}",
            style(format!("📂 Found {} conversation session(s) in database", total_sessions)).dim()
        );

        let mut combined = String::new();
        for (title, text) in &conversations {
            combined.push_str(&format!("### Session: {}\n\n{}\n---\n\n", title, text));
        }
        combined
    } else {
        println!("{}", style("📂 No conversations found in database for this date").dim());
        String::new()
    };

    // ══════════════════════════════════════════════════════════════════
    // PHASE 2: SUMMARIZE CONVERSATIONS → DAILY MEMORY FILE
    // ══════════════════════════════════════════════════════════════════

    let daily_file = home.join("memory").join(format!("{}.md", target_date));
    let existing_daily_content = if daily_file.exists() {
        std::fs::read_to_string(&daily_file)?
    } else {
        String::new()
    };

    // If no daily content and no conversations, nothing to reflect on
    if existing_daily_content.trim().is_empty() && !has_conversations {
        println!("Nothing to reflect on for {} — no memory file and no conversations.", target_date);
        return Ok(());
    }

    // Summarize conversations and rewrite the daily file as a single consolidated document.
    // The LLM sees both the existing notes and full conversations, producing the definitive
    // daily record. This is idempotent — running reflect multiple times yields the same result.
    println!("{}", style("📝 Consolidating daily memory file...").dim());
    {
        let summary_prompt = format!(
            "You are writing the definitive daily memory file for a personal AI agent.\n\n\
            You have two sources:\n\
            1. Existing daily notes (may contain auto-generated session markers, previous reflect output, or manual notes)\n\
            2. Full conversation transcripts from the database\n\n\
            Produce a single, clean, consolidated markdown document that captures EVERYTHING \
            important from the day. This REPLACES the entire daily file.\n\n\
            Structure it as:\n\
            - `# YYYY-MM-DD` header\n\
            - Sections for major topics/sessions\n\
            - Key decisions, accomplishments, technical details\n\
            - Open threads or follow-ups\n\n\
            Rules:\n\
            - Be thorough — this is the only record of the day\n\
            - Be concise — capture substance, skip noise (token counts, tool call counts, timestamps)\n\
            - Preserve any important information from the existing notes\n\
            - Don't invent information not present in the sources\n\n\
            Existing daily notes:\n---\n{}\n---\n\n\
            Today's conversations:\n---\n{}\n---\n\n\
            Write the complete daily memory file now (markdown, no JSON wrapping):",
            if existing_daily_content.is_empty() { "(none)" } else { &existing_daily_content },
            if has_conversations { &conversation_text } else { "(no conversations recorded)" }
        );

        let messages = vec![Message::text("user", &summary_prompt)];
        let response = client
            .send_message_with_options(messages, None, None, false, Some(64_000))
            .await?;
        let daily_summary = client.extract_text_content(&response);

        if let Some(ref usage) = response.usage {
            println!(
                "{}",
                style(format!(
                    "  daily file: {} in / {} out | stop: {}",
                    usage.input_tokens, usage.output_tokens,
                    response.stop_reason.as_deref().unwrap_or("?")
                ))
                .dim()
            );
        }

        let memory_dir = home.join("memory");
        let _ = std::fs::create_dir_all(&memory_dir);
        std::fs::write(&daily_file, &daily_summary)?;
        println!("{}", style("  ✅ Daily memory file rewritten").green());
    }

    // Reload daily content (now the consolidated version)
    let daily_content = if daily_file.exists() {
        std::fs::read_to_string(&daily_file)?
    } else {
        String::new()
    };

    // ══════════════════════════════════════════════════════════════════
    // PHASE 3: REFLECT ON FILES (MEMORY.md, USER.md, SOUL.md, etc.)
    // ══════════════════════════════════════════════════════════════════

    let memory_content =
        std::fs::read_to_string(home.join("MEMORY.md")).unwrap_or_default();
    let user_content =
        std::fs::read_to_string(home.join("USER.md")).unwrap_or_default();
    let soul_content =
        std::fs::read_to_string(home.join("SOUL.md")).unwrap_or_default();

    let stack_content = std::fs::read_to_string(home.join("STACK.md")).ok();
    let arch_content = std::fs::read_to_string(home.join("ARCHITECTURE.md")).ok();
    let principles_content = std::fs::read_to_string(home.join("PRINCIPLES.md")).ok();

    let has_engineering_files = stack_content.is_some() || arch_content.is_some() || principles_content.is_some();

    let engineering_schema = if has_engineering_files {
        "\n  \"stack_update\": null | \"<full updated STACK.md content>\",\
        \n  \"architecture_update\": null | \"<full updated ARCHITECTURE.md content>\",\
        \n  \"principles_update\": null | \"<full updated PRINCIPLES.md content>\","
    } else {
        ""
    };

    let engineering_rules = if has_engineering_files {
        "\n- STACK.md updates: if new technologies, tools, or stack changes were discussed.\
        \n- ARCHITECTURE.md updates: if architectural decisions, patterns, or system design evolved.\
        \n- PRINCIPLES.md updates: if coding principles, conventions, or standards were established or changed."
    } else {
        ""
    };

    let system_prompt = format!("\
You are a reflective memory system for a personal AI agent. You review a day's \
interactions and decide what should be persisted to long-term files.

You will be given:
- The day's notes (including conversation summaries extracted from the database)
- Current contents of the agent's core files

Your job is to produce a JSON response with this exact structure:

```json
{{
  \"summary\": \"A 2-3 sentence summary of what happened today and what you learned.\",
  \"memory_update\": null | \"<full updated MEMORY.md content>\",
  \"user_update\": null | \"<full updated USER.md content>\",
  \"soul_update\": null | \"<full updated SOUL.md content>\",{}
  \"knowledge_entries\": []
}}
```

The `knowledge_entries` array should contain interesting facts, relationships, and concepts \
worth storing in the knowledge graph. Each entry is an object:
```json
{{
  \"content\": \"A self-contained statement of the fact/knowledge\",
  \"entities\": [\"entity_name_1\", \"entity_name_2\"]
}}
```

Rules:
- summary is ALWAYS required — even if nothing else changes, summarize the day.
- Set a field to null if no meaningful update is needed. Do NOT rewrite a file just to rephrase things.
- MEMORY.md updates: add genuine insights, preferences discovered, decisions made, lessons learned. Remove stale info.
- USER.md updates: only if you learned something new about the human. Don't add speculative info.
- SOUL.md updates: only if identity or values genuinely evolved (very rare).{}
- knowledge_entries: extract interesting facts, technical decisions, relationships between concepts/people/projects. \
  Skip routine operations and trivial info. Quality over quantity.
- Be conservative with file updates — only when there's real new information.
- When updating, return the COMPLETE file content (not a diff).
- Return ONLY the JSON block, no other text.",
        engineering_schema,
        engineering_rules
    );

    let mut user_message = format!(
        "## Current MEMORY.md\n\n{}\n\n---\n\n\
         ## Current USER.md\n\n{}\n\n---\n\n\
         ## Current SOUL.md\n\n{}\n\n---\n\n",
        memory_content, user_content, soul_content
    );

    if let Some(ref content) = stack_content {
        user_message.push_str(&format!("## Current STACK.md\n\n{}\n\n---\n\n", content));
    }
    if let Some(ref content) = arch_content {
        user_message.push_str(&format!("## Current ARCHITECTURE.md\n\n{}\n\n---\n\n", content));
    }
    if let Some(ref content) = principles_content {
        user_message.push_str(&format!("## Current PRINCIPLES.md\n\n{}\n\n---\n\n", content));
    }

    user_message.push_str(&format!(
        "## Today's Notes ({})\n\n{}\n\n---\n\n",
        target_date, daily_content
    ));

    user_message.push_str(
        "Reflect on today's interactions and produce the JSON response. \
         Include knowledge_entries for anything worth storing in the knowledge graph."
    );

    println!("{}", style("🔍 Reflecting on files and extracting knowledge...").dim());

    let messages = vec![Message::text("user", &user_message)];
    // Generous budget — thinking + output both need room.
    let response = client
        .send_message_with_options(messages, Some(system_prompt), None, false, Some(64_000))
        .await?;
    let raw_response = client.extract_text_content(&response);

    if let Some(ref usage) = response.usage {
        println!(
            "{}",
            style(format!(
                "  reflect: {} in / {} out | stop: {}",
                usage.input_tokens, usage.output_tokens,
                response.stop_reason.as_deref().unwrap_or("?")
            ))
            .dim()
        );
    }

    // Warn if response was truncated
    if response.stop_reason.as_deref() == Some("max_tokens") {
        eprintln!(
            "{}",
            style("⚠️  Response was truncated (hit max_tokens). Output may be incomplete.").yellow()
        );
    }

    // Parse JSON response
    let json_str = extract_json_block(&raw_response);
    let parsed: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{}\n{}\n\nRaw response:\n{}",
                style("❌ Failed to parse reflection response as JSON").red(),
                e,
                &raw_response[..raw_response.len().min(500)]
            );
            return Ok(());
        }
    };

    // Show summary
    if let Some(summary) = parsed.get("summary").and_then(|v| v.as_str()) {
        println!("\n{}", style("📋 Summary").bold());
        println!("  {}\n", summary);
    }

    let mut updates = 0;

    // Apply file updates
    if let Some(content) = parsed.get("memory_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("MEMORY.md"), content)?;
            println!("{}", style("  ✅ MEMORY.md updated").green());
            updates += 1;
        }
    }

    if let Some(content) = parsed.get("user_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("USER.md"), content)?;
            println!("{}", style("  ✅ USER.md updated").green());
            updates += 1;
        }
    }

    if let Some(content) = parsed.get("soul_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("SOUL.md"), content)?;
            println!("{}", style("  ✅ SOUL.md updated").green());
            updates += 1;
        }
    }

    if stack_content.is_some() {
        if let Some(content) = parsed.get("stack_update").and_then(|v| v.as_str()) {
            let content = content.trim();
            if !content.is_empty() {
                std::fs::write(home.join("STACK.md"), content)?;
                println!("{}", style("  ✅ STACK.md updated").green());
                updates += 1;
            }
        }
    }

    if arch_content.is_some() {
        if let Some(content) = parsed.get("architecture_update").and_then(|v| v.as_str()) {
            let content = content.trim();
            if !content.is_empty() {
                std::fs::write(home.join("ARCHITECTURE.md"), content)?;
                println!("{}", style("  ✅ ARCHITECTURE.md updated").green());
                updates += 1;
            }
        }
    }

    if principles_content.is_some() {
        if let Some(content) = parsed.get("principles_update").and_then(|v| v.as_str()) {
            let content = content.trim();
            if !content.is_empty() {
                std::fs::write(home.join("PRINCIPLES.md"), content)?;
                println!("{}", style("  ✅ PRINCIPLES.md updated").green());
                updates += 1;
            }
        }
    }

    if updates == 0 {
        println!(
            "{}",
            style("  No file updates needed — nothing new to persist.").dim()
        );
    }

    // ══════════════════════════════════════════════════════════════════
    // PHASE 4: QUEUE KNOWLEDGE FROM REFLECTION
    // ══════════════════════════════════════════════════════════════════

    let mut knowledge_queued = 0;
    if let Some(entries) = parsed.get("knowledge_entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let content = match entry.get("content").and_then(|v| v.as_str()) {
                Some(c) if !c.trim().is_empty() => c,
                _ => continue,
            };

            let entities: Vec<String> = entry
                .get("entities")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let session_id = format!("reflect-{}", target_date);
            if let Err(e) = append_to_queue(content, &session_id, entities) {
                eprintln!("⚠️ Failed to queue knowledge entry: {}", e);
            } else {
                knowledge_queued += 1;
            }
        }

        if knowledge_queued > 0 {
            println!(
                "{}",
                style(format!("  ✅ Queued {} knowledge entries for graph processing", knowledge_queued)).green()
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // PHASE 5: PROCESS KNOWLEDGE GRAPH QUEUE
    // ══════════════════════════════════════════════════════════════════

    match process_knowledge_queue(&mut client).await {
        Ok(stats) => {
            if stats.entities_added > 0 || stats.relationships_added > 0 || stats.chunks_stored > 0 {
                println!(
                    "{}",
                    style(format!(
                        "  ✅ Knowledge graph updated ({} entities, {} relationships, {} chunks)",
                        stats.entities_added, stats.relationships_added, stats.chunks_stored
                    ))
                    .green()
                );
            } else {
                println!("{}", style("  Knowledge graph: no new entries to process.").dim());
            }
        }
        Err(e) => {
            eprintln!("⚠️ Knowledge graph processing failed: {}", e);
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // PHASE 6: EMBED DAILY NOTES + CONVERSATION CHUNKS
    // ══════════════════════════════════════════════════════════════════

    if config.knowledge.enabled {
        match KnowledgeGraph::open() {
            Ok(kg) => {
                // Reload daily content (now includes conversation summary)
                let full_daily = if daily_file.exists() {
                    std::fs::read_to_string(&daily_file)?
                } else {
                    daily_content.clone()
                };

                let chunks = chunk_text(&full_daily, 400);
                let mut chunks_stored = 0;

                for chunk_txt in &chunks {
                    if chunk_txt.trim().len() < 50 {
                        continue;
                    }
                    if let Ok(false) = kg.has_similar_chunk(chunk_txt, 0.9) {
                        if let Ok(chunk_id) = kg.store_chunk(chunk_txt, "daily_note", &target_date) {
                            link_chunk_to_entities(&kg, &chunk_id, chunk_txt);
                            chunks_stored += 1;
                        }
                    }
                }

                if chunks_stored > 0 {
                    println!(
                        "{}",
                        style(format!("  ✅ Embedded {} daily note chunks", chunks_stored)).green()
                    );
                }

                // Backfill missing embeddings
                match kg.backfill_embeddings() {
                    Ok((processed, success)) if processed > 0 => {
                        println!(
                            "{}",
                            style(format!("  ✅ Backfilled {}/{} embeddings", success, processed)).green()
                        );
                    }
                    _ => {}
                }

                // Stats
                if let Ok(stats) = kg.stats() {
                    println!("\n{}", style("📊 Knowledge Graph").bold());
                    println!(
                        "  Entities: {}, Relationships: {}, Chunks: {}",
                        stats.entity_count, stats.relationship_count, stats.chunk_count
                    );
                    println!(
                        "  Schema: {} types, {} relationship types",
                        stats.schema_type_count, stats.schema_rel_count
                    );
                }
            }
            Err(e) => {
                eprintln!("⚠️ Could not open knowledge graph for embedding: {}", e);
            }
        }
    }

    println!("\n{}", style("✨ Reflection complete.").bold().green());

    Ok(())
}

/// Process the knowledge queue using LLM extraction.
async fn process_knowledge_queue(client: &mut ClaudeClient) -> Result<ExtractionStats> {
    let queue_entries = read_queue()?;

    if queue_entries.is_empty() {
        return Ok(ExtractionStats {
            entities_added: 0,
            relationships_added: 0,
            chunks_stored: 0,
            new_types_added: 0,
        });
    }

    println!(
        "{}",
        style(format!("  Processing {} knowledge queue entries...", queue_entries.len())).dim()
    );

    let kg = KnowledgeGraph::open()?;
    let schema = kg.get_schema()?;
    let existing_entities = get_entity_names(&kg);

    let mut total_stats = ExtractionStats {
        entities_added: 0,
        relationships_added: 0,
        chunks_stored: 0,
        new_types_added: 0,
    };

    // Batch queue entries to avoid excessive LLM calls — combine related entries
    let batched_content = queue_entries
        .iter()
        .map(|e| e.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    match extract_knowledge_llm(&batched_content, &schema, &existing_entities, client).await {
        Ok(extraction_result) => {
            match process_extraction(&kg, &extraction_result) {
                Ok(stats) => {
                    total_stats.entities_added += stats.entities_added;
                    total_stats.relationships_added += stats.relationships_added;
                    total_stats.chunks_stored += stats.chunks_stored;
                    total_stats.new_types_added += stats.new_types_added;
                }
                Err(e) => {
                    eprintln!("⚠️ Failed to process extraction: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("⚠️ Failed to extract knowledge: {}", e);
        }
    }

    // Clear processed entries
    clear_queue()?;

    Ok(total_stats)
}

/// Get all entity names from the knowledge graph (for dedup context in LLM prompts).
fn get_entity_names(kg: &KnowledgeGraph) -> Vec<String> {
    let params = std::collections::BTreeMap::new();
    match kg.run_query(
        "?[name, entity_type] := *entity{name, entity_type}",
        params,
    ) {
        Ok(result) => {
            result.rows.iter().filter_map(|row| {
                if row.len() >= 2 {
                    let name = match &row[0] {
                        cozo::DataValue::Str(s) => s.to_string(),
                        _ => return None,
                    };
                    let etype = match &row[1] {
                        cozo::DataValue::Str(s) => s.to_string(),
                        _ => return None,
                    };
                    Some(format!("{} ({})", name, etype))
                } else {
                    None
                }
            }).collect()
        }
        Err(_) => Vec::new(),
    }
}

/// Split text into chunks at paragraph boundaries, targeting ~max_chars per chunk.
fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for paragraph in text.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }

        if current.len() + trimmed.len() + 2 > max_chars && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(trimmed);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Link a chunk to any entities whose names appear in the chunk text.
fn link_chunk_to_entities(kg: &KnowledgeGraph, chunk_id: &str, text: &str) {
    let chunk_lower = text.to_lowercase();

    let params = std::collections::BTreeMap::new();
    if let Ok(result) = kg.run_query(
        "?[id, name] := *entity{id, name}",
        params,
    ) {
        for row in &result.rows {
            if row.len() >= 2 {
                let entity_id = match &row[0] {
                    cozo::DataValue::Str(s) => s.to_string(),
                    _ => continue,
                };
                let entity_name = match &row[1] {
                    cozo::DataValue::Str(s) => s.to_string(),
                    _ => continue,
                };

                if chunk_lower.contains(&entity_name.to_lowercase()) {
                    let mut params = std::collections::BTreeMap::new();
                    params.insert("chunk_id".to_string(), cozo::DataValue::Str(chunk_id.into()));
                    params.insert("entity_id".to_string(), cozo::DataValue::Str(entity_id.into()));

                    let _ = kg.run_mutating(
                        r#"?[chunk_id, entity_id] <- [[$chunk_id, $entity_id]]
                        :put chunk_entity { chunk_id, entity_id }"#,
                        params,
                    );
                }
            }
        }
    }
}

/// Extract a JSON object from a response that might be wrapped in ```json fences.
/// Uses brace matching rather than fence detection, since JSON values can contain
/// markdown backticks (e.g. file content updates with code blocks).
fn extract_json_block(text: &str) -> &str {
    let trimmed = text.trim();

    // Find the first { and last } — reliable for JSON objects regardless of fencing.
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return &trimmed[start..=end];
            }
        }
    }

    trimmed
}
