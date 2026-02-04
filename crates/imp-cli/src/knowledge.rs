//! Knowledge graph module for Imp.
//!
//! Provides a persistent knowledge graph backed by CozoDB (RocksDB engine).
//! Stores entities, relationships, and schema metadata that the agent can
//! query and evolve over time.
//!
//! Also provides a JSONL-based knowledge queue for flagging content during
//! conversations for later processing.

use crate::config::imp_home;
use crate::error::{ImpError, Result};
use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ────────────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: String,
    pub name: String,
    pub properties: JsonValue,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub id: String,
    pub from_id: String,
    pub rel_type: String,
    pub to_id: String,
    pub properties: JsonValue,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaType {
    pub type_name: String,
    pub description: String,
    pub example_names: JsonValue,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRel {
    pub rel_name: String,
    pub description: String,
    pub from_types: JsonValue,
    pub to_types: JsonValue,
    pub example_usage: String,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub types: Vec<SchemaType>,
    pub relationships: Vec<SchemaRel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    pub entity_count: usize,
    pub relationship_count: usize,
    pub chunk_count: usize,
    pub schema_type_count: usize,
    pub schema_rel_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedEntity {
    pub entity: Entity,
    pub rel_type: String,
    pub direction: String, // "->" or "<-"
}

// ────────────────────────────────────────────────────────────────────
// Knowledge Queue types
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub content: String,
    pub timestamp: f64,
    pub session_id: String,
    pub suggested_entities: Vec<String>,
}

// ────────────────────────────────────────────────────────────────────
// KnowledgeGraph
// ────────────────────────────────────────────────────────────────────

pub struct KnowledgeGraph {
    db: DbInstance,
}

impl KnowledgeGraph {
    /// Open or create the knowledge graph database.
    /// Uses RocksDB storage at `~/.imp/knowledge.cozo`.
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let path_str = path.to_str().ok_or_else(|| {
            ImpError::Database("Invalid path for knowledge database".to_string())
        })?;

        let db = DbInstance::new("rocksdb", path_str, Default::default()).map_err(|e| {
            ImpError::Database(format!("Failed to open knowledge database: {}", e))
        })?;

        let kg = Self { db };
        kg.ensure_schema()?;
        Ok(kg)
    }

    /// Path to the CozoDB database directory.
    fn db_path() -> Result<PathBuf> {
        Ok(imp_home()?.join("knowledge.cozo"))
    }

    /// Create all required relations if they don't already exist.
    fn ensure_schema(&self) -> Result<()> {
        // Create each relation, ignoring "already exists" errors
        let relations = vec![
            r#":create entity {
                id: String,
                entity_type: String,
                name: String,
                =>
                properties: Json,
                created_at: Float,
                updated_at: Float
            }"#,
            r#":create relationship {
                id: String,
                from_id: String,
                rel_type: String,
                to_id: String,
                =>
                properties: Json,
                created_at: Float
            }"#,
            r#":create memory_chunk {
                id: String,
                =>
                content: String,
                source_type: String,
                source_id: String,
                created_at: Float
            }"#,
            r#":create chunk_entity {
                chunk_id: String,
                entity_id: String
            }"#,
            r#":create schema_type {
                type_name: String,
                =>
                description: String,
                example_names: Json,
                created_at: Float
            }"#,
            r#":create schema_rel {
                rel_name: String,
                =>
                description: String,
                from_types: Json,
                to_types: Json,
                example_usage: String,
                created_at: Float
            }"#,
        ];

        for script in &relations {
            match self.run_mutating(script, BTreeMap::new()) {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    // Ignore "already exists" errors
                    if msg.contains("already exists") {
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        // Seed initial schema if empty
        self.seed_schema()?;

        Ok(())
    }

    /// Seed initial schema types and relationship types if the tables are empty.
    fn seed_schema(&self) -> Result<()> {
        // Check if schema_type has any rows
        let result = self.run_query("?[count(type_name)] := *schema_type{type_name}", BTreeMap::new())?;
        let count = Self::extract_int(&result, 0, 0).unwrap_or(0);
        if count > 0 {
            return Ok(()); // Already seeded
        }

        // Seed entity types
        self.run_mutating(
            r#"?[type_name, description, example_names, created_at] <- [
                ["person", "A human (team member, collaborator)", ["Casey", "Victor"], 0.0],
                ["project", "A code repository or major system", ["eu-exeqts-delta1", "global-prism", "imp"], 0.0],
                ["protocol", "A communication/exchange protocol", ["OUCH", "BOE", "FIX", "Pillar"], 0.0],
                ["exchange", "A stock/trading exchange", ["NASDAQ", "NYSE", "BATS"], 0.0],
                ["concept", "An abstract idea, pattern, or topic", ["instrument lifecycle", "retry logic"], 0.0],
                ["tool", "A software tool or technology", ["Rust", "Neo4j", "Cozo"], 0.0]
            ]
            :put schema_type { type_name, description, example_names, created_at }"#,
            BTreeMap::new(),
        )?;

        // Seed relationship types
        self.run_mutating(
            r#"?[rel_name, description, from_types, to_types, example_usage, created_at] <- [
                ["works_on", "Person actively works on a project", ["person"], ["project"], "Casey works_on eu-exeqts-delta1", 0.0],
                ["uses", "Project/system uses a protocol or tool", ["project"], ["protocol", "tool"], "eu-exeqts-delta1 uses OUCH", 0.0],
                ["authored", "Person created something", ["person"], ["pr", "feature"], "Victor authored PR #4161", 0.0],
                ["related_to", "General relationship between concepts", ["concept", "project", "tool"], ["concept", "project", "tool"], "instrument lifecycle related_to exeqts", 0.0],
                ["part_of", "Component belongs to larger system", ["feature", "protocol"], ["project", "system"], "TAQ OIL part_of global-prism", 0.0]
            ]
            :put schema_rel { rel_name, description, from_types, to_types, example_usage, created_at }"#,
            BTreeMap::new(),
        )?;

        Ok(())
    }

    // ────────────────────────────────────────────────────────────
    // Entity CRUD
    // ────────────────────────────────────────────────────────────

    /// Store a single entity. Generates UUID if id is empty.
    pub fn store_entity(&self, mut entity: Entity) -> Result<()> {
        if entity.id.is_empty() {
            entity.id = uuid::Uuid::new_v4().to_string();
        }
        if entity.created_at == 0.0 {
            entity.created_at = now_f64();
        }
        if entity.updated_at == 0.0 {
            entity.updated_at = entity.created_at;
        }

        let mut params = BTreeMap::new();
        params.insert("id".to_string(), DataValue::Str(entity.id.into()));
        params.insert("entity_type".to_string(), DataValue::Str(entity.entity_type.into()));
        params.insert("name".to_string(), DataValue::Str(entity.name.into()));
        params.insert("properties".to_string(), json_to_datavalue(&entity.properties));
        params.insert("created_at".to_string(), DataValue::from(entity.created_at));
        params.insert("updated_at".to_string(), DataValue::from(entity.updated_at));

        self.run_mutating(
            r#"?[id, entity_type, name, properties, created_at, updated_at] <- [
                [$id, $entity_type, $name, $properties, $created_at, $updated_at]
            ]
            :put entity { id, entity_type, name => properties, created_at, updated_at }"#,
            params,
        )?;

        Ok(())
    }

    /// Store multiple entities.
    pub fn store_entities(&self, entities: Vec<Entity>) -> Result<()> {
        for entity in entities {
            self.store_entity(entity)?;
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────────
    // Relationship CRUD
    // ────────────────────────────────────────────────────────────

    /// Store a single relationship. Generates UUID if id is empty.
    pub fn store_relationship(&self, mut rel: Relationship) -> Result<()> {
        if rel.id.is_empty() {
            rel.id = uuid::Uuid::new_v4().to_string();
        }
        if rel.created_at == 0.0 {
            rel.created_at = now_f64();
        }

        let mut params = BTreeMap::new();
        params.insert("id".to_string(), DataValue::Str(rel.id.into()));
        params.insert("from_id".to_string(), DataValue::Str(rel.from_id.into()));
        params.insert("rel_type".to_string(), DataValue::Str(rel.rel_type.into()));
        params.insert("to_id".to_string(), DataValue::Str(rel.to_id.into()));
        params.insert("properties".to_string(), json_to_datavalue(&rel.properties));
        params.insert("created_at".to_string(), DataValue::from(rel.created_at));

        self.run_mutating(
            r#"?[id, from_id, rel_type, to_id, properties, created_at] <- [
                [$id, $from_id, $rel_type, $to_id, $properties, $created_at]
            ]
            :put relationship { id, from_id, rel_type, to_id => properties, created_at }"#,
            params,
        )?;

        Ok(())
    }

    /// Store multiple relationships.
    pub fn store_relationships(&self, rels: Vec<Relationship>) -> Result<()> {
        for rel in rels {
            self.store_relationship(rel)?;
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────────
    // Queries
    // ────────────────────────────────────────────────────────────

    /// Find an entity by name: exact match first, then case-insensitive.
    pub fn find_entity_by_name(&self, name: &str) -> Result<Option<Entity>> {
        // 1. Exact match
        let mut params = BTreeMap::new();
        params.insert("name".to_string(), DataValue::Str(name.into()));

        let result = self.run_query(
            r#"?[id, entity_type, name, properties, created_at, updated_at] :=
                *entity{id, entity_type, name, properties, created_at, updated_at},
                name == $name"#,
            params.clone(),
        )?;

        if let Some(entity) = Self::rows_to_entities(&result).into_iter().next() {
            return Ok(Some(entity));
        }

        // 2. Case-insensitive match
        let result = self.run_query(
            r#"?[id, entity_type, name, properties, created_at, updated_at] :=
                *entity{id, entity_type, name, properties, created_at, updated_at},
                lowercase(name) == lowercase($name)"#,
            params,
        )?;

        Ok(Self::rows_to_entities(&result).into_iter().next())
    }

    /// Get entities related to a given entity (1-2 hops via relationships).
    pub fn get_related(&self, entity_name: &str, max_depth: usize) -> Result<Vec<RelatedEntity>> {
        let entity = match self.find_entity_by_name(entity_name)? {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let mut params = BTreeMap::new();
        params.insert("eid".to_string(), DataValue::Str(entity.id.clone().into()));

        // 1-hop: direct relationships
        let result = self.run_query(
            r#"?[other_id, other_type, other_name, other_props, other_created, other_updated, rel_type, direction] :=
                *relationship{from_id: $eid, rel_type, to_id: other_id},
                *entity{id: other_id, entity_type: other_type, name: other_name, properties: other_props, created_at: other_created, updated_at: other_updated},
                direction = "->"
            ?[other_id, other_type, other_name, other_props, other_created, other_updated, rel_type, direction] :=
                *relationship{from_id: other_id, rel_type, to_id: $eid},
                *entity{id: other_id, entity_type: other_type, name: other_name, properties: other_props, created_at: other_created, updated_at: other_updated},
                direction = "<-""#,
            params.clone(),
        )?;

        let mut related: Vec<RelatedEntity> = Vec::new();
        for row in &result.rows {
            if row.len() >= 8 {
                let other = Entity {
                    id: dv_to_string(&row[0]),
                    entity_type: dv_to_string(&row[1]),
                    name: dv_to_string(&row[2]),
                    properties: dv_to_json(&row[3]),
                    created_at: dv_to_f64(&row[4]),
                    updated_at: dv_to_f64(&row[5]),
                };
                related.push(RelatedEntity {
                    entity: other,
                    rel_type: dv_to_string(&row[6]),
                    direction: dv_to_string(&row[7]),
                });
            }
        }

        // 2-hop if requested (collect IDs from 1-hop, traverse again)
        if max_depth >= 2 && !related.is_empty() {
            let hop1_ids: Vec<DataValue> = related
                .iter()
                .map(|r| DataValue::Str(r.entity.id.clone().into()))
                .collect();

            let mut params2 = BTreeMap::new();
            params2.insert("eid".to_string(), DataValue::Str(entity.id.clone().into()));
            params2.insert("hop1_ids".to_string(), DataValue::List(hop1_ids));

            let result2 = self.run_query(
                r#"hop1[hop1_id] := hop1_id in $hop1_ids
                ?[other_id, other_type, other_name, other_props, other_created, other_updated, rel_type, direction] :=
                    hop1[hop1_id],
                    *relationship{from_id: hop1_id, rel_type, to_id: other_id},
                    *entity{id: other_id, entity_type: other_type, name: other_name, properties: other_props, created_at: other_created, updated_at: other_updated},
                    other_id != $eid,
                    not hop1[other_id],
                    direction = "->"
                ?[other_id, other_type, other_name, other_props, other_created, other_updated, rel_type, direction] :=
                    hop1[hop1_id],
                    *relationship{from_id: other_id, rel_type, to_id: hop1_id},
                    *entity{id: other_id, entity_type: other_type, name: other_name, properties: other_props, created_at: other_created, updated_at: other_updated},
                    other_id != $eid,
                    not hop1[other_id],
                    direction = "<-""#,
                params2,
            )?;

            for row in &result2.rows {
                if row.len() >= 8 {
                    let other = Entity {
                        id: dv_to_string(&row[0]),
                        entity_type: dv_to_string(&row[1]),
                        name: dv_to_string(&row[2]),
                        properties: dv_to_json(&row[3]),
                        created_at: dv_to_f64(&row[4]),
                        updated_at: dv_to_f64(&row[5]),
                    };
                    related.push(RelatedEntity {
                        entity: other,
                        rel_type: dv_to_string(&row[6]),
                        direction: dv_to_string(&row[7]),
                    });
                }
            }
        }

        Ok(related)
    }

    /// Get the current schema (types + relationship types) for LLM context.
    pub fn get_schema(&self) -> Result<SchemaInfo> {
        let types_result = self.run_query(
            "?[type_name, description, example_names, created_at] := *schema_type{type_name, description, example_names, created_at}",
            BTreeMap::new(),
        )?;

        let mut types = Vec::new();
        for row in &types_result.rows {
            if row.len() >= 4 {
                types.push(SchemaType {
                    type_name: dv_to_string(&row[0]),
                    description: dv_to_string(&row[1]),
                    example_names: dv_to_json(&row[2]),
                    created_at: dv_to_f64(&row[3]),
                });
            }
        }

        let rels_result = self.run_query(
            "?[rel_name, description, from_types, to_types, example_usage, created_at] := *schema_rel{rel_name, description, from_types, to_types, example_usage, created_at}",
            BTreeMap::new(),
        )?;

        let mut relationships = Vec::new();
        for row in &rels_result.rows {
            if row.len() >= 6 {
                relationships.push(SchemaRel {
                    rel_name: dv_to_string(&row[0]),
                    description: dv_to_string(&row[1]),
                    from_types: dv_to_json(&row[2]),
                    to_types: dv_to_json(&row[3]),
                    example_usage: dv_to_string(&row[4]),
                    created_at: dv_to_f64(&row[5]),
                });
            }
        }

        Ok(SchemaInfo { types, relationships })
    }

    /// Get counts of entities, relationships, and chunks.
    pub fn stats(&self) -> Result<KnowledgeStats> {
        let entity_count = self.count_rows("?[count(id)] := *entity{id}")?;
        let relationship_count = self.count_rows("?[count(id)] := *relationship{id}")?;
        let chunk_count = self.count_rows("?[count(id)] := *memory_chunk{id}")?;
        let schema_type_count = self.count_rows("?[count(type_name)] := *schema_type{type_name}")?;
        let schema_rel_count = self.count_rows("?[count(rel_name)] := *schema_rel{rel_name}")?;

        Ok(KnowledgeStats {
            entity_count,
            relationship_count,
            chunk_count,
            schema_type_count,
            schema_rel_count,
        })
    }

    // ────────────────────────────────────────────────────────────
    // Internal helpers
    // ────────────────────────────────────────────────────────────

    fn run_query(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> Result<NamedRows> {
        self.db
            .run_script(script, params, ScriptMutability::Immutable)
            .map_err(|e| ImpError::Database(format!("Query failed: {}", e)))
    }

    fn run_mutating(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> Result<NamedRows> {
        self.db
            .run_script(script, params, ScriptMutability::Mutable)
            .map_err(|e| ImpError::Database(format!("Mutation failed: {}", e)))
    }

    fn count_rows(&self, query: &str) -> Result<usize> {
        let result = self.run_query(query, BTreeMap::new())?;
        Ok(Self::extract_int(&result, 0, 0).unwrap_or(0) as usize)
    }

    fn extract_int(result: &NamedRows, row: usize, col: usize) -> Option<i64> {
        result.rows.get(row).and_then(|r| r.get(col)).and_then(|v| v.get_int())
    }

    fn rows_to_entities(result: &NamedRows) -> Vec<Entity> {
        let mut entities = Vec::new();
        for row in &result.rows {
            if row.len() >= 6 {
                entities.push(Entity {
                    id: dv_to_string(&row[0]),
                    entity_type: dv_to_string(&row[1]),
                    name: dv_to_string(&row[2]),
                    properties: dv_to_json(&row[3]),
                    created_at: dv_to_f64(&row[4]),
                    updated_at: dv_to_f64(&row[5]),
                });
            }
        }
        entities
    }
}

// ────────────────────────────────────────────────────────────────────
// Knowledge Queue
// ────────────────────────────────────────────────────────────────────

/// Path to the knowledge queue JSONL file.
fn queue_path() -> Result<PathBuf> {
    Ok(imp_home()?.join("knowledge_queue.jsonl"))
}

/// Append a new entry to the knowledge queue.
pub fn append_to_queue(
    content: &str,
    session_id: &str,
    suggested_entities: Vec<String>,
) -> Result<()> {
    let entry = QueueEntry {
        content: content.to_string(),
        timestamp: now_f64(),
        session_id: session_id.to_string(),
        suggested_entities,
    };

    let path = queue_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    let json = serde_json::to_string(&entry)
        .map_err(|e| ImpError::Database(format!("Failed to serialize queue entry: {}", e)))?;
    writeln!(file, "{}", json)?;

    Ok(())
}

/// Read all pending queue entries.
pub fn read_queue() -> Result<Vec<QueueEntry>> {
    let path = queue_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<QueueEntry>(&line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                eprintln!("Warning: skipping malformed queue entry: {}", e);
            }
        }
    }

    Ok(entries)
}

/// Clear all entries from the queue.
pub fn clear_queue() -> Result<()> {
    let path = queue_path()?;
    if path.exists() {
        fs::write(&path, "")?;
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────
// DataValue conversion helpers
// ────────────────────────────────────────────────────────────────────

fn dv_to_string(dv: &DataValue) -> String {
    match dv {
        DataValue::Str(s) => s.to_string(),
        DataValue::Null => String::new(),
        other => format!("{:?}", other),
    }
}

fn dv_to_f64(dv: &DataValue) -> f64 {
    dv.get_float().unwrap_or(0.0)
}

fn dv_to_json(dv: &DataValue) -> JsonValue {
    datavalue_to_json(dv)
}

/// Convert a DataValue into a serde_json::Value.
fn datavalue_to_json(dv: &DataValue) -> JsonValue {
    match dv {
        DataValue::Null => JsonValue::Null,
        DataValue::Bool(b) => JsonValue::Bool(*b),
        DataValue::Num(_) => {
            if let Some(i) = dv.get_int() {
                JsonValue::Number(serde_json::Number::from(i))
            } else {
                let f = dv.get_float().unwrap_or(0.0);
                serde_json::Number::from_f64(f)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null)
            }
        }
        DataValue::Str(s) => JsonValue::String(s.to_string()),
        DataValue::List(items) => {
            JsonValue::Array(items.iter().map(datavalue_to_json).collect())
        }
        _ => JsonValue::String(format!("{:?}", dv)),
    }
}

/// Convert a serde_json::Value into a DataValue for use as a Cozo parameter.
fn json_to_datavalue(jv: &JsonValue) -> DataValue {
    match jv {
        JsonValue::Null => DataValue::Null,
        JsonValue::Bool(b) => DataValue::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                DataValue::from(i)
            } else if let Some(f) = n.as_f64() {
                DataValue::from(f)
            } else {
                DataValue::Null
            }
        }
        JsonValue::String(s) => DataValue::Str(s.clone().into()),
        JsonValue::Array(arr) => {
            DataValue::List(arr.iter().map(json_to_datavalue).collect())
        }
        JsonValue::Object(map) => {
            // Cozo doesn't have a native Map type — store as JSON string
            let json_str = serde_json::to_string(jv).unwrap_or_default();
            DataValue::Str(json_str.into())
        }
    }
}

/// Current time as f64 (Unix timestamp).
fn now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_roundtrip() {
        // Use a temp dir for testing
        std::env::set_var("IMP_HOME", "/tmp/imp-test-knowledge");
        let _ = clear_queue();

        append_to_queue("test content", "session-1", vec!["entity1".to_string()]).unwrap();
        append_to_queue("more content", "session-1", vec![]).unwrap();

        let entries = read_queue().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "test content");
        assert_eq!(entries[1].content, "more content");

        clear_queue().unwrap();
        let entries = read_queue().unwrap();
        assert_eq!(entries.len(), 0);

        // Cleanup
        let _ = fs::remove_dir_all("/tmp/imp-test-knowledge");
    }
}
