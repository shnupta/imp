use crate::config::{Config, LlmConfig, WorkspaceConfig};
use crate::context::ContextManager;
use crate::error::Result;
use console::style;
use dialoguer::{Input, Password};
use std::env;
use std::fs;

pub async fn run() -> Result<()> {
    println!("{}", style("🚀 Welcome to Imp Setup!").bold().blue());
    println!("Let's get your AI agent configured and ready to help.\n");

    // Check if config already exists
    let config_path = Config::config_path()?;
    if config_path.exists() {
        println!("{}", style("⚠️  Config already exists!").yellow());
        println!("Found existing config at: {}", config_path.display());
        
        let overwrite: bool = dialoguer::Confirm::new()
            .with_prompt("Do you want to overwrite it?")
            .default(false)
            .interact()?;
            
        if !overwrite {
            println!("Setup cancelled. Your existing config is unchanged.");
            return Ok(());
        }
    }

    // Step 1: Agent Identity
    println!("{}", style("1. Agent Identity").bold());
    println!("First, let's give your agent a name and personality.");
    
    let agent_name: String = Input::new()
        .with_prompt("What do you want to name your agent?")
        .default("Imp".to_string())
        .interact()?;

    let agent_personality: String = Input::new()
        .with_prompt("Describe your agent's personality (optional)")
        .default("A helpful AI assistant for engineering teams, focused on coding and problem-solving.".to_string())
        .interact()?;

    // Step 2: API Key
    println!("\n{}", style("2. API Configuration").bold());
    println!("You'll need an Anthropic API key to use Claude.");
    println!("Get one at: https://console.anthropic.com/");
    
    let api_key: String = Password::new()
        .with_prompt("Enter your Anthropic API key")
        .interact()?;

    // Validate API key format (basic check)
    if !api_key.starts_with("sk-ant-") {
        println!("{}", style("⚠️  Warning: API key doesn't look like an Anthropic key (should start with 'sk-ant-')").yellow());
    }

    // Step 3: Model Selection
    println!("\n{}", style("3. Model Selection").bold());
    let models = vec![
        "claude-3-sonnet-20240229",
        "claude-3-haiku-20240307",
        "claude-3-opus-20240229",
    ];
    
    let model_selection = dialoguer::Select::new()
        .with_prompt("Choose your default model")
        .default(0)
        .items(&models)
        .interact()?;
    
    let selected_model = models[model_selection].to_string();

    // Step 4: Workspace
    println!("\n{}", style("4. Workspace Setup").bold());
    let current_dir = env::current_dir()?;
    let default_repos_dir = current_dir.parent()
        .unwrap_or(&current_dir)
        .to_string_lossy()
        .to_string();
    
    let repos_dir: String = Input::new()
        .with_prompt("Where are your code repositories?")
        .default(default_repos_dir)
        .interact()?;

    // Step 5: Create config
    println!("\n{}", style("5. Saving Configuration").bold());
    
    let config = Config {
        llm: LlmConfig {
            provider: "anthropic".to_string(),
            api_key,
            model: selected_model,
        },
        workspace: WorkspaceConfig {
            repos_dir,
        },
    };

    config.save()?;
    println!("✅ Configuration saved to: {}", config_path.display());

    // Step 6: Create context directory and files
    println!("\n{}", style("6. Setting up Context").bold());
    
    let context_dir = current_dir.join("context");
    let context_manager = ContextManager::new(&context_dir);
    
    context_manager.create_context_directory()?;
    context_manager.create_template_files()?;

    // Create IDENTITY.md
    let identity_content = format!(
        "# {}\n\nYou are {}, an AI agent for engineering teams.\n\n## Personality\n{}\n\n## Role\nYou help with:\n- Code review and development\n- Architecture discussions\n- Problem solving\n- Task automation\n- Documentation\n\n## Style\n- Be concise but thorough\n- Ask clarifying questions when needed\n- Provide actionable advice\n- Use the available tools to help accomplish tasks\n",
        agent_name, agent_name, agent_personality
    );
    
    context_manager.write_file("IDENTITY", &identity_content)?;
    println!("✅ Created context files in: {}", context_dir.display());

    // Step 7: Create tools directory
    println!("\n{}", style("7. Setting up Tools").bold());
    let tools_dir = current_dir.join("tools");
    fs::create_dir_all(&tools_dir)?;
    println!("✅ Created tools directory: {}", tools_dir.display());
    println!("   (Built-in tools are available by default)");

    // Step 8: Success and next steps
    println!("\n{}", style("🎉 Setup Complete!").bold().green());
    println!("\nYour agent '{}' is ready to help!", style(&agent_name).bold());
    
    println!("\n{}", style("Quick Start:").bold());
    println!("  {} - Ask a question", style("imp ask \"What files are in this directory?\"").cyan());
    println!("  {} - Start an interactive chat", style("imp chat").cyan());
    
    println!("\n{}", style("Context Files:").bold());
    println!("  Edit these files to customize your agent:");
    println!("  • {}/IDENTITY.md - Agent personality and role", context_dir.display());
    println!("  • {}/STACK.md - Your tech stack", context_dir.display());
    println!("  • {}/PRINCIPLES.md - Coding principles", context_dir.display());
    println!("  • {}/ARCHITECTURE.md - System architecture", context_dir.display());

    println!("\n{}", style("Need help?").bold());
    println!("  Run any command to see your agent in action!");

    Ok(())
}