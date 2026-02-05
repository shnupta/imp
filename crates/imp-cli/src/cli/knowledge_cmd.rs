//! CLI commands for the knowledge graph.
//!
//! Provides `imp knowledge stats`, `imp knowledge schema`, and
//! `imp knowledge query <name>` subcommands.

use crate::embeddings::Embedder;
use crate::error::Result;
use crate::knowledge::KnowledgeGraph;
use console::style;

/// Show entity/relationship/chunk counts.
pub fn stats() -> Result<()> {
    let kg = KnowledgeGraph::open()?;
    let s = kg.stats()?;

    // Get embedding-specific stats
    let with_embeddings = kg.count_rows("?[count(id)] := *memory_chunk{id, has_embedding}, has_embedding == true")?;
    let without_embeddings = kg.count_rows("?[count(id)] := *memory_chunk{id, has_embedding}, has_embedding == false")?;

    println!("{}", style("Knowledge Graph Stats").bold().cyan());
    println!("  Entities:       {}", s.entity_count);
    println!("  Relationships:  {}", s.relationship_count);
    println!("  Memory chunks:  {} ({} with embeddings, {} without)", 
        s.chunk_count, with_embeddings, without_embeddings);
    println!("  Schema types:   {}", s.schema_type_count);
    println!("  Schema rels:    {}", s.schema_rel_count);
    
    // Show embedding status
    if Embedder::available() {
        println!("  Embeddings:     {}", style("✓ Available (BGE-large-en-v1.5)").green());
    } else {
        println!("  Embeddings:     {}", style("✗ Unavailable").red());
    }

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

/// Search for memory chunks using semantic or text search.
pub fn search(query: &str) -> Result<()> {
    let kg = KnowledgeGraph::open()?;

    // Warn if embeddings unavailable
    if !Embedder::available() {
        Embedder::warn_if_unavailable();
        println!("Falling back to text search...\n");
    }

    let chunks = kg.search_similar(query, 10)?;

    if chunks.is_empty() {
        println!("{}", style(format!("No chunks found for query: '{}'", query)).yellow());
        return Ok(());
    }

    println!("{}", style("Search Results").bold().cyan());
    println!("Query: {}\n", style(query).yellow());

    for (i, chunk) in chunks.iter().enumerate() {
        println!("{}. {} ({})", 
            style(format!("{}", i + 1)).bold(),
            style(&chunk.source_type).green(),
            style(&chunk.source_id).dim()
        );
        
        // Show content preview (first 200 chars)
        let preview = if chunk.content.len() > 200 {
            format!("{}...", &chunk.content[..200])
        } else {
            chunk.content.clone()
        };
        println!("   {}", style(preview).dim());

        // Show metadata
        println!("   {} • accessed {} times • embedding: {}", 
            style(format!("created: {:.0}", chunk.created_at)).dim(),
            style(chunk.access_count).cyan(),
            if chunk.has_embedding { style("✓").green() } else { style("✗").red() }
        );
        println!();
    }

    Ok(())
}

/// Backfill embeddings for chunks that don't have them.
pub fn backfill_embeddings() -> Result<()> {
    let kg = KnowledgeGraph::open()?;

    if !Embedder::available() {
        Embedder::warn_if_unavailable();
        println!("{}", style("Cannot backfill embeddings: model unavailable").red());
        return Ok(());
    }

    println!("{}", style("Backfilling embeddings...").cyan());

    let (processed, success) = kg.backfill_embeddings()?;

    if processed == 0 {
        println!("{}", style("✓ No chunks need embedding backfill").green());
    } else {
        println!("{}", style(format!(
            "✓ Processed {} chunks, successfully embedded {} chunks",
            processed, success
        )).green());

        if success < processed {
            println!("{}", style(format!(
                "⚠️  {} chunks failed to embed",
                processed - success
            )).yellow());
        }
    }

    Ok(())
}
