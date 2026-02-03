//! Syntax highlighting for fenced code blocks using syntect.
//!
//! Processes markdown text, finds fenced code blocks (```lang ... ```),
//! and replaces them with ANSI-highlighted output. Non-code content
//! passes through unchanged for termimad to render.

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

use std::sync::LazyLock;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Process markdown text: find fenced code blocks, syntax-highlight them,
/// and return the text with highlighted blocks replaced.
///
/// Code blocks without a language tag or with an unrecognised language
/// are left unchanged (termimad will render them as plain code blocks).
pub fn highlight_code_blocks(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pos = 0;

    while pos < text.len() {
        // Look for ``` at the start of a line (or start of string)
        if let Some(fence_start) = find_fence_start(text, pos) {
            // Push everything before the fence
            result.push_str(&text[pos..fence_start]);

            // Parse the opening fence: ```lang
            let line_end = text[fence_start..].find('\n').map_or(text.len(), |i| fence_start + i);
            let fence_line = &text[fence_start..line_end];
            let lang = fence_line.trim_start_matches('`').trim();

            // Find closing ```
            let content_start = if line_end < text.len() { line_end + 1 } else { line_end };
            if let Some(close_offset) = find_closing_fence(text, content_start) {
                let code_content = &text[content_start..close_offset];
                let close_line_end = text[close_offset..].find('\n')
                    .map_or(text.len(), |i| close_offset + i + 1);

                if let Some(highlighted) = try_highlight(lang, code_content) {
                    // Replace the entire fenced block with highlighted output.
                    // We emit raw ANSI — termimad will pass it through since
                    // it won't be inside a code fence anymore.
                    result.push_str("\n");
                    result.push_str(&highlighted);
                    if !highlighted.ends_with('\n') {
                        result.push('\n');
                    }
                } else {
                    // Unknown language — keep original for termimad
                    result.push_str(&text[fence_start..close_line_end]);
                }

                pos = close_line_end;
            } else {
                // No closing fence — pass through as-is
                result.push_str(&text[fence_start..line_end]);
                pos = line_end;
            }
        } else {
            // No more fences — push the rest
            result.push_str(&text[pos..]);
            break;
        }
    }

    result
}

/// Find the start of a ``` fence at or after `from`, only matching at line start.
fn find_fence_start(text: &str, from: usize) -> Option<usize> {
    let search = &text[from..];
    let mut offset = 0;

    while offset < search.len() {
        if let Some(idx) = search[offset..].find("```") {
            let abs = from + offset + idx;
            // Must be at start of line (or start of string)
            if abs == 0 || text.as_bytes()[abs - 1] == b'\n' {
                return Some(abs);
            }
            offset += idx + 3;
        } else {
            break;
        }
    }
    None
}

/// Find the closing ``` fence starting from `from`.
fn find_closing_fence(text: &str, from: usize) -> Option<usize> {
    let search = &text[from..];
    let mut offset = 0;

    while offset < search.len() {
        if let Some(idx) = search[offset..].find("```") {
            let abs = from + offset + idx;
            // Must be at start of line
            if abs == 0 || text.as_bytes()[abs - 1] == b'\n' {
                return Some(abs);
            }
            offset += idx + 3;
        } else {
            break;
        }
    }
    None
}

/// Try to syntax-highlight a code block. Returns None if the language
/// is empty or not recognised by syntect.
fn try_highlight(lang: &str, code: &str) -> Option<String> {
    if lang.is_empty() || code.trim().is_empty() {
        return None;
    }

    let syntax = SYNTAX_SET.find_syntax_by_token(lang)?;
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut h = HighlightLines::new(syntax, theme);
    let mut output = String::new();

    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &SYNTAX_SET).ok()?;
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        output.push_str(&escaped);
    }
    // Reset terminal colors
    output.push_str("\x1b[0m");

    Some(output)
}
