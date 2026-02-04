//! Knowledge extraction module for Imp.
//!
//! Extracts entities, relationships, and chunks from conversation content
//! and stores them in the knowledge graph. Uses lightweight pattern matching
//! and queue processing to avoid blocking conversations.

use crate::error::{ImpError, Result};
use crate::knowledge::{Entity, Relationship, SchemaInfo, QueueEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::time::{SystemTime, UNIX_EPOCH};

// ────────────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub entities: Vec<ExtractedEntity>,
    pub relationships: Vec<ExtractedRelationship>,
    pub chunks: Vec<ExtractedChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub entity_type: String, // existing type or "NEW:suggested_type"
    pub name: String,
    pub properties: JsonValue,
    pub type_description: Option<String>, // only for NEW types
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    pub from_name: String,
    pub rel_type: String,
    pub to_name: String,
    pub rel_description: Option<String>, // only for NEW types
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedChunk {
    pub content: String,
    pub mentions: Vec<String>, // entity names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionStats {
    pub entities_added: usize,
    pub relationships_added: usize,
    pub chunks_stored: usize,
    pub new_types_added: usize,
}

// ────────────────────────────────────────────────────────────────────
// Extraction logic
// ────────────────────────────────────────────────────────────────────

/// Heuristic to decide if a conversation turn is worth extracting knowledge from.
/// Skip trivial turns (short responses, no tools, greetings, etc.).
/// Extract when: tool_count > 0, response is substantial (>300 chars), 
/// conversation involves facts/decisions/technical content.
pub fn should_extract(user_message: &str, response_text: &str, tool_count: usize) -> bool {
    // Always extract if tools were used - indicates substantive interaction
    if tool_count > 0 {
        return true;
    }

    // Skip very short responses
    if response_text.len() < 300 {
        return false;
    }

    // Skip common greetings and simple responses
    let response_lower = response_text.to_lowercase();
    if response_lower.starts_with("hello") ||
       response_lower.starts_with("hi ") ||
       response_lower.starts_with("sure") ||
       response_lower.starts_with("okay") ||
       response_lower.starts_with("yes") ||
       response_lower.starts_with("no ") {
        return false;
    }

    // Look for technical/factual content indicators
    let combined = format!("{} {}", user_message.to_lowercase(), response_lower);
    let tech_indicators = [
        "project", "code", "file", "function", "api", "database", "server",
        "error", "bug", "fix", "implement", "feature", "config", "deploy",
        "test", "build", "run", "install", "dependency", "protocol", "exchange",
        "entity", "relationship", "data", "schema", "query", "search", "store",
        "rust", "cargo", "github", "git", "branch", "commit", "pr", "issue",
    ];

    for indicator in &tech_indicators {
        if combined.contains(indicator) {
            return true;
        }
    }

    false
}

/// Process pending knowledge queue entries using lightweight extraction.
/// This is Option B from the task - instead of calling the LLM during conversation,
/// we process the queue that gets populated by the queue_knowledge tool.
pub fn process_knowledge_queue(
    queue_entries: Vec<QueueEntry>, 
    _schema: &SchemaInfo
) -> Result<ExtractionResult> {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();
    let mut chunks = Vec::new();

    for entry in queue_entries {
        // Extract chunks - store the content for semantic search
        chunks.push(ExtractedChunk {
            content: entry.content.clone(),
            mentions: entry.suggested_entities.clone(),
        });

        // Simple pattern-based entity extraction from suggested entities
        for entity_name in &entry.suggested_entities {
            // Basic type inference based on naming patterns
            let entity_type = infer_entity_type(entity_name);
            
            entities.push(ExtractedEntity {
                entity_type: entity_type.to_string(),
                name: entity_name.clone(),
                properties: JsonValue::Object(serde_json::Map::new()),
                type_description: None,
            });
        }

        // Simple relationship extraction - if multiple entities mentioned, 
        // create "related_to" relationships between them
        if entry.suggested_entities.len() >= 2 {
            for i in 0..entry.suggested_entities.len() {
                for j in (i + 1)..entry.suggested_entities.len() {
                    relationships.push(ExtractedRelationship {
                        from_name: entry.suggested_entities[i].clone(),
                        rel_type: "related_to".to_string(),
                        to_name: entry.suggested_entities[j].clone(),
                        rel_description: None,
                    });
                }
            }
        }
    }

    Ok(ExtractionResult {
        entities,
        relationships,
        chunks,
    })
}

/// Simple entity type inference based on naming patterns.
/// This is a lightweight heuristic for queue processing.
fn infer_entity_type(name: &str) -> &'static str {
    let name_lower = name.to_lowercase();
    
    // Project patterns
    if name_lower.contains("project") || 
       name_lower.ends_with("-cli") ||
       name_lower.contains("exeqts") ||
       name_lower.contains("prism") ||
       name_lower == "imp" {
        return "project";
    }
    
    // Protocol patterns
    if name_lower == "ouch" || name_lower == "fix" || 
       name_lower == "boe" || name_lower == "pillar" ||
       name_lower == "memo" {
        return "protocol";
    }
    
    // Exchange patterns
    if name_lower == "nasdaq" || name_lower == "nyse" ||
       name_lower == "bats" || name_lower == "memx" {
        return "exchange";
    }
    
    // Tool patterns
    if name_lower == "rust" || name_lower == "cargo" ||
       name_lower == "cozo" || name_lower == "neo4j" ||
       name_lower == "git" || name_lower == "github" {
        return "tool";
    }
    
    // Person patterns (common names)
    if name_lower == "casey" || name_lower == "victor" ||
       name_lower.len() <= 12 && !name_lower.contains("_") && 
       !name_lower.contains("-") && !name_lower.contains(".") {
        return "person";
    }
    
    // Default to concept for everything else
    "concept"
}

