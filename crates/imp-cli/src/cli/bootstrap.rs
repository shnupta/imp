use crate::config::{imp_home, AuthConfig, AuthMethod, ApiKeyConfig, Config, LlmConfig, OAuthConfig};
use crate::error::Result;
use anthropic_auth::{AsyncOAuthClient, OAuthConfig as AuthOAuthConfig, OAuthMode, open_browser};
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
    println!("Choose how you want to authenticate with Anthropic:\n");
    
    let auth_methods = vec![
        "OAuth (Claude Pro/Max subscription) - Recommended",
        "API Key (Pay-per-token)",
    ];

    let auth_choice = dialoguer::Select::new()
        .with_prompt("Select authentication method")
        .items(&auth_methods)
        .default(0)
        .interact()?;

    let (auth_method, oauth_config, api_key) = if auth_choice == 0 {
        // OAuth flow
        println!("\n{}", style("Setting up OAuth authentication...").cyan());
        println!("This will use your Claude Pro/Max subscription.\n");
        
        let oauth_config = setup_oauth_auth().await?;
        (crate::config::AuthMethod::OAuth, Some(oauth_config), None)
    } else {
        // API Key flow  
        println!("\n{}", style("Setting up API key authentication...").cyan());
        println!("You need an Anthropic API key. Get one at: https://console.anthropic.com/\n");
        
        let api_key = setup_api_key_auth()?;
        (crate::config::AuthMethod::ApiKey, None, Some(api_key))
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
            model: "claude-opus-4-5-20251101".to_string(),
            api_key: None, // Legacy field - not used in new format
        },
        auth: if auth_method == AuthMethod::OAuth {
            AuthConfig {
                method: AuthMethod::OAuth,
                oauth: oauth_config,
                api_key: None,
            }
        } else {
            AuthConfig {
                method: AuthMethod::ApiKey,
                oauth: None,
                api_key: Some(ApiKeyConfig {
                    key: api_key.unwrap(), // Safe because we set it in API key flow
                }),
            }
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

async fn setup_oauth_auth() -> Result<OAuthConfig> {
    let oauth_client = AsyncOAuthClient::new(AuthOAuthConfig::default())
        .map_err(|e| crate::error::ImpError::Config(format!("Failed to initialize OAuth client: {}", e)))?;

    let flow = oauth_client.start_flow(OAuthMode::Max)
        .map_err(|e| crate::error::ImpError::Config(format!("Failed to start OAuth flow: {}", e)))?;

    println!("Opening your browser to authenticate with Claude...");
    
    // Attempt to open browser
    if let Err(_) = open_browser(&flow.authorization_url) {
        println!("{}", style("⚠️  Could not open browser automatically").yellow());
        println!("Please manually visit: {}", flow.authorization_url);
    } else {
        println!("✅ Opened browser");
    }
    
    println!("\nAfter authorizing, you'll be redirected to a page that may show an error.");
    println!("That's normal! Look at the URL bar and copy the part after 'code='");
    println!("Example: if the URL is 'http://localhost:8080/?code=abc123#state456'");
    println!("Copy: 'abc123#state456'");
    
    let auth_response: String = loop {
        let response = Input::<String>::new()
            .with_prompt("Paste the authorization response (code#state)")
            .interact()?;
        
        if response.trim().is_empty() {
            println!("{}", style("❌ Response cannot be empty").red());
            continue;
        }
        
        break response;
    };

    println!("Exchanging code for tokens...");
    
    let tokens = oauth_client
        .exchange_code(&auth_response, &flow.state, &flow.verifier)
        .await
        .map_err(|e| crate::error::ImpError::Agent(format!("Failed to exchange authorization code: {}", e)))?;

    println!("✅ Successfully obtained access tokens!");

    Ok(OAuthConfig {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at as i64,
    })
}

fn setup_api_key_auth() -> Result<String> {
    loop {
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
        
        break Ok(key);
    }
}
