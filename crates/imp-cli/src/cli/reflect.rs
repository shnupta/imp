use anyhow::Result;

use crate::client::{ClaudeClient, Message};
use crate::config::{imp_home, Config};

pub async fn run(date: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let mut client = ClaudeClient::new(config)?;
    let home = imp_home()?;

    // Determine which daily file to reflect on
    let target_date =
        date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let daily_file = home.join("memory").join(format!("{}.md", target_date));

    if !daily_file.exists() {
        println!("No memory file found for {}", target_date);
        return Ok(());
    }

    let daily_content = std::fs::read_to_string(&daily_file)?;
    let memory_content =
        std::fs::read_to_string(home.join("MEMORY.md")).unwrap_or_default();

    println!("🧠 Reflecting on {}...", target_date);

    let system_prompt = "\
You are a knowledge distillation system. Your job is to extract meaningful \
insights from a day's interaction logs and produce an updated long-term memory file.\n\
\n\
Rules:\n\
- Extract actual insights: preferences discovered, patterns noticed, decisions made, lessons learned\n\
- Ignore noise: tool counts, timestamps, routine operations\n\
- Merge with existing memory: deduplicate, update stale info, add new insights\n\
- Keep it structured with clear sections\n\
- Be concise but preserve important nuance\n\
- Return ONLY the complete updated MEMORY.md content"
        .to_string();

    let user_message = format!(
        "Here's the current MEMORY.md:\n\n---\n{}\n---\n\n\
         Here are today's ({}) interaction notes:\n\n---\n{}\n---\n\n\
         Produce an updated MEMORY.md that incorporates any valuable insights from today.",
        memory_content, target_date, daily_content
    );

    let messages = vec![Message::text("user", &user_message)];
    let response = client
        .send_message(messages, Some(system_prompt), None, false)
        .await?;
    let updated = client.extract_text_content(&response);

    if !updated.trim().is_empty() {
        std::fs::write(home.join("MEMORY.md"), updated.trim())?;
        println!("✅ MEMORY.md updated with insights from {}", target_date);
    } else {
        println!("No significant insights to extract.");
    }

    Ok(())
}