/// Convert an ExtractedEntity to the knowledge graph Entity type.
impl From<ExtractedEntity> for Entity {
    fn from(extracted: ExtractedEntity) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        
        Entity {
            id: String::new(), // Will be set by store_entity
            entity_type: extracted.entity_type,
            name: extracted.name,
            properties: extracted.properties,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Convert an ExtractedRelationship to the knowledge graph Relationship type.
impl From<ExtractedRelationship> for Relationship {
    fn from(extracted: ExtractedRelationship) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
            
        Relationship {
            id: String::new(), // Will be set by store_relationship
            from_id: String::new(), // Will be resolved by caller
            rel_type: extracted.rel_type,
            to_id: String::new(), // Will be resolved by caller
            properties: JsonValue::Object(serde_json::Map::new()),
            created_at: now,
        }
    }
}

/// Build extraction prompt for the LLM that includes current schema.
pub fn build_extraction_prompt(content: &str, schema: &SchemaInfo) -> String {
    let types_list = schema.types.iter()
        .map(|t| format!("- {}: {}", t.type_name, t.description))
        .collect::<Vec<_>>()
        .join("\n");

    let rels_list = schema.relationships.iter()
        .map(|r| format!("- {}: {} (example: {})", r.rel_name, r.description, r.example_usage))
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"You are extracting structured knowledge from conversation content. 

Current schema types:
{types_list}

Current relationship types:
{rels_list}

Extract from this content:
---
{content}
---

Return JSON:
{{
  "entities": [
    {{"type": "existing_type", "name": "...", "properties": {{}}}},
    {{"type": "NEW:suggested_type", "name": "...", "properties": {{}}, "type_description": "..."}}
  ],
  "relationships": [
    {{"from": "entity_name", "rel": "existing_rel", "to": "entity_name"}},
    {{"from": "entity_name", "rel": "NEW:suggested_rel", "to": "entity_name", "rel_description": "..."}}
  ],
  "chunks": [
    {{"content": "verbatim useful text", "mentions": ["entity_name", "entity_name"]}}
  ]
}}

Rules:
- Use existing types/relationships when possible
- For NEW: prefixed items, provide clear descriptions
- Extract only substantive, factual content
- Chunks should be self-contained useful information
- Entity names should be consistent and canonical (e.g., "NASDAQ" not "nasdaq")
- Limit to the most important entities and relationships"#,
        types_list = types_list,
        rels_list = rels_list,
        content = content
    )
}

