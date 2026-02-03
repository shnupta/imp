use crate::config::{AuthMethod, Config};
use crate::claude_code;
use crate::error::Result;
use console::style;
use dialoguer::{Confirm, Password};

pub async fn run() -> Result<()> {
    println!("{}", style("🔐 Login").bold().blue());
    println!("Set up authentication for imp.\n");

    // Check if already configured
    if let Ok(config) = Config::load() {
        if config.auth_method() == &AuthMethod::OAuth {
            println!("{}", style("ℹ️  You're already using OAuth authentication.").cyan());
            
            let re_auth = Confirm::new()
                .with_prompt("Do you want to re-authenticate?")
                .default(false)
                .interact()?;
            
            if !re_auth {
                println!("Login cancelled.");
                return Ok(());
            }
        } else {
            println!("{}", style("ℹ️  You're currently using API key authentication.").cyan());
            println!("Switching to OAuth will let you use your Claude Pro/Max subscription.\n");
            
            let switch = Confirm::new()
                .with_prompt("Switch from API key to OAuth authentication?")
                .default(false)
                .interact()?;
            
            if !switch {
                println!("Login cancelled.");
                return Ok(());
            }
        }
    }

    // Check for Claude Code credentials first
    if claude_code::claude_code_credentials_exist() {
        println!("{}", style("1. Found Claude Code credentials!").bold().green());
        println!("We found existing Claude Code OAuth credentials on your system.");
        
        let use_claude_code = Confirm::new()
            .with_prompt("Import your Claude Code OAuth credentials?")
            .default(true)
            .interact()?;

        if use_claude_code {
            match claude_code::read_claude_code_credentials() {
                Ok(credentials) => {
                    let oauth_config = claude_code::to_oauth_config(&credentials);
                    
                    // Save tokens to config
                    let mut config = Config::load().unwrap_or_else(|_| Config {
                        llm: crate::config::LlmConfig {
                            provider: "anthropic".to_string(),
                            model: "claude-opus-4-5-20251101".to_string(),
                            api_key: None,
                        },
                        auth: Default::default(),
                    });

                    config.update_oauth_tokens(
                        oauth_config.access_token,
                        oauth_config.refresh_token,
                        oauth_config.expires_at,
                    )?;

                    println!("\n{}", style("✅ Success!").bold().green());
                    println!("Imported OAuth credentials from Claude Code.");
                    println!("You can now use imp with your Claude Pro/Max subscription:");
                    println!("  {} — Ask a question", style("imp ask \"hello\"").cyan());
                    println!("  {} — Start a chat", style("imp chat").cyan());
                    
                    return Ok(());
                }
                Err(e) => {
                    println!("{}", style(format!("❌ Failed to read Claude Code credentials: {}", e)).red());
                    println!("Falling back to API key authentication.");
                }
            }
        }
    } else {
        println!("{}", style("ℹ️  No Claude Code credentials found.").cyan());
        println!("Claude Code credentials would be at: ~/.claude/.credentials.json");
    }

    // Fall back to API key authentication
    println!("\n{}", style("2. API Key Authentication").bold());
    println!("Enter your Anthropic API key. Get one at: https://console.anthropic.com/\n");
    
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
        
        // Save API key to config
        let mut config = Config::load().unwrap_or_else(|_| Config {
            llm: crate::config::LlmConfig {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-5-20251101".to_string(),
                api_key: None,
            },
            auth: Default::default(),
        });

        config.auth.method = AuthMethod::ApiKey;
        config.auth.api_key = Some(crate::config::ApiKeyConfig { key });
        config.auth.oauth = None;
        config.save()?;

        println!("\n{}", style("✅ Success!").bold().green());
        println!("API key authentication configured successfully!");
        println!("You can now use imp:");
        println!("  {} — Ask a question", style("imp ask \"hello\"").cyan());
        println!("  {} — Start a chat", style("imp chat").cyan());
        
        break;
    }

    Ok(())
}