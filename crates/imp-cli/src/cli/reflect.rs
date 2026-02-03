use anyhow::Result;
use console::style;

use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};

pub async fn run(date: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config)?;
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

    println!("🧠 Reflecting on {}...\n", target_date);

    let system_prompt = "\
You are a reflective memory system for a personal AI agent. You review a day's \
interaction logs and decide what, if anything, should be persisted to long-term files.

You will be given the day's notes plus the current contents of three files:
- MEMORY.md — long-term memory (facts, preferences, lessons, open threads)
- USER.md — information about the human you serve
- SOUL.md — your identity, personality, values

Your job is to produce a JSON response with this exact structure:

```json
{
  \"summary\": \"A 2-3 sentence summary of what happened today and what you learned.\",
  \"memory_update\": null | \"<full updated MEMORY.md content>\",
  \"user_update\": null | \"<full updated USER.md content>\",
  \"soul_update\": null | \"<full updated SOUL.md content>\"
}
```

Rules:
- summary is ALWAYS required — even if nothing else changes, summarize the day.
- Set a field to null if no meaningful update is needed. Do NOT rewrite a file just to rephrase things.
- MEMORY.md updates: add genuine insights, preferences discovered, decisions made, lessons learned. Remove stale info. Ignore noise (tool counts, timestamps, routine operations).
- USER.md updates: only if you learned something new about the human (new preferences, new context, corrected info). Don't add speculative info.
- SOUL.md updates: only if your identity or values genuinely evolved (very rare). Not for minor style tweaks.
- Be conservative — only update files when there's real new information.
- When updating, return the COMPLETE file content (not a diff).
- Return ONLY the JSON block, no other text."
        .to_string();

    let user_message = format!(
        "## Current MEMORY.md\n\n{}\n\n---\n\n\
         ## Current USER.md\n\n{}\n\n---\n\n\
         ## Current SOUL.md\n\n{}\n\n---\n\n\
         ## Today's Notes ({})\n\n{}\n\n---\n\n\
         Reflect on today's interactions and produce the JSON response.",
        memory_content, user_content, soul_content, target_date, daily_content
    );

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

    if updates == 0 {
        println!(
            "{}",
            style("  No file updates needed — nothing new to persist.").dim()
        );
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
