use crate::config::{imp_home, Config, LlmConfig};
use crate::error::Result;
use console::style;
use dialoguer::{Confirm, Input, Password};
use std::fs;

pub async fn run() -> Result<()> {
    println!("{}", style("🚀 Welcome to Imp Setup!").bold().blue());
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

    let api_key: String = Password::new()
        .with_prompt("Enter your Anthropic API key")
        .interact()?;

    if !api_key.starts_with("sk-ant-") {
        println!(
            "{}",
            style("⚠️  Warning: key doesn't look like an Anthropic key (should start with 'sk-ant-')")
                .yellow()
        );
    }

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

    // ── 3. Save config + core files ──────────────────────────────────
    println!("\n{}", style("3. Saving Configuration").bold());
    fs::create_dir_all(&home)?;
    fs::create_dir_all(home.join("projects"))?;
    fs::create_dir_all(home.join("memory"))?;

    let config = Config {
        llm: LlmConfig {
            provider: "anthropic".to_string(),
            api_key,
            model: "claude-sonnet-4-20250514".to_string(),
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

    // ── 4. Optional engineering context ──────────────────────────────
    println!(
        "\n{}",
        style("4. Engineering Context (optional)").bold()
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
