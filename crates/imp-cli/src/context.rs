use crate::config::imp_home;
use crate::error::Result;
use crate::project::ProjectInfo;
use chrono::Local;
use std::fs;
use std::path::Path;

/// Manages three-layer context loading and system prompt assembly.
///
/// Layer 1 — Global (~/.imp/): identity, memory, engineering context
/// Layer 2 — Per-project (~/.imp/projects/<name>/): learned project context
/// Layer 3 — Repo team context (<cwd>/.imp/): team-maintained, checked into repo
pub struct ContextManager {
    global_sections: Vec<ContextSection>,
    project_sections: Vec<ContextSection>,
    team_sections: Vec<ContextSection>,
}

#[derive(Debug, Clone)]
struct ContextSection {
    heading: String,
    content: String,
}

impl ContextManager {
    /// Load all three context layers.
    pub fn load(project: Option<&ProjectInfo>, cwd: &Path) -> Result<Self> {
        let home = imp_home()?;

        let global_sections = Self::load_global(&home)?;

        let project_sections = if let Some(proj) = project {
            Self::load_project(&home, &proj.name)?
        } else {
            Vec::new()
        };

        let team_sections = Self::load_team(cwd)?;

        Ok(Self {
            global_sections,
            project_sections,
            team_sections,
        })
    }

    /// Load Layer 1 — Global context from ~/.imp/
    fn load_global(home: &Path) -> Result<Vec<ContextSection>> {
        let mut sections = Vec::new();

        // Always load: IDENTITY.md, MEMORY.md
        if let Some(content) = read_md(home, "IDENTITY.md") {
            sections.push(ContextSection {
                heading: "Identity".into(),
                content,
            });
        }
        if let Some(content) = read_md(home, "MEMORY.md") {
            sections.push(ContextSection {
                heading: "Long-Term Memory".into(),
                content,
            });
        }

        // Optional engineering context
        for (file, heading) in [
            ("STACK.md", "Technology Stack"),
            ("PRINCIPLES.md", "Coding Principles"),
            ("ARCHITECTURE.md", "Architecture Overview"),
            ("PRIORITIES.md", "Current Priorities"),
            ("TRACKING.md", "Task Tracking"),
        ] {
            if let Some(content) = read_md(home, file) {
                sections.push(ContextSection {
                    heading: heading.into(),
                    content,
                });
            }
        }

        // Daily memory: today + yesterday
        let memory_dir = home.join("memory");
        if memory_dir.exists() {
            let today = Local::now().format("%Y-%m-%d").to_string();
            let yesterday =
                (Local::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
            for date in [&yesterday, &today] {
                if let Some(content) = read_md(&memory_dir, &format!("{}.md", date)) {
                    sections.push(ContextSection {
                        heading: format!("Memory — {}", date),
                        content,
                    });
                }
            }
        }

        Ok(sections)
    }

    /// Load Layer 2 — Per-project context from ~/.imp/projects/<name>/
    fn load_project(home: &Path, project_name: &str) -> Result<Vec<ContextSection>> {
        let project_dir = home.join("projects").join(project_name);
        let mut sections = Vec::new();

        if !project_dir.exists() {
            return Ok(sections);
        }

        for (file, heading) in [
            ("CONTEXT.md", "Project Context"),
            ("PATTERNS.md", "Project Patterns"),
            ("HISTORY.md", "Project Decision History"),
        ] {
            if let Some(content) = read_md(&project_dir, file) {
                sections.push(ContextSection {
                    heading: heading.into(),
                    content,
                });
            }
        }

        // Project-specific daily memory
        let memory_dir = project_dir.join("memory");
        if memory_dir.exists() {
            let today = Local::now().format("%Y-%m-%d").to_string();
            let yesterday =
                (Local::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
            for date in [&yesterday, &today] {
                if let Some(content) = read_md(&memory_dir, &format!("{}.md", date)) {
                    sections.push(ContextSection {
                        heading: format!("Project Memory — {}", date),
                        content,
                    });
                }
            }
        }

        Ok(sections)
    }

    /// Load Layer 3 — Team/repo context from <cwd>/.imp/
    fn load_team(cwd: &Path) -> Result<Vec<ContextSection>> {
        let team_dir = cwd.join(".imp");
        let mut sections = Vec::new();

        if !team_dir.exists() || !team_dir.is_dir() {
            return Ok(sections);
        }

        // Read all .md files from the team directory
        let mut entries: Vec<_> = fs::read_dir(&team_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .map_or(false, |ext| ext == "md")
            })
            .collect();

        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            if let Ok(content) = fs::read_to_string(&path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() && has_meaningful_content(trimmed) {
                    let name = path
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown");
                    sections.push(ContextSection {
                        heading: format!("Team — {}", name),
                        content: trimmed.to_string(),
                    });
                }
            }
        }

        Ok(sections)
    }

    /// Assemble the full system prompt from all three layers.
    pub fn assemble_system_prompt(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Global sections (identity, memory, engineering context)
        for section in &self.global_sections {
            parts.push(format!("# {}\n\n{}", section.heading, section.content));
        }

        // Team sections (complement global with repo-specific context)
        for section in &self.team_sections {
            parts.push(format!("# {}\n\n{}", section.heading, section.content));
        }

        // Project sections (what the agent has learned about this project)
        for section in &self.project_sections {
            parts.push(format!("# {}\n\n{}", section.heading, section.content));
        }

        if parts.is_empty() {
            "You are a personal AI agent with memory and learning capabilities. You adapt to your user over time.".to_string()
        } else {
            parts.join("\n\n---\n\n")
        }
    }

    /// List all loaded context sections for display.
    pub fn loaded_summary(&self) -> Vec<String> {
        let mut names = Vec::new();
        for s in &self.global_sections {
            names.push(format!("global: {}", s.heading));
        }
        for s in &self.team_sections {
            names.push(s.heading.clone());
        }
        for s in &self.project_sections {
            names.push(s.heading.clone());
        }
        names
    }
}

/// Read a markdown file from a directory, returning None if missing or boilerplate-only.
fn read_md(dir: &Path, filename: &str) -> Option<String> {
    let path = dir.join(filename);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() || !has_meaningful_content(trimmed) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Check if content has meaningful text beyond just headings and HTML comments.
fn has_meaningful_content(content: &str) -> bool {
    content.lines().any(|line| {
        let l = line.trim();
        !l.is_empty() && !l.starts_with('#') && !l.starts_with("<!--")
    })
}
