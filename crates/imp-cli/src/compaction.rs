use crate::client::Message;
use serde_json::Value;
use tracing::info;

const CHARS_PER_TOKEN: usize = 4;
const CONTEXT_LIMIT_TOKENS: usize = 200_000;
/// Reserve tokens for the model's response (thinking + output)
const RESPONSE_BUFFER_TOKENS: usize = 20_000;
const KEEP_RECENT_MESSAGES: usize = 10;

/// Estimate token count for a message
fn estimate_tokens(message: &Message) -> usize {
    let content_len = match &message.content {
        Value::String(s) => s.len(),
        Value::Array(blocks) => {
            blocks.iter().map(|b| {
                if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
                    text.len()
                } else if let Some(content) = b.get("content").and_then(|c| c.as_str()) {
                    content.len()
                } else {
                    b.to_string().len()
                }
            }).sum()
        }
        other => other.to_string().len(),
    };
    content_len / CHARS_PER_TOKEN
}

/// Estimate total tokens for all messages
fn estimate_total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_tokens).sum()
}

/// Estimate tokens for tool schemas (JSON serialized size / 4)
fn estimate_tool_tokens(tools: &Value) -> usize {
    tools.to_string().len() / CHARS_PER_TOKEN
}

/// Check if compaction is needed based on actual available budget.
///
/// available = context_limit - system_prompt - tool_schemas - response_buffer
/// compact when messages exceed available
fn needs_compaction(messages: &[Message], system_prompt_tokens: usize, tool_tokens: usize) -> bool {
    let overhead = system_prompt_tokens + tool_tokens + RESPONSE_BUFFER_TOKENS;
    let available = CONTEXT_LIMIT_TOKENS.saturating_sub(overhead);
    let message_tokens = estimate_total_tokens(messages);
    message_tokens > available
}

/// Create a compaction summary from messages
fn create_summary_message(messages_to_summarize: &[Message]) -> String {
    let mut summary_parts = Vec::new();

    for msg in messages_to_summarize {
        let role = &msg.role;
        let content_preview = match &msg.content {
            Value::String(s) => {
                if s.chars().count() > 200 {
                    format!("{}...", s.chars().take(200).collect::<String>())
                } else {
                    s.clone()
                }
            }
            Value::Array(blocks) => {
                let mut texts = Vec::new();
                for block in blocks {
                    if let Some(t) = block.get("type").and_then(|t| t.as_str()) {
                        match t {
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    let preview = if text.chars().count() > 150 {
                                        format!("{}...", text.chars().take(150).collect::<String>())
                                    } else {
                                        text.to_string()
                                    };
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

/// Core compaction: summarize older messages, keep recent ones verbatim,
/// and truncate large tool results in the kept messages.
fn do_compact(messages: &[Message]) -> Vec<Message> {
    if messages.len() <= KEEP_RECENT_MESSAGES {
        let mut truncated = messages.to_vec();
        truncate_old_tool_results(&mut truncated, 2);
        return truncated;
    }

    let split_point = messages.len() - KEEP_RECENT_MESSAGES;
    let old_messages = &messages[..split_point];
    let recent_messages = &messages[split_point..];

    let summary = create_summary_message(old_messages);

    let mut compacted = Vec::new();
    compacted.push(Message::text("user", &summary));
    compacted.push(Message::text("assistant", "Understood, I have the conversation context. Continuing from where we left off."));
    compacted.extend(recent_messages.iter().cloned());

    truncate_old_tool_results(&mut compacted, 4);

    info!(compacted = old_messages.len(), kept = KEEP_RECENT_MESSAGES, "Compacted conversation history");

    compacted
}

/// Proactive compaction: runs every iteration, compacts when estimated message
/// tokens exceed the available budget (context limit minus system prompt, tool
/// schemas, and response buffer). Adapts automatically to the actual overhead.
pub fn compact_if_needed(messages: &[Message], system_prompt_tokens: usize, tool_tokens: usize) -> Vec<Message> {
    if !needs_compaction(messages, system_prompt_tokens, tool_tokens) {
        return messages.to_vec();
    }

    do_compact(messages)
}

/// Reactive compaction: triggered when the API rejects input as too long,
/// or when the user explicitly requests /compact. Always compacts.
pub fn compact(messages: &[Message]) -> Vec<Message> {
    do_compact(messages)
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
