use anyhow::Result;
use console::style;

use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};
use crate::extraction::{extract_knowledge_llm, process_extraction, ExtractionStats};
use crate::knowledge::{KnowledgeGraph, read_queue, clear_queue};

pub async fn run(date: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config.clone())?;
    let home = imp_home()?;

    // Determine which daily file to reflect on
    let target_date =
        date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let daily_file = home.join("memory").join(format!("{}.md", target_date));

    if !daily_file.exists() {
        println!("No memory file found for {}", target_date);
        return Ok(());
    }

    let daily_content = std::fs::read_to_string(&daily_file)?;
    if daily_content.trim().is_empty() {
        println!("Memory file for {} is empty — nothing to reflect on.", target_date);
        return Ok(());
    }

    let memory_content =
        std::fs::read_to_string(home.join("MEMORY.md")).unwrap_or_default();
    let user_content =
        std::fs::read_to_string(home.join("USER.md")).unwrap_or_default();
    let soul_content =
        std::fs::read_to_string(home.join("SOUL.md")).unwrap_or_default();

    // Load optional engineering context files
    let stack_content = std::fs::read_to_string(home.join("STACK.md")).ok();
    let arch_content = std::fs::read_to_string(home.join("ARCHITECTURE.md")).ok();
    let principles_content = std::fs::read_to_string(home.join("PRINCIPLES.md")).ok();

    println!("🧠 Reflecting on {}...\n", target_date);

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
interaction logs and decide what, if anything, should be persisted to long-term files.

You will be given the day's notes plus the current contents of the agent's core files:
- MEMORY.md — long-term memory (facts, preferences, lessons, open threads)
- USER.md — information about the human you serve
- SOUL.md — your identity, personality, values{}

Your job is to produce a JSON response with this exact structure:

```json
{{
  \"summary\": \"A 2-3 sentence summary of what happened today and what you learned.\",
  \"memory_update\": null | \"<full updated MEMORY.md content>\",
  \"user_update\": null | \"<full updated USER.md content>\",
  \"soul_update\": null | \"<full updated SOUL.md content>\"{}
}}
```

