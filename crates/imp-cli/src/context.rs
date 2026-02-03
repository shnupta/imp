//! Tiered context management system for Imp agents.
//!
//! Implements a three-layer context system:
//! - L1 (Always loaded): Identity, user info, project summary, self-learning instructions
//! - L2 (On-demand): Full project context, memory, patterns, git info — listed in prompt
//!   so the agent can file_read them when relevant
//! - L3 (Cold storage): SQLite imp.db — searchable via exec tool

use crate::config::imp_home;
use crate::error::Result;
use crate::project::ProjectInfo;
use chrono::Local;
use std::fs;
use std::path::Path;

/// Context manager with tiered loading.
///
/// L1 sections are always included in the system prompt.
/// L2 files are listed with paths and size hints so the agent can load them on demand.
pub struct ContextManager {
    l1_sections: Vec<ContextSection>,
    l2_manifest: Vec<L2FileInfo>,
}

#[derive(Debug, Clone)]
struct ContextSection {
    heading: String,
    content: String,
}

#[derive(Debug, Clone)]
struct L2FileInfo {
    path: String,
    heading: String,
    size_hint: String,
}

impl ContextManager {
    /// Load context, categorizing into L1 (always loaded) and L2 (on-demand).
    pub fn load(project: Option<&ProjectInfo>) -> Result<Self> {
        let home = imp_home()?;
        let mut l1_sections = Vec::new();
        let mut l2_manifest = Vec::new();

        // ── L1: Always loaded (lean, <2k tokens) ────────────────────

        // Core identity files — always needed
        load_md(&home, "IDENTITY.md", "Identity", &mut l1_sections);
        load_md(&home, "USER.md", "About Your Human", &mut l1_sections);

        // ── L2: Global files (on-demand) ─────────────────────────────

        // Memory — available but not auto-loaded
        register_l2_file(
            &home.join("MEMORY.md"),
            "Long-term memory and lessons learned",
            &mut l2_manifest,
        );

        // Optional engineering context files → L2
        register_l2_file(
            &home.join("STACK.md"),
            "Technology stack",
            &mut l2_manifest,
        );
        register_l2_file(
            &home.join("PRINCIPLES.md"),
            "Coding principles",
            &mut l2_manifest,
        );
        register_l2_file(
            &home.join("ARCHITECTURE.md"),
            "Architecture overview",
            &mut l2_manifest,
        );

        // Daily memory files → L2
        register_daily_memory_l2(&home.join("memory"), "Daily memory", &mut l2_manifest);

        // ── Per-project context ──────────────────────────────────────
        if let Some(proj) = project {
            let project_dir = home.join("projects").join(&proj.name);
            if project_dir.exists() {
                // L1: Project summary (first ~500 chars of CONTEXT.md)
                load_md_summary(
                    &project_dir,
                    "CONTEXT.md",
                    &format!("Project Summary — {}", proj.name),
                    500,
                    &mut l1_sections,
                );

                // L2: Full project files
                register_l2_file(
                    &project_dir.join("CONTEXT.md"),
                    &format!("Full project context — {}", proj.name),
                    &mut l2_manifest,
                );
                register_l2_file(
                    &project_dir.join("PATTERNS.md"),
                    &format!("Code patterns and conventions — {}", proj.name),
                    &mut l2_manifest,
                );
                register_l2_file(
                    &project_dir.join("HISTORY.md"),
                    &format!("Project history — {}", proj.name),
                    &mut l2_manifest,
                );

                // Project daily memory → L2
                register_daily_memory_l2(
                    &project_dir.join("memory"),
                    &format!("Project memory — {}", proj.name),
                    &mut l2_manifest,
                );
            }

            // Git context → L2 (just note that it's available)
            register_git_context_l2(Path::new(&proj.path), &proj.name, &mut l2_manifest);

            // Auto-detect common AI coding assistant rules files
            let project_root = Path::new(&proj.path);
            for (rel_path, desc) in &[
                (".cursorrules", "Cursor rules"),
                ("CLAUDE.md", "Claude project instructions"),
                ("AGENTS.md", "Agent instructions"),
                (".github/copilot-instructions.md", "Copilot instructions"),
            ] {
                let full_path = project_root.join(rel_path);
                register_l2_file(
                    &full_path,
                    &format!("{} — {}", desc, proj.name),
                    &mut l2_manifest,
                );
            }
        }

        Ok(Self {
            l1_sections,
            l2_manifest,
        })
    }

