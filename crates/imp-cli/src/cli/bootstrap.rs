use crate::config::{imp_home, AuthConfig, Config, LlmConfig};
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

    // ── 1. Authentication Method ─────────────────────────────────────
    println!("{}", style("1. Authentication Method").bold());
    println!("To use imp, you need an Anthropic token.\n");
    println!("{}", style("Getting your token:").bold());
    println!("1. Install Claude Code CLI: https://claude.ai/code");
    println!("2. Run: claude setup-token");
    println!("3. Copy the token that appears");
    println!("4. Paste it below\n");
    
    let token = loop {
        let input_token = Password::new()
            .with_prompt("Enter your Anthropic token (from 'claude setup-token')")
            .interact()?;
        
        if input_token.trim().is_empty() {
            println!("{}", style("❌ Token cannot be empty").red());
            continue;
        }
        
        if !input_token.starts_with("sk-ant-") {
            println!(
                "{}",
                style("⚠️  Warning: token doesn't look like an Anthropic token (should start with 'sk-ant-')")
                    .yellow()
            );
            
            let continue_anyway = Confirm::new()
                .with_prompt("Continue with this token anyway?")
                .default(false)
                .interact()?;
            
            if !continue_anyway {
                continue;
            }
        }
        
        break input_token;
    };

    // ── 2. Agent Identity ────────────────────────────────────────────
    println!("\n{}", style("2. Your Agent").bold());

    let agent_name: String = Input::new()
        .with_prompt("Name your agent")
        .default("Imp".to_string())
        .interact()?;

    println!("\n  Describe how your agent should act.");
    println!("  {}", style("e.g. strengths, focuses, communication style, personality").dim());
    let persona: String = Input::new()
        .with_prompt("Personality")
        .default(
            "Direct and helpful. Learns and adapts over time.".to_string(),
        )
        .interact()?;

    // ── 3. About You ─────────────────────────────────────────────────
    println!("\n{}", style("3. About You").bold());
    println!("  Tell your agent a bit about yourself.");
    println!("  {}", style("e.g. what you work on, what you care about, things you're responsible for").dim());
    let about_user: String = Input::new()
        .with_prompt("About you")
        .interact()?;

    // ── 4. Save config + core files ──────────────────────────────────
    println!("\n{}", style("Saving configuration...").dim());
    fs::create_dir_all(&home)?;
    fs::create_dir_all(home.join("projects"))?;
    fs::create_dir_all(home.join("memory"))?;

    let mut config = Config {
        llm: LlmConfig {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 16384,
            api_key: None, // Legacy field - not used in new format
        },
        auth: AuthConfig::default(),
        thinking: Default::default(),
    };
    
    // Auto-detect token type and configure auth
    config.setup_token_auto_detect(token)?;
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
        .replace("{{about_user}}", &about_user);
    fs::write(home.join("USER.md"), user_content)?;
    println!("  ✅ USER.md");

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


