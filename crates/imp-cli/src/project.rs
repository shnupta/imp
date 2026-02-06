use crate::config::imp_home;
use crate::error::{ImpError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── ProjectInfo ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    /// Absolute path to the project root (stored as String for TOML compat).
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    /// Primary programming language detected from project files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Key configuration files present in the project
    #[serde(default)]
    pub config_files: Vec<String>,
    /// First line of README.md if present (project description)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Detection ────────────────────────────────────────────────────────

/// Detect a project from the current working directory.
/// Requires a git repo; returns None if not inside one.
pub fn detect_project(cwd: &Path) -> Option<ProjectInfo> {
    let toplevel = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !toplevel.status.success() {
        return None;
    }

    let repo_root = String::from_utf8_lossy(&toplevel.stdout).trim().to_string();
    let root_path = Path::new(&repo_root);

    let git_remote = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if url.is_empty() { None } else { Some(url) }
            } else {
                None
            }
        });

    let name = match &git_remote {
        Some(remote) => project_name_from_remote(remote),
        None => root_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
    };

    // Detect primary language
    let language = detect_primary_language(root_path);

    // Detect key configuration files
    let config_files = detect_config_files(root_path);

    // Extract project description from README.md
    let description = extract_readme_description(root_path);

    Some(ProjectInfo {
        name,
        path: repo_root,
        git_remote,
        language,
        config_files,
        description,
    })
}

/// Derive a short project name from a git remote URL.
fn project_name_from_remote(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Detect the primary programming language from project files
fn detect_primary_language(root: &Path) -> Option<String> {
    // Check for language-specific files in priority order
    let language_indicators = [
        ("Cargo.toml", "Rust"),
        ("package.json", "JavaScript/TypeScript"),
        ("pyproject.toml", "Python"),
        ("setup.py", "Python"),
        ("requirements.txt", "Python"),
        ("go.mod", "Go"),
        ("pom.xml", "Java"),
        ("build.gradle", "Java"),
        ("CMakeLists.txt", "C/C++"),
        ("Makefile", "C/C++"),
        ("composer.json", "PHP"),
        ("mix.exs", "Elixir"),
        ("pubspec.yaml", "Dart/Flutter"),
    ];

    for (file, lang) in &language_indicators {
        if root.join(file).exists() {
            return Some(lang.to_string());
        }
    }

    None
}

/// Detect key configuration files present in the project
fn detect_config_files(root: &Path) -> Vec<String> {
    let config_candidates = [
        "Makefile",
        "Dockerfile",
        "docker-compose.yml",
        "docker-compose.yaml",
        ".github/workflows",
        ".gitlab-ci.yml",
        "azure-pipelines.yml",
        "Jenkinsfile",
        ".travis.yml",
        "circle.yml",
        ".circleci/config.yml",
        "tailwind.config.js",
        "vite.config.js",
        "webpack.config.js",
        "rollup.config.js",
        "tsconfig.json",
        ".eslintrc.json",
        ".eslintrc.js",
        "prettier.config.js",
        ".prettierrc",
        "jest.config.js",
        "vitest.config.js",
        "playwright.config.js",
        "cypress.config.js",
        ".env.example",
        ".env.template",
        "flake.nix",
        "shell.nix",
        "Procfile",
        "app.yaml",
        "serverless.yml",
    ];

    let mut found = Vec::new();
    for candidate in &config_candidates {
        let path = root.join(candidate);
        if path.exists() {
            found.push(candidate.to_string());
        }
    }

    found
}

/// Extract the first meaningful line from README.md as project description
fn extract_readme_description(root: &Path) -> Option<String> {
    let readme_files = ["README.md", "readme.md", "Readme.md", "README.rst", "README.txt"];
    
    for readme_name in &readme_files {
        let readme_path = root.join(readme_name);
        if let Ok(content) = fs::read_to_string(&readme_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                // Skip empty lines and markdown headers
                if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("<!--") {
                    // Return first meaningful line, truncated if too long
                    if trimmed.chars().count() > 120 {
                        let truncated: String = trimmed.chars().take(117).collect();
                        return Some(format!("{}…", truncated));
                    } else {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    None
}

// ── Registry ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    projects: HashMap<String, ProjectInfo>,
}

pub struct ProjectRegistry {
    data: RegistryFile,
    path: PathBuf,
}

impl ProjectRegistry {
    pub fn load() -> Result<Self> {
        let path = imp_home()?.join("projects").join("registry.toml");
        let data = if path.exists() {
            let content = fs::read_to_string(&path)?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            RegistryFile::default()
        };
        Ok(Self { data, path })
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(&self.data).map_err(|e| ImpError::Config(e.to_string()))?;
        fs::write(&self.path, content)?;
        Ok(())
    }

    /// Register a project (idempotent). Creates skeleton context if needed.
    pub fn register_project(&mut self, info: &ProjectInfo) -> Result<()> {
        self.data
            .projects
            .entry(info.name.clone())
            .or_insert_with(|| info.clone());
        self.save()?;
        ensure_project_context(&info.name)?;
        Ok(())
    }

    pub fn list_projects(&self) -> Vec<&ProjectInfo> {
        self.data.projects.values().collect()
    }

    pub fn get(&self, name: &str) -> Option<&ProjectInfo> {
        self.data.projects.get(name)
    }
}

/// Ensure the per-project context directory and skeleton files exist.
pub fn ensure_project_context(name: &str) -> Result<()> {
    let dir = imp_home()?.join("projects").join(name);
    fs::create_dir_all(dir.join("memory"))?;

    let context_path = dir.join("CONTEXT.md");
    if !context_path.exists() {
        let content =
            include_str!("../../../templates/project/CONTEXT.md").replace("{{name}}", name);
        fs::write(&context_path, content)?;
    }

    let patterns_path = dir.join("PATTERNS.md");
    if !patterns_path.exists() {
        fs::write(
            &patterns_path,
            format!(
                "# Code Patterns — {}\n\nPatterns, idioms, and conventions specific to this project.\n",
                name
            ),
        )?;
    }

    let history_path = dir.join("HISTORY.md");
    if !history_path.exists() {
        fs::write(
            &history_path,
            format!(
                "# Decision History — {}\n\nSignificant decisions made during work on this project.\n",
                name
            ),
        )?;
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_from_ssh_remote() {
        assert_eq!(
            project_name_from_remote("git@github.com:user/my-project.git"),
            "my-project"
        );
    }

    #[test]
    fn name_from_https_remote() {
        assert_eq!(
            project_name_from_remote("https://github.com/user/my-project"),
            "my-project"
        );
    }

    #[test]
    fn name_from_trailing_slash() {
        assert_eq!(
            project_name_from_remote("https://github.com/user/my-project/"),
            "my-project"
        );
    }
}