    /// Assemble the full system prompt: L1 content + L2 manifest.
    pub fn assemble_system_prompt(&self) -> String {
        let mut prompt_parts = Vec::new();

        // Home directory information (L1)
        if let Ok(home) = imp_home() {
            let home_section = format!(
                "# Your Home Directory\n\n\
                Your files are stored at ~/.imp/ (resolved to {}).\n\
                - IDENTITY.md, USER.md — your core identity\n\
                - MEMORY.md — long-term memory (load when needed)\n\
                - memory/YYYY-MM-DD.md — daily notes\n\
                - projects/<name>/ — per-project context\n\n\
                Use file_read and file_write tools with these ABSOLUTE paths to read and update your context files.",
                home.display()
            );
            prompt_parts.push(home_section);

            // Self-learning instructions (L1)
            prompt_parts.push(
                "# Self-Learning\n\n\
                You can and should update your own context files to improve over time:\n\
                - ~/.imp/memory/YYYY-MM-DD.md — daily notes about what you learned\n\
                - ~/.imp/projects/<name>/CONTEXT.md — project-specific knowledge\n\
                - ~/.imp/projects/<name>/PATTERNS.md — code patterns and conventions you've noticed\n\
                - ~/.imp/MEMORY.md — long-term memory (important things to remember)\n\n\
                After completing significant work, use file_write to update relevant context files."
                    .to_string(),
            );
        }

        // Add all L1 context sections
        for section in &self.l1_sections {
            prompt_parts.push(format!("# {}\n\n{}", section.heading, section.content));
        }

        // Add L2 manifest — tell the agent what's available on-demand
        if !self.l2_manifest.is_empty() {
            let mut manifest = String::from(
                "# Available Context (load with file_read when relevant)\n\n\
                These files contain additional context. Read them when the conversation requires it:\n",
            );
            for entry in &self.l2_manifest {
                manifest.push_str(&format!(
                    "\n- {} — {} ({})",
                    entry.path, entry.heading, entry.size_hint
                ));
            }
            manifest.push_str(
                "\n\nUse file_read to access any of these when you need the information.",
            );
            prompt_parts.push(manifest);
        }

        if prompt_parts.is_empty() && self.l1_sections.is_empty() {
            return "You are a personal AI agent with memory and learning capabilities.".to_string();
        }

        prompt_parts.join("\n\n---\n\n")
    }

    /// List all loaded L1 section headings (for display).
    pub fn loaded_sections(&self) -> Vec<&str> {
        self.l1_sections
            .iter()
            .map(|s| s.heading.as_str())
            .collect()
    }

    /// Extract the agent's name from the Identity section.
    pub fn agent_name(&self) -> Option<String> {
        let identity = self.l1_sections.iter().find(|s| s.heading == "Identity")?;
        // Try "**Your Name**: <name>"
        for line in identity.content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("**Your Name**:") {
                let name = rest.trim();
                if !name.is_empty() && !name.contains("{{") {
                    return Some(name.to_string());
                }
            }
        }
        // Fallback: "# Your Identity: <name>"
        for line in identity.content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("# Your Identity:") {
                let name = rest.trim();
                if !name.is_empty() && !name.contains("{{") {
                    return Some(name.to_string());
                }
            }
        }
        None
    }
}

// ── helpers ──────────────────────────────────────────────────────────

/// Read a markdown file and push it as a context section if it has meaningful content.
fn load_md(dir: &Path, filename: &str, heading: &str, sections: &mut Vec<ContextSection>) {
    let path = dir.join(filename);
    if let Ok(content) = fs::read_to_string(&path) {
        let trimmed = content.trim();
        if !trimmed.is_empty() && has_meaningful_content(trimmed) {
            sections.push(ContextSection {
                heading: heading.to_string(),
                content: trimmed.to_string(),
            });
        }
    }
}

