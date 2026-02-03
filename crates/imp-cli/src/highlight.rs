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

/// ANSI escape for a subtle dark background (RGB 30, 35, 45 — slightly lighter
/// than typical terminal backgrounds, matches base16-ocean.dark's feel).
const BG_START: &str = "\x1b[48;2;30;35;45m";
const BG_RESET: &str = "\x1b[0m";

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

    // Get terminal width for padding lines to full width
    let term_width = terminal_width();

    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &SYNTAX_SET).ok()?;
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);

        // Strip the trailing newline from the highlighted text so we can pad it
        let trimmed = escaped.trim_end_matches('\n');
        // Estimate visible length (strip ANSI escapes)
        let visible_len = strip_ansi_len(trimmed);
        let padding = term_width.saturating_sub(visible_len);

        output.push_str(BG_START);
        output.push_str(trimmed);
        // Pad to terminal width so the background fills the line
        for _ in 0..padding {
            output.push(' ');
        }
        output.push_str(BG_RESET);
        output.push('\n');
    }

    Some(output)
}

/// Get terminal width, defaulting to 80 if unavailable.
fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

/// Estimate the visible (non-ANSI-escape) length of a string.
fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
}
