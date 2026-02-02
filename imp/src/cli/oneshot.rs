use crate::agent::Agent;
use crate::error::Result;
use console::style;

pub async fn run(message: &str) -> Result<()> {
    println!("{}", style("🤖 Imp").bold().blue());
    println!("{}", style("─".repeat(50)).dim());
    
    let mut agent = Agent::new().await?;
    
    // Show context files being used
    let context_files = agent.get_context_files();
    if !context_files.is_empty() {
        println!("{}", style(format!("📚 Loaded context: {}", context_files.join(", "))).dim());
        println!();
    }

    let response = agent.process_message(message, true).await?;
    
    println!("\n{}", style("─".repeat(50)).dim());
    
    Ok(())
}