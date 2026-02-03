use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};
use crate::error::Result;
use crate::project;
use console::style;
use dialoguer::Select;
use std::fs;
use std::io::{self, Write};

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

    print!(
        "{} ",
        style("What would you like to teach me?").bold()
    );
    io::stdout().flush()?;

    let mut knowledge = String::new();
    match io::stdin().read_line(&mut knowledge) {
        Ok(0) => return Ok(()), // EOF
        Ok(_) => {}
        Err(_) => return Ok(()),
    }
    let knowledge = knowledge.trim().to_string();

    if knowledge.is_empty() {
        println!("{}", style("❌ No knowledge provided").red());
        return Ok(());
    }

    let (file_path, context_type) = match selection {
        0 => {
            // About yourself - update USER.md
            let home = imp_home()?;
            (home.join("USER.md"), "personal context")
        }
        1 => {
            // About current project - update project CONTEXT.md
            let cwd = std::env::current_dir()?;
            let project_info = project::detect_project(&cwd);

            if let Some(proj) = project_info {
                let home = imp_home()?;
                let project_dir = home.join("projects").join(&proj.name);
                fs::create_dir_all(&project_dir)?;
                (project_dir.join("CONTEXT.md"), "project context")
            } else {
                println!(
                    "{}",
                    style("❌ No project detected in current directory").red()
                );
                println!("Run 'imp project add' first to register this project.");
                return Ok(());
            }
        }
        2 => {
            // General knowledge - update MEMORY.md
            let home = imp_home()?;
            (home.join("MEMORY.md"), "long-term memory")
        }
        _ => unreachable!(),
    };

    // Read existing content
    let existing_content = if file_path.exists() {
        fs::read_to_string(&file_path)?
    } else {
        String::new()
    };

    println!(
        "{}",
        style("🧠 Processing and organizing...").dim()
    );

    // Use the LLM to intelligently merge knowledge
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config)?;

    let system_prompt = format!(
        "You are a context file manager. The user is teaching you something new about their {}.\n\
        Your job is to intelligently merge this new knowledge into the existing file content.\n\n\
        Rules:\n\
        - Maintain existing structure and formatting\n\
        - Categorize new info into appropriate sections\n\
        - Create new sections if needed\n\
        - Deduplicate — update/replace existing info if the user is correcting something\n\
        - Keep the file clean, well-organized, and in markdown format\n\
        - If the file is empty, create a sensible structure\n\
        - Return ONLY the complete updated file content, no explanations or commentary",
        context_type
    );

    let user_message = if existing_content.trim().is_empty() {
        format!(
            "Create a new {} file with this knowledge:\n\n{}",
            context_type, knowledge
        )
    } else {
        format!(
            "Here's the current file content:\n\n---\n{}\n---\n\nMerge in this new knowledge:\n\n{}",
            existing_content, knowledge
        )
    };

    let messages = vec![Message::text("user", &user_message)];
    let response = client
        .send_message(messages, Some(system_prompt), None, false)
        .await?;
    let updated_content = client.extract_text_content(&response);

    if updated_content.trim().is_empty() {
        // Fallback: if the LLM returned nothing, just append like before
        let new_content = if existing_content.trim().is_empty() {
            knowledge.clone()
        } else {
            format!("{}\n\n{}", existing_content.trim(), knowledge)
        };
        fs::write(&file_path, new_content)?;
        println!(
            "\n{}",
            style("⚠ LLM returned empty response, appended raw text.").yellow()
        );
    } else {
        fs::write(&file_path, updated_content.trim())?;
    }

    println!(
        "\n{}",
        style("✅ Knowledge merged successfully!").green()
    );
    println!(
        "Updated {} with new {}.",
        style(file_path.display().to_string()).cyan(),
        context_type
    );

    Ok(())
}
