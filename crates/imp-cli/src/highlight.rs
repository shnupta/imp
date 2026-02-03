//! Syntax highlighting for fenced code blocks using syntect.
//!
//! Renders markdown in segments: prose goes through termimad, fenced code
//! blocks are rendered directly with syntect + background rectangle.
//! This avoids termimad stripping ANSI escapes or padding from code blocks.

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use termimad::MadSkin;

use std::sync::LazyLock;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Subtle dark background for code blocks (RGB 30, 35, 45).
const CODE_BG: &str = "\x1b[48;2;30;35;45m";
const RESET: &str = "\x1b[0m";
/// Dim color for the language label
const DIM: &str = "\x1b[2m";

/// A segment of markdown content — either prose (for termimad) or a code block
/// (rendered directly).
enum Segment<'a> {
    Prose(&'a str),
    CodeBlock { lang: &'a str, code: &'a str },
}

/// Render markdown with syntax-highlighted code blocks.
/// Prose segments go through termimad; code blocks are rendered directly
/// with syntect highlighting and a full-width background rectangle.
pub fn render_markdown(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }

    let segments = parse_segments(text);
    let skin = make_skin();
    let term_width = terminal_width();
    let mut output = String::new();

    for segment in segments {
        match segment {
            Segment::Prose(prose) => {
                if !prose.trim().is_empty() {
                    let rendered = format!("{}", skin.term_text(prose));
                    output.push_str(rendered.trim_end());
                    output.push('\n');
                }
            }
            Segment::CodeBlock { lang, code } => {
                output.push_str(&render_code_block(lang, code, term_width));
            }
        }
    }

    output
}

/// Parse markdown text into alternating prose and code block segments.
fn parse_segments(text: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut pos = 0;

    while pos < text.len() {
        if let Some(fence_start) = find_fence_start(text, pos) {
            // Prose before the fence
            if fence_start > pos {
                segments.push(Segment::Prose(&text[pos..fence_start]));
            }

            // Parse opening fence: ```lang
            let line_end = text[fence_start..].find('\n')
                .map_or(text.len(), |i| fence_start + i);
            let fence_line = &text[fence_start..line_end];
            let lang = fence_line.trim_start_matches('`').trim();

            let content_start = if line_end < text.len() { line_end + 1 } else { line_end };

            if let Some(close_offset) = find_closing_fence(text, content_start) {
                let code = &text[content_start..close_offset];
                let close_line_end = text[close_offset..].find('\n')
                    .map_or(text.len(), |i| close_offset + i + 1);

                segments.push(Segment::CodeBlock { lang, code });
                pos = close_line_end;
            } else {
                // No closing fence — treat as prose
                segments.push(Segment::Prose(&text[fence_start..line_end]));
                pos = line_end;
            }
        } else {
            // No more fences — rest is prose
            segments.push(Segment::Prose(&text[pos..]));
            break;
        }
    }

    segments
}

/// Render a code block with syntax highlighting and full-width background.
fn render_code_block(lang: &str, code: &str, term_width: usize) -> String {
    let mut output = String::new();

    // Top border: empty line with background
    output.push_str(&bg_line("", term_width));

    // Language label (if present)
    if !lang.is_empty() {
        let label = format!(" {}{}{}", DIM, lang, RESET);
        output.push_str(&bg_line(&label, term_width));
    }

    // Try syntax highlighting; fall back to plain
    if let Some(highlighted) = try_highlight(lang, code, term_width) {
        output.push_str(&highlighted);
    } else {
        // Plain code with background
        for line in code.lines() {
            let display = format!("  {}", line);
            output.push_str(&bg_line(&display, term_width));
        }
    }

    // Bottom border: empty line with background
    output.push_str(&bg_line("", term_width));

    output
}

/// Create a full-width background line. `content` may contain ANSI escapes.
fn bg_line(content: &str, term_width: usize) -> String {
    let visible_len = strip_ansi_len(content);
    let padding = term_width.saturating_sub(visible_len);

    let mut line = String::new();
    line.push_str(CODE_BG);
    line.push_str(content);
    for _ in 0..padding {
        line.push(' ');
    }
    line.push_str(RESET);
    line.push('\n');
    line
}

/// Try to syntax-highlight code. Returns None if language is unrecognised.
fn try_highlight(lang: &str, code: &str, term_width: usize) -> Option<String> {
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
        let trimmed = escaped.trim_end_matches('\n');

        // Indent code 2 spaces within the block
        let indented = format!("  {}", trimmed);
        let visible_len = strip_ansi_len(&indented);
        let padding = term_width.saturating_sub(visible_len);

        output.push_str(CODE_BG);
        output.push_str(&indented);
        for _ in 0..padding {
            output.push(' ');
        }
        output.push_str(RESET);
        output.push('\n');
    }

    Some(output)
}

/// Create a MadSkin with inline code background styling.
fn make_skin() -> MadSkin {
    use termimad::crossterm::style::Color;
    let mut skin = MadSkin::default();
    // Style inline code with a subtle background
    skin.inline_code.set_bg(Color::Rgb { r: 45, g: 50, b: 60 });
    skin
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
