//! # Imp - AI Agent CLI
//!
//! A personal AI agent that learns across your projects, providing intelligent
//! assistance for coding, documentation, and task automation.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod agent;
mod cli;
mod client;
mod config;
mod context;
mod error;
mod project;
mod tools;

use cli::{bootstrap, chat, oneshot, project_cmd};

#[derive(Parser)]
#[command(name = "imp")]
#[command(about = "A personal AI agent that learns across your projects")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "Your Name <your.email@example.com>")]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap wizard — set up your agent for the first time
    Bootstrap,
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
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// List all registered projects
    List,
    /// Register a project (defaults to current directory)
    Add {
        /// Path to the project directory
        path: Option<PathBuf>,
    },
    /// Show context summary for the current project
    Context,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Bootstrap => {
            bootstrap::run().await?;
        }
        Commands::Ask { message } => {
            let full_message = message.join(" ");
            oneshot::run(&full_message).await?;
        }
        Commands::Chat => {
            chat::run().await?;
        }
        Commands::Project { command } => match command {
            ProjectCommands::List => {
                project_cmd::list()?;
            }
            ProjectCommands::Add { path } => {
                project_cmd::add(path)?;
            }
            ProjectCommands::Context => {
                project_cmd::context()?;
            }
        },
    }

    Ok(())
}
