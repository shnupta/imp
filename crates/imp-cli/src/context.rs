use crate::error::Result;
use chrono;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ContextFile {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct ContextManager {
    files: HashMap<String, ContextFile>,
    context_dir: PathBuf,
}

impl ContextManager {
    pub fn new<P: AsRef<Path>>(context_dir: P) -> Self {
        Self {
            files: HashMap::new(),
            context_dir: context_dir.as_ref().to_path_buf(),
        }
    }

    pub fn load_all(&mut self) -> Result<()> {
        self.files.clear();
        
        if !self.context_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.context_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "md") {
                if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                    let content = fs::read_to_string(&path)?;
                    let context_file = ContextFile {
                        name: name.to_string(),
                        path: path.clone(),
                        content,
                    };
                    self.files.insert(name.to_string(), context_file);
                }
            }
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ContextFile> {
        self.files.get(name)
    }

    pub fn get_content(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(|f| f.content.as_str())
    }

    pub fn list_files(&self) -> Vec<&str> {
        self.files.keys().map(|s| s.as_str()).collect()
    }

    pub fn assemble_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        // Load order matters — identity and operating instructions frame everything else.
        let ordered_files = [
            ("AGENTS",       "Operating Instructions"),
            ("SOUL",         "Persona"),
            ("IDENTITY",     "Identity"),
            ("USER",         "About the User"),
            ("TOOLS",        "Tools & Environment"),
            ("MEMORY",       "Long-Term Memory"),
            ("STACK",        "Technology Stack"),
            ("PRINCIPLES",   "Coding Principles"),
            ("ARCHITECTURE", "Architecture Overview"),
        ];

        for (name, heading) in ordered_files {
            if let Some(content) = self.get_content(name) {
                let trimmed = content.trim();
                if !trimmed.is_empty() && !trimmed.lines().all(|l| l.starts_with('#') || l.starts_with("<!--") || l.trim().is_empty()) {
                    sections.push(format!("# {}\n\n{}", heading, trimmed));
                }
            }
        }

        // Also load any memory daily files
        let memory_dir = self.context_dir.join("memory");
        if memory_dir.exists() {
            // Load today's and yesterday's memory
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let yesterday = (chrono::Local::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
            
            for date in [&yesterday, &today] {
                let path = memory_dir.join(format!("{}.md", date));
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            sections.push(format!("# Memory — {}\n\n{}", date, trimmed));
                        }
                    }
                }
            }
        }

        if sections.is_empty() {
            "You are an AI agent for engineering teams. Help with coding, architecture, and engineering tasks.".to_string()
        } else {
            sections.join("\n\n---\n\n")
        }
    }

    pub fn create_context_directory(&self) -> Result<()> {
        fs::create_dir_all(&self.context_dir)?;
        Ok(())
    }

    pub fn write_file(&self, name: &str, content: &str) -> Result<()> {
        let path = self.context_dir.join(format!("{}.md", name));
        fs::write(path, content)?;
        Ok(())
    }

    pub fn create_template_files(&self) -> Result<()> {
        self.create_context_directory()?;

        // Create template files if they don't exist
        let templates = vec![
            ("SOUL", include_str!("../../../templates/SOUL.md")),
            ("AGENTS", include_str!("../../../templates/AGENTS.md")),
            ("USER", include_str!("../../../templates/USER.md")),
            ("HEARTBEAT", include_str!("../../../templates/HEARTBEAT.md")),
            ("TOOLS", include_str!("../../../templates/TOOLS.md")),
            ("MEMORY", include_str!("../../../templates/MEMORY.md")),
            ("STACK", include_str!("../../../templates/STACK.md")),
            ("PRINCIPLES", include_str!("../../../templates/PRINCIPLES.md")),
            ("ARCHITECTURE", include_str!("../../../templates/ARCHITECTURE.md")),
        ];

        for (name, content) in templates {
            let path = self.context_dir.join(format!("{}.md", name));
            if !path.exists() {
                fs::write(path, content)?;
            }
        }

        Ok(())
    }
}