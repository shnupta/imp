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
use std::process::Command;

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
        load_md(&home, "USER.md", "About Your Human", &mut sections);
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

            // ── Git Context ───────────────────────────────────────────
            load_git_context(Path::new(&proj.path), &proj.name, &mut sections);
        }

        Ok(Self { sections })
    }

    /// Assemble the full system prompt from all loaded sections.
    pub fn assemble_system_prompt(&self) -> String {
        let mut prompt_parts = Vec::new();

        // Add agent home directory information
        if let Ok(home) = imp_home() {
            let home_section = format!(
                "# Your Home Directory\n\nYour files are stored at ~/.imp/ (resolved to {}).\n- IDENTITY.md, USER.md, MEMORY.md — your core context\n- memory/YYYY-MM-DD.md — daily notes\n- projects/<name>/ — per-project context\n\nUse file_read and file_write tools with these ABSOLUTE paths to read and update your context files.",
                home.display()
            );
            prompt_parts.push(home_section);
        }

        if self.sections.is_empty() {
            if prompt_parts.is_empty() {
                return "You are a personal AI agent with memory and learning capabilities."
                    .to_string();
            }
        } else {
            // Add all loaded context sections
            for section in &self.sections {
                prompt_parts.push(format!("# {}\n\n{}", section.heading, section.content));
            }
        }

        prompt_parts.join("\n\n---\n\n")
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

/// Load git context (recent commits and diff stats) if the project is a git repo.
fn load_git_context(project_path: &Path, project_name: &str, sections: &mut Vec<ContextSection>) {
    let mut git_info = Vec::new();

    // Try to get recent git log
    if let Ok(output) = Command::new("git")
        .args(&["log", "--oneline", "-10"])
        .current_dir(project_path)
        .output()
    {
        if output.status.success() {
            let log_output = String::from_utf8_lossy(&output.stdout);
            if !log_output.trim().is_empty() {
                git_info.push(format!("Recent commits:\n{}", log_output.trim()));
            }
        }
    }

    // Try to get diff stats
    if let Ok(output) = Command::new("git")
        .args(&["diff", "--stat"])
        .current_dir(project_path)
        .output()
    {
        if output.status.success() {
            let diff_output = String::from_utf8_lossy(&output.stdout);
            if !diff_output.trim().is_empty() {
                git_info.push(format!("Current changes:\n{}", diff_output.trim()));
            }
        }
    }

    // If we have git information, add it as a context section
    if !git_info.is_empty() {
        sections.push(ContextSection {
            heading: format!("Recent Git Activity — {}", project_name),
            content: git_info.join("\n\n"),
        });
    }
}
