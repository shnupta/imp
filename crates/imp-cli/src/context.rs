//! Context management system for Imp agents.
//!
//! Implements a two-layer context system:
//! - Layer 1: Global context from ~/.imp/ (identity, memory, engineering context)  
//! - Layer 2: Per-project context from ~/.imp/projects/<name>/ (project-specific context)
//!
//! Context files are loaded and assembled into system prompts that give the agent
//! understanding of the user's identity, project structure, and domain knowledge.

use crate::config::imp_home;
use crate::error::Result;
use crate::project::ProjectInfo;
use chrono::Local;
use std::fs;
use std::path::Path;

/// Two-layer context manager.
///
/// Layer 1 — Global (`~/.imp/`): identity, memory, optional engineering context.
/// Layer 2 — Per-project (`~/.imp/projects/<name>/`): learned project context.
pub struct ContextManager {
    sections: Vec<ContextSection>,
}

#[derive(Debug, Clone)]
struct ContextSection {
    heading: String,
    content: String,
}

impl ContextManager {
    /// Load context from both layers, optionally scoped to a project.
    pub fn load(project: Option<&ProjectInfo>) -> Result<Self> {
        let home = imp_home()?;
        let mut sections = Vec::new();

        // ── Layer 1: Global (~/.imp/) ────────────────────────────────
        // Always loaded
        load_md(&home, "IDENTITY.md", "Identity", &mut sections);
        load_md(&home, "MEMORY.md", "Long-Term Memory", &mut sections);

        // Optional engineering context
        load_md(&home, "STACK.md", "Technology Stack", &mut sections);
        load_md(&home, "PRINCIPLES.md", "Coding Principles", &mut sections);
        load_md(
            &home,
            "ARCHITECTURE.md",
            "Architecture Overview",
            &mut sections,
        );

        // Global daily memory (today + yesterday)
        load_daily_memory(&home.join("memory"), "Memory", &mut sections);

        // ── Layer 2: Per-project (~/.imp/projects/<name>/) ───────────
        if let Some(proj) = project {
            let project_dir = home.join("projects").join(&proj.name);
            if project_dir.exists() {
                load_md(
                    &project_dir,
                    "CONTEXT.md",
                    &format!("Project Context — {}", proj.name),
                    &mut sections,
                );
                load_md(
                    &project_dir,
                    "PATTERNS.md",
                    &format!("Project Patterns — {}", proj.name),
                    &mut sections,
                );
                load_md(
                    &project_dir,
                    "HISTORY.md",
                    &format!("Project History — {}", proj.name),
                    &mut sections,
                );

                load_daily_memory(
                    &project_dir.join("memory"),
                    &format!("Project Memory — {}", proj.name),
                    &mut sections,
                );
            }
        }

        Ok(Self { sections })
    }

    /// Assemble the full system prompt from all loaded sections.
    pub fn assemble_system_prompt(&self) -> String {
        if self.sections.is_empty() {
            return "You are a personal AI agent with memory and learning capabilities."
                .to_string();
        }

        self.sections
            .iter()
            .map(|s| format!("# {}\n\n{}", s.heading, s.content))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    /// List all loaded section headings (for display).
    pub fn loaded_sections(&self) -> Vec<&str> {
        self.sections.iter().map(|s| s.heading.as_str()).collect()
    }

    /// Extract the agent's name from the Identity section.
    /// Looks for "**Your Name**: <name>" or falls back to the heading "# Your Identity: <name>".
    pub fn agent_name(&self) -> Option<String> {
        let identity = self.sections.iter().find(|s| s.heading == "Identity")?;
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

/// Load today's and yesterday's daily memory files.
fn load_daily_memory(memory_dir: &Path, prefix: &str, sections: &mut Vec<ContextSection>) {
    if !memory_dir.exists() {
        return;
    }

    let today = Local::now().format("%Y-%m-%d").to_string();
    let yesterday = (Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    for date in [&yesterday, &today] {
        let path = memory_dir.join(format!("{}.md", date));
        if let Ok(content) = fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                sections.push(ContextSection {
                    heading: format!("{} — {}", prefix, date),
                    content: trimmed.to_string(),
                });
            }
        }
    }
}

/// Check if content has meaningful text beyond just headings and HTML comments.
fn has_meaningful_content(content: &str) -> bool {
    content.lines().any(|line| {
        let l = line.trim();
        !l.is_empty() && !l.starts_with('#') && !l.starts_with("<!--")
    })
}
