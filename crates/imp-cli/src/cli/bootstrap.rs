use crate::config::{imp_home, Config, LlmConfig};
use crate::error::Result;
use console::style;
use dialoguer::{Confirm, Input, Password};
use std::fs;

pub async fn run() -> Result<()> {
    println!("{}", style("🚀 Welcome to Imp Bootstrap!").bold().blue());
    println!("Let's get your AI agent configured.\n");

    let home = imp_home()?;

    // Check if already initialised
    let config_path = home.join("config.toml");
    if config_path.exists() {
        println!("{}", style("⚠️  Imp is already set up!").yellow());
        println!("Found config at: {}", config_path.display());

        let overwrite = Confirm::new()
            .with_prompt("Do you want to re-initialize?")
            .default(false)
            .interact()?;

        if !overwrite {
            println!("Setup cancelled.");
            return Ok(());
        }
    }

    // ── 1. API Key ───────────────────────────────────────────────────
    println!("{}", style("1. API Configuration").bold());
    println!(
        "You need an Anthropic API key. Get one at: https://console.anthropic.com/\n"
    );

    let api_key: String = loop {
        let key = Password::new()
            .with_prompt("Enter your Anthropic API key")
            .interact()?;
        
        if key.trim().is_empty() {
            println!("{}", style("❌ API key cannot be empty").red());
            continue;
        }
        
        if !key.starts_with("sk-ant-") {
            println!(
                "{}",
                style("⚠️  Warning: key doesn't look like an Anthropic key (should start with 'sk-ant-')")
                    .yellow()
            );
            
            let continue_anyway = Confirm::new()
                .with_prompt("Continue with this key anyway?")
                .default(false)
                .interact()?;
            
            if !continue_anyway {
                continue;
            }
        }
        
        break key;
    };

    // ── 2. Agent Identity ────────────────────────────────────────────
    println!("\n{}", style("2. Agent Identity").bold());

    let agent_name: String = Input::new()
        .with_prompt("What do you want to name your agent?")
        .default("Imp".to_string())
        .interact()?;

    let persona: String = Input::new()
        .with_prompt("Brief personality/style (or press Enter for default)")
        .default(
            "A helpful, direct AI assistant that learns and adapts over time.".to_string(),
        )
        .interact()?;

    // ── 3. User Information ──────────────────────────────────────────
    println!("\n{}", style("3. About You").bold());
    println!("Help your agent understand who you are and how you work.\n");

    let user_name: String = Input::new()
        .with_prompt("What's your name?")
        .interact()?;

    let preferred_name: String = Input::new()
        .with_prompt("What should your agent call you?")
        .default(user_name.clone())
        .interact()?;

    let user_role: String = Input::new()
        .with_prompt("What's your role? (e.g., Software Engineer, DevOps, Product Manager)")
        .default("Software Engineer".to_string())
        .interact()?;

    let communication_style: String = Input::new()
        .with_prompt("Communication preference")
        .default("Direct and concise".to_string())
        .interact()?;

    let work_schedule: String = Input::new()
        .with_prompt("Typical work hours (e.g., 9-5 EST, flexible)")
        .default("9-5 local time".to_string())
        .interact()?;

    // ── 4. Save config + core files ──────────────────────────────────
    println!("\n{}", style("4. Saving Configuration").bold());
    fs::create_dir_all(&home)?;
    fs::create_dir_all(home.join("projects"))?;
    fs::create_dir_all(home.join("memory"))?;

    let config = Config {
        llm: LlmConfig {
            provider: "anthropic".to_string(),
            api_key,
            model: "claude-opus-4-5-20250514".to_string(),
        },
    };
    config.save()?;
    println!("  ✅ config.toml");

    // IDENTITY.md
    let identity_content = include_str!("../../../../templates/global/IDENTITY.md")
        .replace("{{name}}", &agent_name)
        .replace("{{persona}}", &persona);
    fs::write(home.join("IDENTITY.md"), identity_content)?;
    println!("  ✅ IDENTITY.md");

    // MEMORY.md
    fs::write(
        home.join("MEMORY.md"),
        include_str!("../../../../templates/global/MEMORY.md"),
    )?;
    println!("  ✅ MEMORY.md");

    // USER.md
    let user_content = include_str!("../../../../templates/global/USER.md")
        .replace("{{user_name}}", &user_name)
        .replace("{{preferred_name}}", &preferred_name)
        .replace("{{user_role}}", &user_role)
        .replace("{{communication_style}}", &communication_style)
        .replace("{{work_schedule}}", &work_schedule)
        .replace("{{timezone}}", "Local timezone"); // TODO: could detect this automatically
    fs::write(home.join("USER.md"), user_content)?;
    println!("  ✅ USER.md");

    // ── 5. Optional engineering context ──────────────────────────────
    println!(
        "\n{}",
        style("5. Engineering Context (optional)").bold()
    );
    println!("Engineering context files help your agent understand your tech stack,");
    println!("coding principles, and architecture across all projects.\n");

    let setup_engineering = Confirm::new()
        .with_prompt("Would you like to set up engineering context?")
        .default(false)
        .interact()?;

    if setup_engineering {
        for (file, content) in [
            (
                "STACK.md",
                include_str!("../../../../templates/global/STACK.md"),
            ),
            (
                "PRINCIPLES.md",
                include_str!("../../../../templates/global/PRINCIPLES.md"),
            ),
            (
                "ARCHITECTURE.md",
                include_str!("../../../../templates/global/ARCHITECTURE.md"),
            ),
        ] {
            let path = home.join(file);
            if !path.exists() {
                fs::write(&path, content)?;
                println!("  ✅ {}", file);
            }
        }
        println!("\nFill these in to give your agent deep understanding of your work.");
    } else {
        println!("Skipped. You can create STACK.md, PRINCIPLES.md, and ARCHITECTURE.md");
        println!("in {} at any time.", home.display());
    }

    // ── Done ─────────────────────────────────────────────────────────
    println!("\n{}", style("🎉 Setup Complete!").bold().green());
    println!(
        "\nYour agent '{}' is ready. Use it anywhere:\n",
        style(&agent_name).bold()
    );
    println!(
        "  {} — Ask a question",
        style("imp ask \"what files are in this project?\"").cyan()
    );
    println!(
        "  {} — Start an interactive chat",
        style("imp chat").cyan()
    );
    println!(
        "  {} — Show registered projects",
        style("imp project list").cyan()
    );

    Ok(())
}