/// Read a markdown file but only include the first `max_chars` characters as a summary.
fn load_md_summary(
    dir: &Path,
    filename: &str,
    heading: &str,
    max_chars: usize,
    sections: &mut Vec<ContextSection>,
) {
    let path = dir.join(filename);
    if let Ok(content) = fs::read_to_string(&path) {
        let trimmed = content.trim();
        if !trimmed.is_empty() && has_meaningful_content(trimmed) {
            let summary = if trimmed.len() > max_chars {
                // Try to cut at a word boundary
                let cut = &trimmed[..max_chars];
                match cut.rfind(char::is_whitespace) {
                    Some(pos) if pos > max_chars / 2 => format!("{}…", &trimmed[..pos]),
                    _ => format!("{}…", cut),
                }
            } else {
                trimmed.to_string()
            };
            sections.push(ContextSection {
                heading: heading.to_string(),
                content: summary,
            });
        }
    }
}

/// Register an existing file in the L2 manifest with its actual size.
fn register_l2_file(path: &Path, heading: &str, manifest: &mut Vec<L2FileInfo>) {
    if let Ok(metadata) = fs::metadata(path) {
        let size = metadata.len();
        if size == 0 {
            return;
        }
        // Also verify it has meaningful content (not just headings)
        if let Ok(content) = fs::read_to_string(path) {
            if !has_meaningful_content(content.trim()) {
                return;
            }
        }
        manifest.push(L2FileInfo {
            path: format_display_path(path),
            heading: heading.to_string(),
            size_hint: format_size_hint(size),
        });
    }
}

/// Register today's and yesterday's daily memory files in L2.
fn register_daily_memory_l2(
    memory_dir: &Path,
    prefix: &str,
    manifest: &mut Vec<L2FileInfo>,
) {
    if !memory_dir.exists() {
        return;
    }

    let today = Local::now().format("%Y-%m-%d").to_string();
    let yesterday = (Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    for (date, label) in [(&today, "today"), (&yesterday, "yesterday")] {
        let path = memory_dir.join(format!("{}.md", date));
        if let Ok(metadata) = fs::metadata(&path) {
            let size = metadata.len();
            if size > 0 {
                manifest.push(L2FileInfo {
                    path: format_display_path(&path),
                    heading: format!("{} — {} ({})", prefix, date, label),
                    size_hint: format_size_hint(size),
                });
            }
        }
    }
}

/// Register git context availability in L2 (doesn't run git commands).
fn register_git_context_l2(project_path: &Path, project_name: &str, manifest: &mut Vec<L2FileInfo>) {
    // Just check if it's a git repo without running expensive commands
    let git_dir = project_path.join(".git");
    if git_dir.exists() {
        manifest.push(L2FileInfo {
            path: format!("(run: git -C {} log --oneline -10)", project_path.display()),
            heading: format!("Git recent commits — {}", project_name),
            size_hint: "run via exec".to_string(),
        });
        manifest.push(L2FileInfo {
            path: format!("(run: git -C {} diff --stat)", project_path.display()),
            heading: format!("Git current changes — {}", project_name),
            size_hint: "run via exec".to_string(),
        });
    }
}

/// Check if content has meaningful text beyond just headings and HTML comments.
fn has_meaningful_content(content: &str) -> bool {
    content.lines().any(|line| {
        let l = line.trim();
        !l.is_empty() && !l.starts_with('#') && !l.starts_with("<!--")
    })
}

/// Format a byte count as a human-readable size hint.
fn format_size_hint(bytes: u64) -> String {
    if bytes < 1024 {
        format!("~{} bytes", bytes)
    } else {
        let kb = bytes as f64 / 1024.0;
        format!("~{:.1}k chars", kb)
    }
}

/// Convert an absolute path to a ~/.imp/ display path if possible.
fn format_display_path(path: &Path) -> String {
    if let Ok(home) = imp_home() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/.imp/{}", stripped.display());
        }
    }
    path.display().to_string()
}
