use crate::config::{AuthMethod, Config};
use crate::error::Result;
use anthropic_auth::{AsyncOAuthClient, OAuthConfig as AuthOAuthConfig, OAuthMode, open_browser};
use console::style;
use dialoguer::{Confirm, Input};

pub async fn run() -> Result<()> {
    println!("{}", style("🔐 OAuth Login").bold().blue());
    println!("Authenticate with your Claude Pro/Max subscription using OAuth.\n");

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

    // Run OAuth flow
    println!("{}", style("1. Starting OAuth Flow").bold());
    
    let oauth_client = AsyncOAuthClient::new(AuthOAuthConfig::default())
        .map_err(|e| crate::error::ImpError::Config(format!("Failed to initialize OAuth client: {}", e)))?;

    let flow = oauth_client.start_flow(OAuthMode::Max)
        .map_err(|e| crate::error::ImpError::Config(format!("Failed to start OAuth flow: {}", e)))?;

    println!("\n{}", style("2. Authorization").bold());
    println!("Opening your browser to authenticate with Claude...");
    
    // Attempt to open browser
    if let Err(_) = open_browser(&flow.authorization_url) {
        println!("{}", style("⚠️  Could not open browser automatically").yellow());
        println!("Please manually visit: {}", flow.authorization_url);
    } else {
        println!("✅ Opened browser");
    }
    
    println!("\n{}", style("3. Get Authorization Code").bold());
    println!("After authorizing, you'll be redirected to a page that may show an error.");
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

    println!("\n{}", style("4. Exchanging Code for Tokens").bold());
    
    let tokens = oauth_client
        .exchange_code(&auth_response, &flow.state, &flow.verifier)
        .await
        .map_err(|e| crate::error::ImpError::Agent(format!("Failed to exchange authorization code: {}", e)))?;

    println!("✅ Successfully obtained access tokens!");

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
        tokens.access_token,
        tokens.refresh_token,
        tokens.expires_at as i64,
    )?;

    println!("{}", style("5. Configuration Saved").bold());
    println!("✅ OAuth authentication configured successfully!");
    println!("\nYou can now use imp with your Claude Pro/Max subscription:");
    println!("  {} — Ask a question", style("imp ask \"hello\"").cyan());
    println!("  {} — Start a chat", style("imp chat").cyan());

    Ok(())
}