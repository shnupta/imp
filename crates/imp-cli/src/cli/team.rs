use crate::error::Result;
use console::style;
use std::fs;

pub async fn init() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let team_dir = cwd.join(".imp");

    if team_dir.exists() {
        println!(
            "{}",
            style("⚠️  .imp/ already exists in this repo.").yellow()
        );
        return Ok(());
    }

    fs::create_dir_all(&team_dir)?;

    let templates: [(&str, &str); 5] = [
        ("STACK.md", include_str!("../../../../templates/team/STACK.md")),
        (
            "PRINCIPLES.md",
            include_str!("../../../../templates/team/PRINCIPLES.md"),
        ),
        (
            "ARCHITECTURE.md",
            include_str!("../../../../templates/team/ARCHITECTURE.md"),
        ),
        (
            "GOTCHAS.md",
            include_str!("../../../../templates/team/GOTCHAS.md"),
        ),
        ("TEAM.md", include_str!("../../../../templates/team/TEAM.md")),
    ];

    for (filename, content) in templates {
        fs::write(team_dir.join(filename), content)?;
        println!("  ✅ .imp/{}", filename);
    }

    println!(
        "\n{}",
        style("Team context initialized!").bold().green()
    );
    println!("Edit the files in .imp/ and commit them to share with your team.");
    println!("Every team member's agent will pick them up automatically.");

    Ok(())
}
