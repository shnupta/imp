use crate::config::imp_home;
use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub git_remote: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectRegistry {
    #[serde(default)]
    pub projects: HashMap<String, ProjectInfo>,
}

/// Detect the current project from the working directory.
/// Tries git remote URL first, falls back to directory name.
pub fn detect_project(cwd: &Path) -> Option<ProjectInfo> {
    // Check if we're in a git repo at all
    let toplevel = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !toplevel.status.success() {
        return None;
    }

    let repo_root = PathBuf::from(
        String::from_utf8_lossy(&toplevel.stdout).trim().to_string(),
    );

    // Try git remote
    let git_remote = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        });

    // Derive name from git remote URL or directory name
    let name = if let Some(ref remote) = git_remote {
        extract_project_name_from_remote(remote)
    } else {
        repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    Some(ProjectInfo {
        name,
        path: repo_root.to_string_lossy().to_string(),
        git_remote,
    })
}

/// Extract a project name from a git remote URL.
/// e.g., "git@github.com:user/repo.git" -> "repo"
/// e.g., "https://github.com/user/repo" -> "repo"
fn extract_project_name_from_remote(remote: &str) -> String {
    let remote = remote.trim_end_matches(".git");
    remote
        .rsplit('/')
        .next()
        .or_else(|| remote.rsplit(':').next())
        .unwrap_or("unknown")
        .to_string()
}

impl ProjectRegistry {
    fn registry_path() -> Result<PathBuf> {
        Ok(imp_home()?.join("projects").join("registry.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::registry_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        let registry: ProjectRegistry = toml::from_str(&content)
            .map_err(|e| ImpError::Config(format!("Failed to parse registry: {}", e)))?;
        Ok(registry)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::registry_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| ImpError::Config(e.to_string()))?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn register_project(&mut self, info: &ProjectInfo) -> Result<()> {
        self.projects.insert(info.name.clone(), info.clone());
        self.save()?;
        ensure_project_context(&info.name)?;
        Ok(())
    }

    pub fn list_projects(&self) -> Vec<&ProjectInfo> {
        self.projects.values().collect()
    }

    pub fn get_project(&self, name: &str) -> Option<&ProjectInfo> {
        self.projects.get(name)
    }
}

/// Ensure the per-project context directory exists with skeleton files.
pub fn ensure_project_context(name: &str) -> Result<()> {
    let project_dir = imp_home()?.join("projects").join(name);
    fs::create_dir_all(&project_dir)?;

    // Create CONTEXT.md if missing
    let context_path = project_dir.join("CONTEXT.md");
    if !context_path.exists() {
        let template = include_str!("../../../templates/project/CONTEXT.md");
        let content = template.replace("{{name}}", name);
        fs::write(&context_path, content)?;
    }

    // Create PATTERNS.md if missing
    let patterns_path = project_dir.join("PATTERNS.md");
    if !patterns_path.exists() {
        fs::write(
            &patterns_path,
            format!(
                "# Code Patterns — {}\n\nPatterns, idioms, and conventions specific to this project.\n",
                name
            ),
        )?;
    }

    // Create HISTORY.md if missing
    let history_path = project_dir.join("HISTORY.md");
    if !history_path.exists() {
        fs::write(
            &history_path,
            format!(
                "# Decision History — {}\n\nSignificant decisions made during work on this project.\n",
                name
            ),
        )?;
    }

    // Create memory directory
    fs::create_dir_all(project_dir.join("memory"))?;

    Ok(())
}