Rules:
- summary is ALWAYS required — even if nothing else changes, summarize the day.
- Set a field to null if no meaningful update is needed. Do NOT rewrite a file just to rephrase things.
- MEMORY.md updates: add genuine insights, preferences discovered, decisions made, lessons learned. Remove stale info. Ignore noise (tool counts, timestamps, routine operations).
- USER.md updates: only if you learned something new about the human (new preferences, new context, corrected info). Don't add speculative info.
- SOUL.md updates: only if your identity or values genuinely evolved (very rare). Not for minor style tweaks.{}
- Be conservative — only update files when there's real new information.
- When updating, return the COMPLETE file content (not a diff).
- Return ONLY the JSON block, no other text.",
        if has_engineering_files {
            "\n- STACK.md — tech stack, languages, frameworks, tools\n\
             - ARCHITECTURE.md — system architecture, design patterns\n\
             - PRINCIPLES.md — coding principles and conventions"
        } else { "" },
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
        "## Today's Notes ({})\n\n{}\n\n---\n\n\
         Reflect on today's interactions and produce the JSON response.",
        target_date, daily_content
    ));

    let messages = vec![Message::text("user", &user_message)];
    let response = client
        .send_message(messages, Some(system_prompt), None, false)
        .await?;
    let raw_response = client.extract_text_content(&response);

    // Parse the JSON from the response (might be wrapped in ```json blocks)
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

    // Always show the summary
    if let Some(summary) = parsed.get("summary").and_then(|v| v.as_str()) {
        println!("{}", style("📋 Summary").bold());
        println!("  {}\n", summary);
    }

    let mut updates = 0;

    // MEMORY.md
    if let Some(content) = parsed.get("memory_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("MEMORY.md"), content)?;
            println!("{}", style("  ✅ MEMORY.md updated").green());
            updates += 1;
        }
    }

    // USER.md
    if let Some(content) = parsed.get("user_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("USER.md"), content)?;
            println!("{}", style("  ✅ USER.md updated").green());
            updates += 1;
        }
    }

    // SOUL.md
    if let Some(content) = parsed.get("soul_update").and_then(|v| v.as_str()) {
        let content = content.trim();
        if !content.is_empty() {
            std::fs::write(home.join("SOUL.md"), content)?;
            println!("{}", style("  ✅ SOUL.md updated").green());
            updates += 1;
        }
    }

    // STACK.md (only if it already exists)
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

    // ARCHITECTURE.md (only if it already exists)
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

    // PRINCIPLES.md (only if it already exists)
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
    // KNOWLEDGE GRAPH PROCESSING
    // ══════════════════════════════════════════════════════════════════
    
    // Process knowledge queue if it exists
    match process_knowledge_queue().await {
        Ok(stats) => {
            if stats.entities_added > 0 || stats.relationships_added > 0 || stats.chunks_stored > 0 {
                println!(
                    "{}",
                    style(format!("  ✅ Knowledge graph updated ({} entities, {} relationships, {} chunks)", 
                        stats.entities_added, stats.relationships_added, stats.chunks_stored)).green()
                );
            }
        }
        Err(e) => {
            eprintln!("⚠️ Knowledge graph processing failed: {}", e);
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // PHASE 5: EMBED DAILY NOTES
    // ══════════════════════════════════════════════════════════════════

    if config.knowledge.enabled {
        match KnowledgeGraph::open() {
            Ok(kg) => {
                // 1. Chunk and embed daily notes
                let chunks = chunk_text(&daily_content, 400);
                let mut chunks_stored = 0;

                for chunk_txt in &chunks {
                    if chunk_txt.trim().len() < 50 {
                        continue; // Skip tiny fragments
                    }

                    // Check for duplicates before storing
                    if let Ok(false) = kg.has_similar_chunk(chunk_txt, 0.9) {
                        if let Ok(chunk_id) = kg.store_chunk(chunk_txt, "daily_note", &target_date) {
                            // Link chunk to any mentioned entities
                            link_chunk_to_entities(&kg, &chunk_id, chunk_txt);
                            chunks_stored += 1;
                        }
                    }
                }

                if chunks_stored > 0 {
                    println!("{}", style(format!("  ✅ Embedded {} daily note chunks", chunks_stored)).green());
                }

                // 2. Backfill any chunks missing embeddings
                match kg.backfill_embeddings() {
                    Ok((processed, success)) if processed > 0 => {
                        println!("{}", style(format!("  ✅ Backfilled {}/{} embeddings", success, processed)).green());
                    }
                    _ => {}
                }

                // 3. Print stats
                if let Ok(stats) = kg.stats() {
                    println!("\n{}", style("📊 Knowledge Graph").bold());
                    println!("  Entities: {}, Relationships: {}, Chunks: {}",
                        stats.entity_count, stats.relationship_count, stats.chunk_count);
                    println!("  Schema: {} types, {} relationship types",
                        stats.schema_type_count, stats.schema_rel_count);
                }
            }
            Err(e) => {
                eprintln!("⚠️ Could not open knowledge graph for daily embedding: {}", e);
            }
        }
    }

    // Token usage
    if let Some(ref usage) = response.usage {
        let total = usage.input_tokens + usage.output_tokens;
        println!(
            "\n{}",
            style(format!("tokens: {} (in: {}, out: {})", total, usage.input_tokens, usage.output_tokens)).dim()
        );
    }

    Ok(())
}

/// Process the knowledge queue using LLM extraction.
async fn process_knowledge_queue() -> Result<ExtractionStats> {
    // Read pending queue entries
    let queue_entries = read_queue()?;
    
    if queue_entries.is_empty() {
        return Ok(ExtractionStats {
            entities_added: 0,
            relationships_added: 0,
            chunks_stored: 0,
            new_types_added: 0,
        });
    }

    // Open knowledge graph
    let kg = KnowledgeGraph::open()?;
    
    // Get current schema for LLM context
    let schema = kg.get_schema()?;
    
    // Initialize client
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config)?;
    
    let mut total_stats = ExtractionStats {
        entities_added: 0,
        relationships_added: 0,
        chunks_stored: 0,
        new_types_added: 0,
    };

    // Process each queue entry
    for entry in &queue_entries {
        match extract_knowledge_llm(&entry.content, &schema, &mut client).await {
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
                eprintln!("⚠️ Failed to extract knowledge from entry: {}", e);
            }
        }
    }

    // Clear processed entries
    clear_queue()?;

    Ok(total_stats)
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

    // Get all entities
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

/// Extract a JSON block from a response that might be wrapped in ```json fences.
fn extract_json_block(text: &str) -> &str {
    let trimmed = text.trim();
    // Try to find ```json ... ``` block
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7; // skip ```json
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim();
        }
    }
    // Try ``` ... ``` block
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim();
        }
    }
    // Try raw JSON (starts with {)
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}