/// Parse the LLM's JSON response into structured extraction result.
pub fn parse_extraction_response(response: &str) -> Result<ExtractionResult> {
    // Extract JSON from response (might be wrapped in ```json blocks)
    let json_str = extract_json_block(response);
    
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| ImpError::Tool(format!("Failed to parse extraction JSON: {}", e)))?;

    // Parse entities
    let mut entities = Vec::new();
    if let Some(entities_array) = parsed.get("entities").and_then(|v| v.as_array()) {
        for entity_val in entities_array {
            if let (Some(entity_type), Some(name)) = (
                entity_val.get("type").and_then(|v| v.as_str()),
                entity_val.get("name").and_then(|v| v.as_str())
            ) {
                entities.push(ExtractedEntity {
                    entity_type: entity_type.to_string(),
                    name: name.to_string(),
                    properties: entity_val.get("properties").cloned().unwrap_or(JsonValue::Object(serde_json::Map::new())),
                    type_description: entity_val.get("type_description").and_then(|v| v.as_str()).map(String::from),
                });
            }
        }
    }

    // Parse relationships
    let mut relationships = Vec::new();
    if let Some(rels_array) = parsed.get("relationships").and_then(|v| v.as_array()) {
        for rel_val in rels_array {
            if let (Some(from), Some(rel_type), Some(to)) = (
                rel_val.get("from").and_then(|v| v.as_str()),
                rel_val.get("rel").and_then(|v| v.as_str()),
                rel_val.get("to").and_then(|v| v.as_str())
            ) {
                relationships.push(ExtractedRelationship {
                    from_name: from.to_string(),
                    rel_type: rel_type.to_string(),
                    to_name: to.to_string(),
                    rel_description: rel_val.get("rel_description").and_then(|v| v.as_str()).map(String::from),
                });
            }
        }
    }

    // Parse chunks
    let mut chunks = Vec::new();
    if let Some(chunks_array) = parsed.get("chunks").and_then(|v| v.as_array()) {
        for chunk_val in chunks_array {
            if let Some(content) = chunk_val.get("content").and_then(|v| v.as_str()) {
                let mentions = chunk_val.get("mentions")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                    
                chunks.push(ExtractedChunk {
                    content: content.to_string(),
                    mentions,
                });
            }
        }
    }

    Ok(ExtractionResult {
        entities,
        relationships,
        chunks,
    })
}

/// Process extraction results and store them in the knowledge graph.
/// Handles deduplication and NEW type creation.
pub fn process_extraction(
    kg: &crate::knowledge::KnowledgeGraph,
    result: &ExtractionResult,
) -> Result<ExtractionStats> {
    use crate::knowledge::{Entity, Relationship};
    
    let mut stats = ExtractionStats {
        entities_added: 0,
        relationships_added: 0,
        chunks_stored: 0,
        new_types_added: 0,
    };

    // Process entities
    for extracted_entity in &result.entities {
        let mut entity_type = extracted_entity.entity_type.clone();
        
        // Handle NEW: prefixed types
        if entity_type.starts_with("NEW:") {
            let new_type = entity_type.strip_prefix("NEW:").unwrap_or(&entity_type);
            if let Some(description) = &extracted_entity.type_description {
                // Add new schema type
                let schema_add_script = r#"
                    ?[type_name, description, example_names, created_at] <- [[
                        $type_name, $description, [], $created_at
                    ]]
                    :put schema_type { type_name => description, example_names, created_at }
                "#;
                
                let mut params = std::collections::BTreeMap::new();
                params.insert("type_name".to_string(), cozo::DataValue::Str(new_type.into()));
                params.insert("description".to_string(), cozo::DataValue::Str(description.clone().into()));
                params.insert("created_at".to_string(), cozo::DataValue::from(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64()
                ));
                
                // Try to add the new type (ignore if already exists)
                let _ = kg.run_mutating(schema_add_script, params);
                
                stats.new_types_added += 1;
                entity_type = new_type.to_string();
            }
        }
        
        // Check if entity already exists
        if let Ok(Some(_)) = kg.find_entity_by_name(&extracted_entity.name) {
            // Entity exists, could merge properties here
            continue;
        }
        
        // Create new entity
        let entity = Entity {
            id: String::new(), // Will be set by store_entity
            entity_type,
            name: extracted_entity.name.clone(),
            properties: extracted_entity.properties.clone(),
            created_at: 0.0, // Will be set by store_entity
            updated_at: 0.0, // Will be set by store_entity
        };
        
        kg.store_entity(entity)?;
        stats.entities_added += 1;
    }

    // Process relationships
    for extracted_rel in &result.relationships {
        // Find entity IDs
        let from_entity = kg.find_entity_by_name(&extracted_rel.from_name)?;
        let to_entity = kg.find_entity_by_name(&extracted_rel.to_name)?;
        
        if let (Some(from), Some(to)) = (from_entity, to_entity) {
            // Check if relationship already exists
            let exists_query = r#"
                ?[exists] := 
                    *relationship{from_id: $from_id, rel_type: $rel_type, to_id: $to_id},
                    exists = true
            "#;
            
            let mut params = std::collections::BTreeMap::new();
            params.insert("from_id".to_string(), cozo::DataValue::Str(from.id.clone().into()));
            params.insert("rel_type".to_string(), cozo::DataValue::Str(extracted_rel.rel_type.clone().into()));
            params.insert("to_id".to_string(), cozo::DataValue::Str(to.id.clone().into()));
            
            if let Ok(result) = kg.run_query(exists_query, params.clone()) {
                if !result.rows.is_empty() {
                    continue; // Relationship already exists
                }
            }
            
            let relationship = Relationship {
                id: String::new(), // Will be set by store_relationship
                from_id: from.id,
                rel_type: extracted_rel.rel_type.clone(),
                to_id: to.id,
                properties: JsonValue::Object(serde_json::Map::new()),
                created_at: 0.0, // Will be set by store_relationship
            };
            
            kg.store_relationship(relationship)?;
            stats.relationships_added += 1;
        }
    }

    // Process chunks
    for extracted_chunk in &result.chunks {
        // Store chunk with "conversation" source type
        let chunk_id = kg.store_chunk(&extracted_chunk.content, "conversation", "reflect")?;
        
        // Link chunk to mentioned entities
        for entity_name in &extracted_chunk.mentions {
            if let Ok(Some(entity)) = kg.find_entity_by_name(entity_name) {
                let link_script = r#"
                    ?[chunk_id, entity_id] <- [[$chunk_id, $entity_id]]
                    :put chunk_entity { chunk_id, entity_id }
                "#;
                
                let mut params = std::collections::BTreeMap::new();
                params.insert("chunk_id".to_string(), cozo::DataValue::Str(chunk_id.clone().into()));
                params.insert("entity_id".to_string(), cozo::DataValue::Str(entity.id.into()));
                
                let _ = kg.run_mutating(link_script, params);
            }
        }
        
        stats.chunks_stored += 1;
    }

    Ok(stats)
}

