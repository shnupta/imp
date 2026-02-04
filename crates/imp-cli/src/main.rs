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
mod compaction;
mod config;
mod context;
mod db;
mod error;
mod highlight;
mod knowledge;
mod logging;
mod project;
mod subagent;
mod tools;
mod usage;

use cli::{bootstrap, chat, knowledge_cmd, learn, login, oneshot, project_cmd, reflect};

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
    /// Login with OAuth (for Claude Pro/Max subscriptions) or re-authenticate
    Login,
    /// Ask your agent a question or give it a task
    Ask {
        /// The question or task
        #[arg(required = true)]
        message: Vec<String>,
    },
    /// Start an interactive chat session
    Chat {
        /// Show session picker to resume a previous session
        #[arg(long, short)]
        resume: bool,

        /// Continue the most recent session (for the current project)
        #[arg(long, short = 'c')]
        r#continue: bool,

        /// Resume a specific session by ID (full or prefix)
        #[arg(long)]
        session: Option<String>,
    },
    /// Teach your agent something new
    Learn,
    /// Distill daily interaction logs into long-term memory
    Reflect {
        /// Date to reflect on (YYYY-MM-DD, default: today)
        #[arg(short, long)]
        date: Option<String>,
    },
    /// List available syntax highlighting themes
    Themes,
    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Manage the knowledge graph
    Knowledge {
        #[command(subcommand)]
        command: KnowledgeCommands,
    },
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Show entity/relationship/chunk counts
    Stats,
    /// Show current schema types and relationship types
    Schema,
    /// Look up an entity by name and show relationships
    Query {
        /// Entity name to look up
        name: String,
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
    logging::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Bootstrap => {
            bootstrap::run().await?;
        }
        Commands::Login => {
            login::run().await?;
        }
        Commands::Ask { message } => {
            let full_message = message.join(" ");
            oneshot::run(&full_message).await?;
        }
        Commands::Chat { resume, r#continue, session } => {
            chat::run(resume, r#continue, session).await?;
        }
        Commands::Learn => {
            learn::run().await?;
        }
        Commands::Reflect { date } => {
            reflect::run(date).await?;
        }
        Commands::Themes => {
            let mut themes = highlight::available_themes();
            themes.sort();
            println!("Available syntax highlighting themes:\n");
            for theme in &themes {
                println!("  {}", theme);
            }
            println!("\nSet in config.toml:\n\n  [display]\n  theme = \"{}\"", themes.first().unwrap_or(&"base16-ocean.dark".to_string()));
        }
        Commands::Knowledge { command } => match command {
            KnowledgeCommands::Stats => {
                knowledge_cmd::stats()?;
            }
            KnowledgeCommands::Schema => {
                knowledge_cmd::schema()?;
            }
            KnowledgeCommands::Query { name } => {
                knowledge_cmd::query(&name)?;
            }
        },
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
