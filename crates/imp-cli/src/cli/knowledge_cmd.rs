//! CLI commands for the knowledge graph.
//!
//! Provides `imp knowledge stats`, `imp knowledge schema`, and
//! `imp knowledge query <name>` subcommands.

use crate::error::Result;
use crate::knowledge::KnowledgeGraph;
use console::style;

/// Show entity/relationship/chunk counts.
pub fn stats() -> Result<()> {
    let kg = KnowledgeGraph::open()?;
    let s = kg.stats()?;

    println!("{}", style("Knowledge Graph Stats").bold().cyan());
    println!("  Entities:       {}", s.entity_count);
    println!("  Relationships:  {}", s.relationship_count);
    println!("  Memory chunks:  {}", s.chunk_count);
    println!("  Schema types:   {}", s.schema_type_count);
    println!("  Schema rels:    {}", s.schema_rel_count);

    Ok(())
}

/// Show current schema types and relationship types.
pub fn schema() -> Result<()> {
    let kg = KnowledgeGraph::open()?;
    let info = kg.get_schema()?;

    println!("{}", style("Entity Types").bold().cyan());
    if info.types.is_empty() {
        println!("  (none)");
    } else {
        for t in &info.types {
            let examples = match &t.example_names {
                serde_json::Value::Array(arr) => {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
                _ => String::new(),
            };
            println!("  {} — {}", style(&t.type_name).green(), t.description);
            if !examples.is_empty() {
                println!("    examples: {}", style(examples).dim());
            }
        }
    }

    println!();
    println!("{}", style("Relationship Types").bold().cyan());
    if info.relationships.is_empty() {
        println!("  (none)");
    } else {
        for r in &info.relationships {
            let from = match &r.from_types {
                serde_json::Value::Array(arr) => {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                        .join("|")
                }
                _ => "?".to_string(),
            };
            let to = match &r.to_types {
                serde_json::Value::Array(arr) => {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                        .join("|")
                }
                _ => "?".to_string(),
            };
            println!(
                "  {} — {}",
                style(&r.rel_name).green(),
                r.description
            );
            println!(
                "    {} → {}",
                style(from).dim(),
                style(to).dim()
            );
            if !r.example_usage.is_empty() {
                println!("    e.g. {}", style(&r.example_usage).dim());
            }
        }
    }

    Ok(())
}

/// Look up an entity by name and show its relationships.
pub fn query(name: &str) -> Result<()> {
    let kg = KnowledgeGraph::open()?;

    match kg.find_entity_by_name(name)? {
        Some(entity) => {
            println!(
                "{} ({}) — id: {}",
                style(&entity.name).bold().green(),
                style(&entity.entity_type).cyan(),
                style(&entity.id).dim(),
            );

            if entity.properties != serde_json::Value::Null
                && entity.properties != serde_json::json!({})
                && entity.properties != serde_json::json!("")
            {
                println!("  properties: {}", entity.properties);
            }

            // Get relationships (up to 2 hops)
            let related = kg.get_related(&entity.name, 2)?;
            if related.is_empty() {
                println!("  No relationships found.");
            } else {
                println!();
                println!("  {}", style("Relationships:").bold());
                for r in &related {
                    let arrow = if r.direction == "->" {
                        format!(
                            "{} {} {}",
                            style(&entity.name).green(),
                            style(&r.rel_type).yellow(),
                            style(&r.entity.name).green()
                        )
                    } else {
                        format!(
                            "{} {} {}",
                            style(&r.entity.name).green(),
                            style(&r.rel_type).yellow(),
                            style(&entity.name).green()
                        )
                    };
                    println!(
                        "    {} ({})",
                        arrow,
                        style(&r.entity.entity_type).dim()
                    );
                }
            }
        }
        None => {
            println!(
                "{}",
                style(format!("No entity found matching '{}'", name)).yellow()
            );
        }
    }

    Ok(())
}
