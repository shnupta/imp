//! Syntax highlighting for fenced code blocks using syntect.
//!
//! Pre-processes markdown text: fenced code blocks with language tags get
//! replaced with ANSI-highlighted output before termimad renders the rest.

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

use std::sync::LazyLock;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Available theme names (from syntect's built-in themes).
pub fn available_themes() -> Vec<String> {
    THEME_SET.themes.keys().cloned().collect()
}

/// Process markdown text: find fenced code blocks, syntax-highlight them,
/// and return the text with highlighted blocks replaced.
///
/// Code blocks without a language tag or with an unrecognised language
/// are left unchanged (termimad will render them as plain code blocks).
pub fn highlight_code_blocks(text: &str, theme: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pos = 0;

    while pos < text.len() {
        if let Some(fence_start) = find_fence_start(text, pos) {
            result.push_str(&text[pos..fence_start]);

            let line_end = text[fence_start..].find('\n').map_or(text.len(), |i| fence_start + i);
            let fence_line = &text[fence_start..line_end];
            let lang = fence_line.trim_start_matches('`').trim();

            let content_start = if line_end < text.len() { line_end + 1 } else { line_end };
            if let Some(close_offset) = find_closing_fence(text, content_start) {
                let code_content = &text[content_start..close_offset];
                let close_line_end = text[close_offset..].find('\n')
                    .map_or(text.len(), |i| close_offset + i + 1);

                if let Some(highlighted) = try_highlight(lang, code_content, theme) {
                    result.push_str("\n");
                    result.push_str(&highlighted);
                    if !highlighted.ends_with('\n') {
                        result.push('\n');
                    }
                } else {
                    result.push_str(&text[fence_start..close_line_end]);
                }

                pos = close_line_end;
            } else {
                result.push_str(&text[fence_start..line_end]);
                pos = line_end;
            }
        } else {
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

/// Try to syntax-highlight code. Returns None if language is unrecognised.
fn try_highlight(lang: &str, code: &str, theme_name: &str) -> Option<String> {
    if lang.is_empty() || code.trim().is_empty() {
        return None;
    }

    let syntax = SYNTAX_SET.find_syntax_by_token(lang)?;
    let theme = THEME_SET.themes.get(theme_name)
        .or_else(|| THEME_SET.themes.get("base16-ocean.dark"))?;
    let mut h = HighlightLines::new(syntax, theme);
    let mut output = String::new();

    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, &SYNTAX_SET).ok()?;
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        output.push_str(&escaped);
    }
    output.push_str("\x1b[0m");

    Some(output)
}
