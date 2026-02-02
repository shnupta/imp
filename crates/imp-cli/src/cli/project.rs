use crate::config::imp_home;
use crate::context::ContextManager;
use crate::error::Result;
use crate::project::{self, ProjectRegistry};
use console::style;

pub async fn list() -> Result<()> {
    let registry = ProjectRegistry::load()?;
    let projects = registry.list_projects();

    if projects.is_empty() {
        println!("No projects registered yet.");
        println!(
            "Run {} inside a git repo, or use {}",
            style("imp ask").cyan(),
            style("imp project add").cyan()
        );
        return Ok(());
    }

    println!("{}", style("Registered projects:").bold());
    println!();
    for project in projects {
        println!("  {} {}", style("•").dim(), style(&project.name).bold());
        println!("    Path: {}", project.path);
        if let Some(ref remote) = project.git_remote {
            println!("    Remote: {}", remote);
        }
    }

    Ok(())
}

pub async fn add(path: Option<String>) -> Result<()> {
    let target = if let Some(p) = path {
        std::path::PathBuf::from(p).canonicalize()?
    } else {
        std::env::current_dir()?
    };

    let info = project::detect_project(&target);
    match info {
        Some(info) => {
            let mut registry = ProjectRegistry::load()?;
            if registry.get_project(&info.name).is_some() {
                println!(
                    "Project '{}' is already registered.",
                    style(&info.name).bold()
                );
            } else {
                registry.register_project(&info)?;
                println!(
                    "✅ Registered project '{}'",
                    style(&info.name).bold()
                );
                println!("   Path: {}", info.path);
                if let Some(ref remote) = info.git_remote {
                    println!("   Remote: {}", remote);
                }
            }
        }
        None => {
            println!(
                "{}",
                style("Not a git repository. Navigate to a git repo and try again.").yellow()
            );
        }
    }

    Ok(())
}

pub async fn context() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let project_info = project::detect_project(&cwd);

    match project_info {
        Some(ref info) => {
            println!(
                "{} {}\n",
                style("Project:").bold(),
                style(&info.name).bold().blue()
            );

            let context = ContextManager::load(Some(info), &cwd)?;
            let summary = context.loaded_summary();

            if summary.is_empty() {
                println!("No context files loaded.");
            } else {
                println!("{}", style("Loaded context:").bold());
                for name in summary {
                    println!("  {} {}", style("•").dim(), name);
                }
            }

            // Show project directory
            let home = imp_home()?;
            let project_dir = home.join("projects").join(&info.name);
            println!(
                "\n{} {}",
                style("Project context dir:").dim(),
                project_dir.display()
            );

            // Show team context if it exists
            let team_dir = cwd.join(".imp");
            if team_dir.exists() {
                println!(
                    "{} {}",
                    style("Team context dir:").dim(),
                    team_dir.display()
                );
            }
        }
        None => {
            println!("Not in a git repository. No project context to show.");

            // Still show global context
            let context = ContextManager::load(None, &cwd)?;
            let summary = context.loaded_summary();
            if !summary.is_empty() {
                println!("\n{}", style("Global context loaded:").bold());
                for name in summary {
                    println!("  {} {}", style("•").dim(), name);
                }
            }
        }
    }

    Ok(())
}
