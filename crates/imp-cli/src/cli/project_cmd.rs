use crate::context::ContextManager;
use crate::error::Result;
use crate::project::{detect_project, ProjectRegistry};
use console::style;
use std::path::PathBuf;

/// `imp project list`
pub fn list() -> Result<()> {
    let registry = ProjectRegistry::load()?;
    let projects = registry.list_projects();

    if projects.is_empty() {
        println!("No projects registered yet.");
        println!(
            "Run {} in a project directory, or use {}.",
            style("imp ask").cyan(),
            style("imp project add").cyan()
        );
        return Ok(());
    }

    println!("{}", style("Registered projects:").bold());
    for p in projects {
        let remote = p.git_remote.as_deref().unwrap_or("(no remote)");
        println!(
            "  {} — {} ({})",
            style(&p.name).cyan(),
            &p.path,
            style(remote).dim()
        );
    }

    Ok(())
}

/// `imp project add [path]`
pub fn add(path: Option<PathBuf>) -> Result<()> {
    let dir = match path {
        Some(p) => std::fs::canonicalize(p)?,
        None => std::env::current_dir()?,
    };

    let info = detect_project(&dir).ok_or_else(|| {
        crate::error::ImpError::Config(format!(
            "Could not detect a project in {}",
            dir.display()
        ))
    })?;

    let mut registry = ProjectRegistry::load()?;
    registry.register_project(&info)?;

    println!(
        "✅ Registered project '{}' at {}",
        style(&info.name).cyan(),
        &info.path,
    );

    Ok(())
}

/// `imp project context` — show context summary for the current project.
pub fn context() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let info = detect_project(&cwd);

    match info {
        Some(ref proj) => {
            println!(
                "{} {}",
                style("Project:").bold(),
                style(&proj.name).cyan()
            );
            println!("  Path:   {}", &proj.path);
            if let Some(ref remote) = proj.git_remote {
                println!("  Remote: {}", remote);
            }

            let ctx = ContextManager::load(Some(proj))?;
            let sections = ctx.loaded_sections();

            if sections.is_empty() {
                println!("\n  No context loaded.");
            } else {
                println!("\n{}", style("Loaded context sections:").bold());
                for s in sections {
                    println!("  • {}", s);
                }
            }
        }
        None => {
            println!("Not inside a detectable project (no git repo found).");
            println!(
                "Use {} to register one explicitly.",
                style("imp project add <path>").cyan()
            );
        }
    }

    Ok(())
}