/// LLM-based extraction for reflect command.
pub async fn extract_knowledge_llm(
    content: &str, 
    schema: &SchemaInfo,
    client: &crate::client::ClaudeClient
) -> Result<ExtractionResult> {
    let prompt = build_extraction_prompt(content, schema);
    
    let messages = vec![crate::client::Message::text("user", &prompt)];
    let response = client.send_message(messages, None, None, false)
        .await
        .map_err(|e| ImpError::Tool(format!("LLM extraction failed: {}", e)))?;
        
    let raw_response = client.extract_text_content(&response);
    parse_extraction_response(&raw_response)
}

/// Extract JSON block from response (handles ```json fences).
fn extract_json_block(text: &str) -> &str {
    let trimmed = text.trim();
    
    // Try to find ```json ... ``` block
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7; // skip ```json
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim();
        }
    }
    
    // Try ``` ... ``` block
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim();
        }
    }
    
    // Try raw JSON (starts with {)
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_extract() {
        // Should extract: tool usage
        assert!(should_extract("help me with code", "I'll help you", 1));
        
        // Should extract: substantial response
        assert!(should_extract("what is rust", &"a".repeat(350), 0));
        
        // Should extract: technical content
        assert!(should_extract("project setup", "ok", 0));
        
        // Should NOT extract: short response
        assert!(!should_extract("hi", "hello", 0));
        
        // Should NOT extract: greeting
        assert!(!should_extract("help", "sure thing", 0));
    }

    #[test]
    fn test_infer_entity_type() {
        assert_eq!(infer_entity_type("eu-exeqts-delta1"), "project");
        assert_eq!(infer_entity_type("OUCH"), "protocol");
        assert_eq!(infer_entity_type("NASDAQ"), "exchange");
        assert_eq!(infer_entity_type("Rust"), "tool");
        assert_eq!(infer_entity_type("Casey"), "person");
        assert_eq!(infer_entity_type("retry logic"), "concept");
    }

    #[test]
    fn test_process_knowledge_queue() {
        let schema = SchemaInfo {
            types: Vec::new(),
            relationships: Vec::new(),
        };
        
        let entries = vec![
            QueueEntry {
                content: "Working on NASDAQ OUCH protocol".to_string(),
                timestamp: 0.0,
                session_id: "test".to_string(),
                suggested_entities: vec!["NASDAQ".to_string(), "OUCH".to_string()],
            }
        ];
        
        let result = process_knowledge_queue(entries, &schema).unwrap();
        
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.chunks.len(), 1);
        assert_eq!(result.relationships.len(), 1);
        
        // Check entity types were inferred correctly
        assert!(result.entities.iter().any(|e| e.name == "NASDAQ" && e.entity_type == "exchange"));
        assert!(result.entities.iter().any(|e| e.name == "OUCH" && e.entity_type == "protocol"));
        
        // Check relationship was created
        assert_eq!(result.relationships[0].rel_type, "related_to");
    }
}