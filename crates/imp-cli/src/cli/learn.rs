use crate::config::imp_home;
use crate::error::Result;
use crate::project;
use console::style;
use dialoguer::{Input, Select};
use std::fs;

pub async fn run() -> Result<()> {
    println!("{}", style("📚 Teaching Your Agent").bold().blue());
    println!("What would you like to teach me?\n");

    let choices = vec![
        "About yourself (personal context)",
        "About a project (current project context)",
        "General knowledge (long-term memory)",
    ];

    let selection = Select::new()
        .with_prompt("What type of knowledge is this?")
        .items(&choices)
        .default(0)
        .interact()?;

    let knowledge: String = Input::new()
        .with_prompt("What would you like to teach me?")
        .interact()?;

    if knowledge.trim().is_empty() {
        println!("{}", style("❌ No knowledge provided").red());
        return Ok(());
    }

    let (file_path, context_type) = match selection {
        0 => {
            // About yourself - append to USER.md
            let home = imp_home()?;
            (home.join("USER.md"), "personal context".to_string())
        }
        1 => {
            // About current project - append to project CONTEXT.md
            let cwd = std::env::current_dir()?;
            let project_info = project::detect_project(&cwd);
            
            if let Some(proj) = project_info {
                let home = imp_home()?;
                let project_dir = home.join("projects").join(&proj.name);
                fs::create_dir_all(&project_dir)?;
                (project_dir.join("CONTEXT.md"), format!("project context for {}", proj.name))
            } else {
                println!("{}", style("❌ No project detected in current directory").red());
                println!("Run 'imp project add' first to register this project.");
                return Ok(());
            }
        }
        2 => {
            // General knowledge - append to MEMORY.md
            let home = imp_home()?;
            (home.join("MEMORY.md"), "long-term memory".to_string())
        }
        _ => unreachable!(),
    };

    // Read existing content
    let existing_content = if file_path.exists() {
        fs::read_to_string(&file_path)?
    } else {
        String::new()
    };

    // Append new knowledge
    let new_content = if existing_content.trim().is_empty() {
        knowledge.clone()
    } else {
        format!("{}\n\n{}", existing_content.trim(), knowledge)
    };

    // Write back to file
    fs::write(&file_path, new_content)?;

    println!("\n{}", style("✅ Knowledge added successfully!").green());
    println!("Updated {} with new {}.", file_path.display(), context_type);
    
    // Show what was written
    println!("\n{}", style("Added:").bold());
    println!("{}", style(format!("\"{}\"", knowledge)).cyan());

    Ok(())
}