use crate::agent::Agent;
use crate::error::Result;
use console::style;

pub async fn run(message: &str) -> Result<()> {
    println!("{}", style("🤖 Imp").bold().blue());
    println!("{}", style("─".repeat(50)).dim());

    let mut agent = Agent::new().await?;

    if let Some(name) = agent.project_name() {
        println!("{}", style(format!("📂 Project: {}", name)).dim());
    }

    let sections = agent.loaded_sections();
    if !sections.is_empty() {
        println!(
            "{}",
            style(format!("📚 Context: {}", sections.join(", "))).dim()
        );
    }
    println!();

    let _response = agent.process_message(message, true).await?;

    println!("\n{}", style("─".repeat(50)).dim());

    Ok(())
}
