use crate::config::{AuthMethod, Config};
use crate::error::Result;
use console::style;
use dialoguer::{Confirm, Password};

pub async fn run() -> Result<()> {
    println!("{}", style("🔐 Login").bold().blue());
    println!("Set up authentication for imp.\n");

    // Check if already configured
    if let Ok(config) = Config::load() {
        match config.auth_method() {
            AuthMethod::OAuth => {
                println!("{}", style("ℹ️  You're currently using OAuth authentication.").cyan());
            }
            AuthMethod::ApiKey => {
                println!("{}", style("ℹ️  You're currently using API key authentication.").cyan());
            }
        }
        
        let re_auth = Confirm::new()
            .with_prompt("Do you want to update your authentication?")
            .default(false)
            .interact()?;
        
        if !re_auth {
            println!("Login cancelled.");
            return Ok(());
        }
    }

    // Simple token input flow
    println!("\n{}", style("Authentication Setup").bold());
    println!("To authenticate with Anthropic, you need a token.\n");
    println!("{}", style("Getting your token:").bold());
    println!("1. Install Claude Code CLI: https://claude.ai/code");
    println!("2. Run: claude setup-token");
    println!("3. Copy the token that appears");
    println!("4. Paste it below");
    println!("   (OAuth tokens start with sk-ant-oat, API keys start with sk-ant-api)\n");
    
    let token = loop {
        let input_token = Password::new()
            .with_prompt("Enter your Anthropic token")
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
    
    // Load or create config
    let mut config = Config::load().unwrap_or_else(|_| Config {
        llm: crate::config::LlmConfig {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-5-20251101".to_string(),
            max_tokens: 16384,
            api_key: None,
        },
        auth: Default::default(),
    });
    
    // Auto-detect token type and configure
    config.setup_token_auto_detect(token)?;
    
    println!("\n{}", style("✅ Success!").bold().green());
    match config.auth_method() {
        AuthMethod::OAuth => {
            println!("OAuth authentication configured! You can now use imp with your Claude subscription:");
        }
        AuthMethod::ApiKey => {
            println!("API key authentication configured! You can now use imp:");
        }
    }
    println!("  {} — Ask a question", style("imp ask \"hello\"").cyan());
    println!("  {} — Start a chat", style("imp chat").cyan());

    Ok(())
}