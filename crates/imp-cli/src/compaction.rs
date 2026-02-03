use crate::client::Message;
use serde_json::Value;

const CHARS_PER_TOKEN: usize = 4;
const CONTEXT_LIMIT_TOKENS: usize = 200_000;
const COMPACTION_THRESHOLD: f64 = 0.75; // Trigger at 75% of limit
const KEEP_RECENT_MESSAGES: usize = 10; // Always keep last 10 messages verbatim

/// Estimate token count for a message
pub fn estimate_tokens(message: &Message) -> usize {
    let content_len = match &message.content {
        Value::String(s) => s.len(),
        Value::Array(blocks) => {
            // Sum up text content from all blocks
            blocks.iter().map(|b| {
                if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
                    text.len()
                } else if let Some(content) = b.get("content").and_then(|c| c.as_str()) {
                    content.len()
                } else {
                    // tool_use blocks etc — estimate from JSON size
                    b.to_string().len()
                }
            }).sum()
        }
        other => other.to_string().len(),
    };
    content_len / CHARS_PER_TOKEN
}

/// Estimate total tokens for all messages
pub fn estimate_total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_tokens).sum()
}

/// Check if compaction is needed
pub fn needs_compaction(messages: &[Message], system_prompt_tokens: usize) -> bool {
    let total = estimate_total_tokens(messages) + system_prompt_tokens;
    total as f64 > (CONTEXT_LIMIT_TOKENS as f64 * COMPACTION_THRESHOLD)
}

/// Create a compaction summary from messages
/// Returns a summary message that replaces the old messages
pub fn create_summary_message(messages_to_summarize: &[Message]) -> String {
    // Build a condensed summary of the conversation so far
    let mut summary_parts = Vec::new();

    for msg in messages_to_summarize {
        let role = &msg.role;
        let content_preview = match &msg.content {
            Value::String(s) => {
                if s.len() > 200 { format!("{}...", &s[..200]) } else { s.clone() }
            }
            Value::Array(blocks) => {
                let mut texts = Vec::new();
                for block in blocks {
                    if let Some(t) = block.get("type").and_then(|t| t.as_str()) {
                        match t {
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    let preview = if text.len() > 150 { format!("{}...", &text[..150]) } else { text.to_string() };
                                    texts.push(preview);
                                }
                            }
                            "tool_use" => {
                                if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                                    texts.push(format!("[tool_use: {}]", name));
                                }
                            }
                            "tool_result" => {
                                texts.push("[tool_result]".to_string());
                            }
                            _ => {}
                        }
                    }
                }
                texts.join(" | ")
            }
            _ => "[complex content]".to_string(),
        };

        if !content_preview.is_empty() {
            summary_parts.push(format!("{}: {}", role, content_preview));
        }
    }

    format!(
        "[Previous conversation summary — {} messages compacted]\n\n{}",
        messages_to_summarize.len(),
        summary_parts.join("\n\n")
    )
}

/// Compact messages if needed. Returns the new message list.
/// Keeps the most recent messages verbatim and summarizes older ones.
pub fn compact_if_needed(messages: &[Message], system_prompt_tokens: usize) -> Vec<Message> {
    if !needs_compaction(messages, system_prompt_tokens) {
        return messages.to_vec();
    }

    if messages.len() <= KEEP_RECENT_MESSAGES {
        return messages.to_vec(); // Not enough to compact
    }

    let split_point = messages.len() - KEEP_RECENT_MESSAGES;
    let old_messages = &messages[..split_point];
    let recent_messages = &messages[split_point..];

    let summary = create_summary_message(old_messages);

    let mut compacted = Vec::new();
    compacted.push(Message::text("user", &summary));
    compacted.push(Message::text("assistant", "Understood, I have the conversation context. Continuing from where we left off."));
    compacted.extend(recent_messages.iter().cloned());

    // Print a notice
    eprintln!("📦 Compacted {} old messages into summary (keeping {} recent)", old_messages.len(), recent_messages.len());

    compacted
}
