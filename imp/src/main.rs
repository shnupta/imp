use anyhow::Result;
use clap::{Parser, Subcommand};

mod agent;
mod cli;
mod client;
mod config;
mod context;
mod error;
mod tools;

use cli::{bootstrap, chat, oneshot};

#[derive(Parser)]
#[command(name = "imp")]
#[command(about = "A Rust-based AI agent CLI for engineering teams")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap wizard - set up your agent for the first time
    Init,
    /// Ask your agent a question or give it a task
    Ask { 
        /// The question or task to give your agent
        #[arg(required = true)]
        message: Vec<String> 
    },
    /// Start an interactive chat session
    Chat,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            bootstrap::run().await?;
        }
        Commands::Ask { message } => {
            let full_message = message.join(" ");
            oneshot::run(&full_message).await?;
        }
        Commands::Chat => {
            chat::run().await?;
        }
    }

    Ok(())
}