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

    do_compact(messages, KEEP_RECENT_MESSAGES)
}

/// Force compaction regardless of threshold. Used when the API rejects input as too long,
/// or when the user explicitly requests compaction.
/// Uses a more aggressive keep count to ensure we get well under the limit.
pub fn force_compact(messages: &[Message]) -> Vec<Message> {
    // Keep fewer messages when force-compacting — we know we're over the limit
    let aggressive_keep = (KEEP_RECENT_MESSAGES / 2).max(4);
    do_compact(messages, aggressive_keep)
}

/// Perform the actual compaction, keeping `keep_recent` messages verbatim.
fn do_compact(messages: &[Message], keep_recent: usize) -> Vec<Message> {
    if messages.len() <= keep_recent {
        // Not enough messages to compact — truncate large tool results instead
        let mut truncated = messages.to_vec();
        truncate_old_tool_results(&mut truncated, 2);
        return truncated;
    }

    let split_point = messages.len() - keep_recent;
    let old_messages = &messages[..split_point];
    let recent_messages = &messages[split_point..];

    let summary = create_summary_message(old_messages);

    let mut compacted = Vec::new();
    compacted.push(Message::text("user", &summary));
    compacted.push(Message::text("assistant", "Understood, I have the conversation context. Continuing from where we left off."));
    compacted.extend(recent_messages.iter().cloned());

    // Also truncate large tool results in the kept messages
    truncate_old_tool_results(&mut compacted, 4);

    eprintln!("📦 Compacted {} old messages into summary (keeping {} recent)", old_messages.len(), keep_recent);

    compacted
}

/// Truncate large tool_result content in messages, except the last `preserve_last` messages.
fn truncate_old_tool_results(messages: &mut [Message], preserve_last: usize) {
    const MAX_TOOL_RESULT_CHARS: usize = 1500;

    if messages.len() <= preserve_last {
        return;
    }

    let truncate_up_to = messages.len() - preserve_last;
    for msg in &mut messages[..truncate_up_to] {
        if let Value::Array(blocks) = &mut msg.content {
            for block in blocks.iter_mut() {
                let is_tool_result = block
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map_or(false, |t| t == "tool_result");

                if !is_tool_result {
                    continue;
                }

                if let Some(text) = block.get("content").and_then(|c| c.as_str()) {
                    if text.len() > MAX_TOOL_RESULT_CHARS {
                        let preview: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
                        block["content"] = Value::String(format!(
                            "{}… [truncated — {} chars total]",
                            preview,
                            text.len()
                        ));
                    }
                }
            }
        }
    }
}
