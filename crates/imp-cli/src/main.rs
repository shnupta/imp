use anyhow::Result;
use clap::{Parser, Subcommand};

mod agent;
mod cli;
mod client;
mod config;
mod context;
mod error;
mod project;
mod tools;

use cli::{bootstrap, chat, oneshot};

#[derive(Parser)]
#[command(name = "imp")]
#[command(about = "Your personal AI agent — learns, remembers, adapts")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap wizard — set up your agent for the first time
    Init,
    /// Ask your agent a question or give it a task
    Ask {
        /// The question or task
        #[arg(required = true)]
        message: Vec<String>,
    },
    /// Start an interactive chat session
    Chat,
    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Manage team shared context (per-repo)
    Team {
        #[command(subcommand)]
        command: TeamCommands,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// List all registered projects
    List,
    /// Register a project (defaults to current directory)
    Add {
        /// Path to the project (defaults to cwd)
        path: Option<String>,
    },
    /// Show loaded context summary for current project
    Context,
}

#[derive(Subcommand)]
enum TeamCommands {
    /// Create .imp/ in the current repo with team engineering templates
    Init,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => bootstrap::run().await?,
        Commands::Ask { message } => {
            let full_message = message.join(" ");
            oneshot::run(&full_message).await?;
        }
        Commands::Chat => chat::run().await?,
        Commands::Project { command } => match command {
            ProjectCommands::List => cli::project::list().await?,
            ProjectCommands::Add { path } => cli::project::add(path).await?,
            ProjectCommands::Context => cli::project::context().await?,
        },
        Commands::Team { command } => match command {
            TeamCommands::Init => cli::team::init().await?,
        },
    }

    Ok(())
}
