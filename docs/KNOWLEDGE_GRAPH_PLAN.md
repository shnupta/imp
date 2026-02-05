# Knowledge Graph + Embeddings for Imp

## Overview

Add a hybrid memory system to imp using **Cozo** (embedded graph database with vector search). This replaces the current flat-file memory approach with structured knowledge that can be queried semantically and traversed relationally.

Key features:
- **Knowledge graph**: Entities and relationships extracted from conversations
- **Vector embeddings**: Semantic search over memory chunks
- **LLM-managed schema**: The schema evolves organically based on what's discussed
- **Fully automatic**: No human intervention required (schema negotiation happens in chat, not reflect)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        imp binary (single)                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │
│  │   imp.db     │    │ knowledge.cozo│    │  fastembed   │       │
│  │  (SQLite)    │    │  (embedded)  │    │  (embedded)  │       │
│  │              │    │              │    │              │       │
│  │ - sessions   │    │ - entities   │    │ - ONNX model │       │
│  │ - messages   │    │ - relations  │    │ - ~23MB      │       │
│  │              │    │ - schema     │    │ - no server  │       │
│  │              │    │ - vectors    │    │              │       │
│  └──────────────┘    └──────────────┘    └──────────────┘       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**No external services required.** Everything runs in-process.

### File Layout

```
~/.imp/
├── imp.db              # Existing: sessions, messages (unchanged)
├── knowledge.cozo      # New: knowledge graph + embeddings
├── MEMORY.md           # Keep: human-readable layer (updated by reflect)
├── USER.md             # Keep: human-readable
├── SOUL.md             # Keep: human-readable
└── ...
```

### Dependencies

Add to `Cargo.toml`:
```toml
cozo = { version = "0.7", features = ["storage-rocksdb"] }
# Or use "storage-sqlite" for lighter weight

fastembed = "4"
# Embeds ONNX runtime, downloads model on first use (~23MB for all-MiniLM-L6-v2)
```

**Model caching**: fastembed downloads models to `~/.cache/fastembed/` on first run. Subsequent runs use the cached model — no network required.

## Cozo Schema

### Core Relations

```datalog
# Entities - any "thing" worth tracking
:create entity {
    id: String,          # UUID
    entity_type: String, # "project", "person", "protocol", etc.
    name: String,        # Display name
    =>
    properties: Json,    # Flexible additional data
    created_at: Float,   # Unix timestamp
    updated_at: Float,
}

# Relationships between entities
:create relationship {
    id: String,          # UUID
    from_id: String,     # Entity ID
    rel_type: String,    # "works_on", "uses", "authored", etc.
    to_id: String,       # Entity ID
    =>
    properties: Json,    # Flexible additional data
    created_at: Float,
}

# Memory chunks with embeddings for semantic search
:create memory_chunk {
    id: String,
    =>
    content: String,        # The actual text
    source_type: String,    # "conversation", "daily_note", "context_file"
    source_id: String,      # Session ID, date, or file path
    created_at: Float,
    embedding: <F32; 1024>, # BGE-large-en-v1.5 dimension
    access_count: Int default 0,    # How many times retrieved (for pruning decisions)
    last_accessed: Float default 0, # When last retrieved
}

# Link chunks to entities they mention
:create chunk_entity {
    chunk_id: String,
    entity_id: String,
}

# Schema metadata - what types/relationships exist
:create schema_type {
    type_name: String,
    =>
    description: String,
    example_names: Json,    # ["eu-exeqts-delta1", "global-prism"]
    created_at: Float,
}

:create schema_rel {
    rel_name: String,
    =>
    description: String,
    from_types: Json,       # ["person"]
    to_types: Json,         # ["project", "pr"]
    example_usage: String,  # "Casey works_on eu-exeqts-delta1"
    created_at: Float,
}
```

### Initial Schema Seed

Bootstrap with common types the LLM can build on:

```datalog
?[type_name, description, example_names, created_at] <- [
    ["person", "A human (team member, collaborator)", ["Casey", "Victor"], 0.0],
    ["project", "A code repository or major system", ["eu-exeqts-delta1", "global-prism", "imp"], 0.0],
    ["protocol", "A communication/exchange protocol", ["OUCH", "BOE", "FIX", "Pillar"], 0.0],
    ["exchange", "A stock/trading exchange", ["NASDAQ", "NYSE", "BATS"], 0.0],
    ["concept", "An abstract idea, pattern, or topic", ["instrument lifecycle", "retry logic"], 0.0],
    ["tool", "A software tool or technology", ["Rust", "Neo4j", "Cozo"], 0.0],
]
:put schema_type { type_name, description, example_names, created_at }

?[rel_name, description, from_types, to_types, example_usage, created_at] <- [
    ["works_on", "Person actively works on a project", ["person"], ["project"], "Casey works_on eu-exeqts-delta1", 0.0],
    ["uses", "Project/system uses a protocol or tool", ["project"], ["protocol", "tool"], "eu-exeqts-delta1 uses OUCH", 0.0],
    ["authored", "Person created something", ["person"], ["pr", "feature"], "Victor authored PR #4161", 0.0],
    ["related_to", "General relationship between concepts", ["concept", "project", "tool"], ["concept", "project", "tool"], "instrument lifecycle related_to exeqts", 0.0],
    ["part_of", "Component belongs to larger system", ["feature", "protocol"], ["project", "system"], "TAQ OIL part_of global-prism", 0.0],
]
:put schema_rel { rel_name, description, from_types, to_types, example_usage, created_at }
```

## How Retrieval Works

### Integration with Existing Memory Files

The knowledge graph **augments** the existing markdown files, not replaces them:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         MEMORY ARCHITECTURE                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ALWAYS LOADED (small, essential):                                       │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐                           │
│  │  SOUL.md   │ │  USER.md   │ │ MEMORY.md  │                           │
│  │ (identity) │ │ (human)    │ │ (key facts)│                           │
│  └────────────┘ └────────────┘ └────────────┘                           │
│                                                                          │
│  LOADED BY PROJECT (when working in a project):                          │
│  ┌─────────────────────────────────────────┐                            │
│  │  projects/<name>/CONTEXT.md             │                            │
│  │  projects/<name>/PATTERNS.md            │                            │
│  └─────────────────────────────────────────┘                            │
│                                                                          │
│  RETRIEVED ON DEMAND (knowledge graph):                                  │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  knowledge.cozo                                                  │    │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │    │
│  │  │ Semantic search │  │ Graph traversal │  │ Entity lookup   │  │    │
│  │  │ "similar to     │  │ "connected to   │  │ "what do I know │  │    │
│  │  │  this query"    │  │  this entity"   │  │  about X?"      │  │    │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────┘  │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

**Why keep both?**
- Markdown files: human-readable, editable, version-controllable, small
- Knowledge graph: precise retrieval, relationships, scales to thousands of facts
- MEMORY.md stays as a curated "highlights" — most important persistent facts
- Knowledge graph holds everything, retrieved when relevant

### Retrieval Flow (Per User Message)

```
User asks: "What was that NASDAQ retry issue we talked about?"
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 1. EMBED THE QUERY                                               │
│    query_vec = Embedder::embed("NASDAQ retry issue")            │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. SEMANTIC SEARCH                                               │
│    Find top-k memory chunks with similar embeddings              │
│                                                                  │
│    ~memory_chunk:semantic_index{content, dist | query: $vec, k: 10}   │
│                                                                  │
│    Returns:                                                      │
│    - "NASDAQ OUCH needs 50ms retry backoff" (dist: 0.12)        │
│    - "Discussed retry logic for exchange connections" (0.23)    │
│    - "OUCH protocol timeout handling" (dist: 0.31)              │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. ENTITY EXTRACTION                                             │
│    From query: detect mentioned entities → ["NASDAQ"]            │
│    From chunks: get linked entities via chunk_entity relation    │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. GRAPH EXPANSION                                               │
│    For each entity, traverse relationships (1-2 hops):           │
│                                                                  │
│    NASDAQ ──uses──▶ OUCH protocol                               │
│           ──type──▶ exchange                                    │
│           ──related_to──▶ eu-exeqts-delta1                      │
│                                                                  │
│    Returns structured context about related entities             │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. MERGE & FORMAT                                                │
│                                                                  │
│    ## Retrieved Knowledge                                        │
│                                                                  │
│    **Entities:**                                                 │
│    - NASDAQ (exchange): US equities exchange                     │
│    - OUCH (protocol): Used by NASDAQ, related to retry logic     │
│                                                                  │
│    **Related notes:**                                            │
│    > NASDAQ OUCH needs 50ms retry backoff                        │
│    > Discussed retry logic for exchange connections              │
│                                                                  │
│    **Connections:**                                              │
│    - NASDAQ uses OUCH protocol                                   │
│    - OUCH related_to eu-exeqts-delta1                           │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
                    Append to system prompt
                    (after MEMORY.md, before conversation)
```

### When Retrieval Happens

```rust
impl Agent {
    async fn process_user_message(&mut self, message: &str) -> Result<Response> {
        // 1. Build system prompt (SOUL.md, USER.md, MEMORY.md, project context)
        let mut system_prompt = self.build_base_system_prompt()?;
        
        // 2. Retrieve relevant knowledge based on the user's message
        let retrieved = self.retrieve_relevant_knowledge(message)?;
        if !retrieved.is_empty() {
            system_prompt.push_str("\n---\n\n# Retrieved Knowledge\n\n");
            system_prompt.push_str(&retrieved);
        }
        
        // 3. Send to LLM
        let response = self.client.send(system_prompt, messages).await?;
        
        // 4. After response, maybe extract new knowledge
        self.maybe_extract_knowledge(&response, &context)?;
        
        Ok(response)
    }
    
    fn retrieve_relevant_knowledge(&self, query: &str) -> Result<String> {
        // Skip if knowledge graph is empty or disabled
        if !self.knowledge.has_content()? {
            return Ok(String::new());
        }
        
        // Embed the query
        let query_vec = Embedder::embed(query)?;
        
        // Hybrid search: semantic + graph
        let results = self.knowledge.query_hybrid(query_vec, HybridParams {
            semantic_k: 10,        // Top 10 similar chunks
            graph_depth: 2,        // Traverse 2 hops from entities
            max_entities: 5,       // Cap entities returned
            max_chunks: 5,         // Cap chunks returned
        })?;
        
        // Format for system prompt
        results.format_for_prompt()
    }
}
```

### How This Updates During Reflect

The `imp reflect` job updates **both** systems:

```
Daily notes from conversation
            │
            ▼
┌───────────────────────────────────────────────────────────────┐
│                        REFLECT JOB                             │
├───────────────────────────────────────────────────────────────┤
│                                                                │
│  LLM processes daily notes and decides:                        │
│                                                                │
│  1. MEMORY.md updates?                                         │
│     → Only major insights, preferences, key facts              │
│     → Human-readable summary layer                             │
│                                                                │
│  2. Knowledge graph updates:                                   │
│     → Extract ALL entities and relationships                   │
│     → Embed ALL substantive chunks                             │
│     → More granular, everything searchable                     │
│                                                                │
└───────────────────────────────────────────────────────────────┘
```

**Example:**
- Daily note: "Learned that NASDAQ OUCH retries need 50ms backoff, discussed with Victor"
- MEMORY.md: Might not be updated (too granular)
- Knowledge graph: 
  - Entities: NASDAQ, OUCH, Victor
  - Relationships: NASDAQ uses OUCH, Victor knows_about OUCH
  - Chunk: "NASDAQ OUCH retries need 50ms backoff" (embedded, linked to entities)

Later when you ask about NASDAQ, the chunk surfaces via semantic search even though it's not in MEMORY.md.

## LLM Schema Management

### During Conversations (Not Reflect)

When the agent observes something that might be worth remembering:

1. **Extract candidates** from the conversation
2. **Check existing schema** to see if types/relationships exist
3. **If uncertain**, ask the user inline (or make best guess and note confidence)
4. **Store** entities, relationships, and chunks

### Extraction Prompt

Used internally when the agent decides to persist knowledge:

```
You are extracting structured knowledge from a conversation. 

Current schema types: {schema_types}
Current relationship types: {schema_rels}

Extract from this content:
---
{content}
---

Return JSON:
{
  "entities": [
    {"type": "existing_type", "name": "...", "properties": {}},
    {"type": "NEW:suggested_type", "name": "...", "properties": {}, "type_description": "..."}
  ],
  "relationships": [
    {"from": "entity_name", "rel": "existing_rel", "to": "entity_name"},
    {"from": "entity_name", "rel": "NEW:suggested_rel", "to": "entity_name", "rel_description": "..."}
  ],
  "chunks": [
    {"content": "verbatim useful text", "mentions": ["entity_name", "entity_name"]}
  ],
  "schema_uncertain": [
    {"item": "...", "question": "Is X a type of Y or Z?"}
  ]
}
```

### Schema Evolution Flow

```
Agent observes: "The MEMO protocol is used by MEMX exchange"

1. Extract: 
   - entity: {type: "protocol", name: "MEMO"}  # type exists
   - entity: {type: "exchange", name: "MEMX"}  # type exists
   - rel: {from: "MEMX", rel: "uses", to: "MEMO"}  # rel exists

2. All types known → store directly, no human input needed

---

Agent observes: "Casey's team owns the Delta1 exeqts"

1. Extract:
   - entity: {type: "person", name: "Casey"}
   - entity: {type: "NEW:team", name: "Delta1 team", type_description: "A group of people"}
   - rel: {from: "Casey", rel: "NEW:member_of", to: "Delta1 team"}
   - rel: {from: "Delta1 team", rel: "NEW:owns", to: "eu-exeqts-delta1"}

2. New types detected → agent can either:
   a) Add silently (if confident)
   b) Mention in response: "I've noted that Delta1 team owns the exeqts - I'm tracking 'team' as a new type of entity"
   c) Ask if uncertain: "Should I track 'Delta1 team' as a team entity, or is it better captured differently?"
```

## Knowledge Graph Management

### When Knowledge Gets Created

Knowledge enters the graph through two paths:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                     KNOWLEDGE CREATION PATHS                             │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  PATH 1: DURING CONVERSATION (real-time, lightweight)                    │
│  ─────────────────────────────────────────────────────                   │
│  Trigger: Agent uses file_write tool on memory/context files             │
│           OR conversation contains substantive new information           │
│                                                                          │
│  When agent writes to:                                                   │
│    - ~/.imp/memory/YYYY-MM-DD.md (daily notes)                          │
│    - ~/.imp/projects/*/CONTEXT.md                                        │
│    - ~/.imp/MEMORY.md                                                    │
│                                                                          │
│  → Automatically extract entities/relationships from what was written    │
│  → Embed the content as searchable chunks                                │
│  → Link chunks to detected entities                                      │
│                                                                          │
│  This is INCREMENTAL — small additions as they happen                    │
│                                                                          │
│                                                                          │
│  PATH 2: DURING REFLECT (batch, thorough)                                │
│  ─────────────────────────────────────────                               │
│  Trigger: `imp reflect` runs (nightly cron or manual)                    │
│                                                                          │
│  → Process entire day's notes                                            │
│  → Extract entities/relationships with full context                      │
│  → Deduplicate against existing graph                                    │
│  → Merge/update existing entities with new info                          │
│  → Prune low-value or stale entries                                      │
│  → Curate schema (merge similar types, remove unused)                    │
│                                                                          │
│  This is COMPREHENSIVE — full review and cleanup                         │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### Reflect's Knowledge Graph Responsibilities

The reflect job does more than just add knowledge — it **curates** the graph:

```rust
// In reflect.rs

pub async fn run(date: Option<String>) -> Result<()> {
    // ... existing markdown file updates ...
    
    let knowledge = KnowledgeGraph::open()?;
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 1: EXTRACT FROM TODAY
    // ══════════════════════════════════════════════════════════════════
    let extraction = extract_knowledge_from_daily_notes(
        &daily_content,
        &knowledge.get_schema()?,
    ).await?;
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 2: DEDUPLICATE & MERGE ENTITIES
    // ══════════════════════════════════════════════════════════════════
    // Don't blindly insert — check if entity already exists
    for entity in extraction.entities {
        if let Some(existing) = knowledge.find_entity_by_name(&entity.name)? {
            // Entity exists — merge properties, update timestamp
            knowledge.merge_entity(existing.id, entity.properties)?;
        } else {
            // New entity — insert
            knowledge.store_entity(entity)?;
        }
    }
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 3: ADD RELATIONSHIPS (avoid duplicates)
    // ══════════════════════════════════════════════════════════════════
    for rel in extraction.relationships {
        if !knowledge.relationship_exists(&rel.from, &rel.rel_type, &rel.to)? {
            knowledge.store_relationship(rel)?;
        }
    }
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 4: EMBED & STORE CHUNKS
    // ══════════════════════════════════════════════════════════════════
    let chunks = chunk_content(&daily_content, ChunkStrategy::Semantic);
    for chunk in chunks {
        // Skip if we already have a very similar chunk (dedup by embedding similarity)
        if !knowledge.has_similar_chunk(&chunk.content, threshold: 0.95)? {
            let embedding = Embedder::embed(&chunk.content)?;
            knowledge.store_chunk(chunk, embedding)?;
        }
    }
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 5: CURATE SCHEMA
    // ══════════════════════════════════════════════════════════════════
    curate_schema(&knowledge).await?;
    
    // ══════════════════════════════════════════════════════════════════
    // STEP 6: PRUNE STALE KNOWLEDGE
    // ══════════════════════════════════════════════════════════════════
    prune_stale_knowledge(&knowledge, max_age_days: 90)?;
    
    Ok(())
}
```

### Schema Curation Prompt (used by reflect)

```
You are curating a knowledge graph schema. Review the current schema and usage statistics.

Current schema types:
{schema_types_with_counts}

Current relationship types:  
{schema_rels_with_counts}

Recent entity additions (last 7 days):
{recent_entities}

Recommend schema changes:

1. **Merge similar types**: If two types are essentially the same, recommend merging
   Example: "component" and "module" might be the same thing
   
2. **Promote frequent patterns**: If a property is used on most entities of a type,
   maybe it should be a relationship instead
   
3. **Remove unused**: Types/relationships with 0 uses in 30+ days can be removed

4. **Add missing**: If you see patterns that should be explicit types, suggest them

Return JSON:
{
  "merge_types": [
    {"keep": "component", "remove": "module", "reason": "..."}
  ],
  "remove_types": [
    {"type": "unused_type", "reason": "No entities in 30 days"}
  ],
  "remove_rels": [
    {"rel": "unused_rel", "reason": "..."}  
  ],
  "suggest_types": [
    {"name": "...", "description": "...", "evidence": "..."}
  ],
  "no_changes_needed": false
}
```

### Entity Resolution (Deduplication)

When extracting "NASDAQ" — is it the same as existing "Nasdaq" or "NASDAQ exchange"?

```rust
impl KnowledgeGraph {
    /// Find existing entity that matches (fuzzy matching + type awareness)
    pub fn find_entity_by_name(&self, name: &str) -> Result<Option<Entity>> {
        // 1. Exact match
        if let Some(e) = self.get_entity_exact(name)? {
            return Ok(Some(e));
        }
        
        // 2. Case-insensitive match
        if let Some(e) = self.get_entity_icase(name)? {
            return Ok(Some(e));
        }
        
        // 3. Embedding similarity (catches "NASDAQ" vs "Nasdaq exchange")
        let embedding = Embedder::embed(name)?;
        let similar = self.search_entities_by_embedding(embedding, k: 3, threshold: 0.9)?;
        
        if similar.len() == 1 {
            return Ok(Some(similar[0].clone()));
        }
        
        // Multiple candidates — return None, let caller decide
        Ok(None)
    }
    
    /// Merge new properties into existing entity
    pub fn merge_entity(&self, entity_id: &str, new_properties: Json) -> Result<()> {
        // Get existing
        let mut entity = self.get_entity(entity_id)?;
        
        // Merge properties (new values override old)
        for (key, value) in new_properties.as_object().unwrap() {
            entity.properties[key] = value.clone();
        }
        
        // Update timestamp
        entity.updated_at = now();
        
        self.update_entity(entity)
    }
}
```

### Chunk Deduplication

Avoid storing nearly-identical chunks:

```rust
impl KnowledgeGraph {
    /// Check if a very similar chunk already exists
    pub fn has_similar_chunk(&self, content: &str, threshold: f32) -> Result<bool> {
        let embedding = Embedder::embed(content)?;
        
        // Search for similar chunks
        let similar = self.search_chunks_by_embedding(embedding, k: 1)?;
        
        if let Some((chunk, similarity)) = similar.first() {
            // If similarity > threshold, consider it a duplicate
            Ok(similarity > threshold)
        } else {
            Ok(false)
        }
    }
}
```

### Pruning Stale Knowledge

```rust
fn prune_stale_knowledge(knowledge: &KnowledgeGraph, max_age_days: u32) -> Result<PruneStats> {
    let cutoff = now() - Duration::days(max_age_days);
    
    // Find chunks that:
    // 1. Are older than cutoff
    // 2. Have never been retrieved (access_count = 0)
    // 3. Are not linked to any active entities
    let stale_chunks = knowledge.query(r#"
        ?[id] := 
            *memory_chunk{id, created_at, access_count},
            created_at < $cutoff,
            access_count == 0,
            not *chunk_entity{chunk_id: id, entity_id: _}
    "#, params: {"cutoff": cutoff})?;
    
    // Delete stale chunks
    for chunk_id in stale_chunks {
        knowledge.delete_chunk(chunk_id)?;
    }
    
    // Find orphaned entities (no relationships, no chunk mentions)
    let orphaned = knowledge.query(r#"
        ?[id] :=
            *entity{id},
            not *relationship{from_id: id},
            not *relationship{to_id: id},
            not *chunk_entity{entity_id: id}
    "#)?;
    
    // Don't auto-delete entities — flag for review
    // (entities might be important even without relationships)
    
    Ok(PruneStats {
        chunks_deleted: stale_chunks.len(),
        orphaned_entities: orphaned.len(),
    })
}
```

### Access Tracking (for pruning decisions)

Track when chunks are retrieved so we know what's useful:

```rust
impl KnowledgeGraph {
    pub fn search_similar(&self, embedding: Vec<f32>, k: usize) -> Result<Vec<MemoryChunk>> {
        let results = self.query_vector_search(embedding, k)?;
        
        // Increment access count for retrieved chunks
        for chunk in &results {
            self.increment_access_count(&chunk.id)?;
        }
        
        Ok(results)
    }
}

// Add to memory_chunk relation:
:create memory_chunk {
    id: String,
    =>
    content: String,
    source_type: String,
    source_id: String,
    created_at: Float,
    embedding: <F32; 1024>,
    access_count: Int default 0,    // Track retrievals
    last_accessed: Float default 0, // When last retrieved
}
```

### Manual Curation Commands

For when automatic curation isn't enough:

```bash
# View knowledge graph stats
imp knowledge stats
# Entities: 142, Relationships: 89, Chunks: 534
# Schema: 8 types, 12 relationship types

# Search for an entity
imp knowledge search "NASDAQ"
# Found entity: NASDAQ (exchange)
#   Relationships: uses OUCH, uses MEMO, related_to eu-exeqts-delta1
#   Mentioned in: 12 chunks

# Merge duplicate entities
imp knowledge merge "Nasdaq" "NASDAQ"
# Merged "Nasdaq" into "NASDAQ" (2 relationships moved)

# Delete an entity
imp knowledge delete entity "old-thing"
# Deleted entity and 3 relationships

# View schema
imp knowledge schema
# Types: person, project, protocol, exchange, concept, tool, team, pr
# Relationships: works_on, uses, authored, related_to, part_of, member_of, owns

# Add schema type manually
imp knowledge schema add-type "feature" "A product feature or capability"

# Prune old data
imp knowledge prune --older-than 90d --dry-run
# Would delete: 45 chunks, 0 entities
imp knowledge prune --older-than 90d
# Deleted: 45 chunks
```

## Integration Points

### 1. New Module: `src/knowledge.rs`

```rust
use cozo::DbInstance;

pub struct KnowledgeGraph {
    db: DbInstance,
}

impl KnowledgeGraph {
    pub fn open() -> Result<Self> {
        let path = imp_home()?.join("knowledge.cozo");
        let db = DbInstance::new("rocksdb", path.to_str().unwrap(), "")?;
        
        // Run migrations / ensure schema exists
        Self::ensure_schema(&db)?;
        
        Ok(Self { db })
    }
    
    /// Get current schema for LLM context
    pub fn get_schema(&self) -> Result<SchemaInfo> { ... }
    
    /// Store extracted entities
    pub fn store_entities(&self, entities: Vec<Entity>) -> Result<()> { ... }
    
    /// Store relationships  
    pub fn store_relationships(&self, rels: Vec<Relationship>) -> Result<()> { ... }
    
    /// Store memory chunk with embedding
    pub fn store_chunk(&self, content: &str, source: ChunkSource, embedding: Vec<f32>) -> Result<String> { ... }
    
    /// Semantic search over chunks
    pub fn search_similar(&self, embedding: Vec<f32>, k: usize) -> Result<Vec<MemoryChunk>> { ... }
    
    /// Graph traversal from entity
    pub fn get_related(&self, entity_name: &str, max_depth: usize) -> Result<SubGraph> { ... }
    
    /// Hybrid query: semantic + graph expansion
    pub fn query_hybrid(&self, query_embedding: Vec<f32>, k: usize) -> Result<HybridResult> { ... }
    
    /// Add new schema type
    pub fn add_schema_type(&self, type_name: &str, description: &str) -> Result<()> { ... }
    
    /// Add new relationship type
    pub fn add_schema_rel(&self, rel_name: &str, from_types: Vec<&str>, to_types: Vec<&str>, description: &str) -> Result<()> { ... }
}
```

### 2. New Module: `src/embeddings.rs`

```rust
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use std::sync::OnceLock;

// Singleton model - expensive to load, reuse across calls
static EMBEDDING_MODEL: OnceLock<TextEmbedding> = OnceLock::new();

pub struct Embedder;

impl Embedder {
    /// Get or initialize the embedding model (lazy, cached)
    fn model() -> &'static TextEmbedding {
        EMBEDDING_MODEL.get_or_init(|| {
            TextEmbedding::try_new(InitOptions {
                model_name: EmbeddingModel::BGELargeENV15,  // 1024 dimensions, high quality
                show_download_progress: true,
                ..Default::default()
            }).expect("Failed to load embedding model")
        })
    }
    
    /// Embed a single text
    pub fn embed(text: &str) -> Result<Vec<f32>> {
        let embeddings = Self::model().embed(vec![text], None)?;
        Ok(embeddings.into_iter().next().unwrap())
    }
    
    /// Embed multiple texts (batched, more efficient)
    pub fn embed_batch(texts: Vec<&str>) -> Result<Vec<Vec<f32>>> {
        Self::model().embed(texts, None).map_err(Into::into)
    }
}
```

**Model choice: BGE-large-en-v1.5**
- 1024 dimensions — captures more semantic nuance
- ~335MB download (once, cached at `~/.cache/fastembed/`)
- Top-tier quality on MTEB benchmarks
- Good for knowledge retrieval where quality matters more than speed

Alternative models supported by fastembed:
| Model | Dimensions | Size | Notes |
|-------|-----------|------|-------|
| `BGELargeENV15` | 1024 | ~335MB | Best quality, recommended |
| `BGEBaseENV15` | 768 | ~130MB | Good balance |
| `BGESmallENV15` | 384 | ~45MB | Faster, lighter |
| `NomicEmbedTextV15` | 768 | ~275MB | Good for long texts |

Can be configured in `config.toml` if user wants to trade quality for size.

### 3. Update `src/agent.rs`

Add knowledge graph to Agent:

```rust
pub struct Agent {
    // ... existing fields ...
    knowledge: KnowledgeGraph,
}

impl Agent {
    /// Called after each assistant response to potentially extract knowledge
    fn maybe_extract_knowledge(&mut self, response: &str, context: &ConversationContext) -> Result<()> {
        // Heuristic: only extract if conversation seems substantive
        if !self.should_extract(response, context) {
            return Ok(());
        }
        
        let schema = self.knowledge.get_schema()?;
        let extraction = self.extract_knowledge(response, &schema)?;
        
        // Store entities (auto-add new schema types if confident)
        for entity in extraction.entities {
            if entity.is_new_type() && entity.confidence > 0.8 {
                self.knowledge.add_schema_type(&entity.type_name, &entity.type_description)?;
            }
            self.knowledge.store_entities(vec![entity.into()])?;
        }
        
        // Store relationships
        self.knowledge.store_relationships(extraction.relationships)?;
        
        // Embed and store chunks
        for chunk in extraction.chunks {
            let embedding = Embedder::embed(&chunk.content)?;
            self.knowledge.store_chunk(&chunk.content, chunk.source, embedding)?;
        }
        
        Ok(())
    }
    
    /// Enhance context loading with knowledge graph
    fn load_relevant_context(&self, user_message: &str) -> Result<String> {
        let query_embedding = Embedder::embed(user_message)?;
        
        // Hybrid search: semantic + graph
        let results = self.knowledge.query_hybrid(query_embedding, 10)?;
        
        // Format for system prompt
        let mut context = String::new();
        
        if !results.entities.is_empty() {
            context.push_str("## Relevant Knowledge\n\n");
            for entity in results.entities {
                context.push_str(&format!("- **{}** ({}): {}\n", 
                    entity.name, entity.entity_type, entity.summary()));
            }
        }
        
        if !results.chunks.is_empty() {
            context.push_str("\n## Related Notes\n\n");
            for chunk in results.chunks {
                context.push_str(&format!("> {}\n\n", chunk.content));
            }
        }
        
        Ok(context)
    }
}
```

### 4. Update `src/cli/reflect.rs`

Add knowledge extraction to reflection:

```rust
pub async fn run(date: Option<String>) -> Result<()> {
    // ... existing reflection logic ...
    
    // After updating markdown files, also update knowledge graph
    let knowledge = KnowledgeGraph::open()?;
    
    // Extract entities/relationships from the day
    let extraction = extract_from_daily_notes(&daily_content, &knowledge.get_schema()?).await?;
    
    // Store everything
    for entity in extraction.entities {
        knowledge.store_entities(vec![entity])?;
    }
    knowledge.store_relationships(extraction.relationships)?;
    
    // Embed the daily notes as chunks (sync, no external service)
    let chunks = chunk_daily_notes(&daily_content);
    for chunk in chunks {
        let embedding = Embedder::embed(&chunk.content)?;
        knowledge.store_chunk(
            &chunk.content,
            ChunkSource::DailyNote { date: target_date.clone() },
            embedding
        )?;
    }
    
    println!("{}", style(format!(
        "  ✅ Knowledge graph updated ({} entities, {} relationships, {} chunks)",
        extraction.entities.len(),
        extraction.relationships.len(),
        chunks.len()
    )).green());
    
    Ok(())
}
```

### 5. System Prompt Integration

Update `src/prompts.rs` or wherever system prompts are built:

```rust
fn build_system_prompt(agent: &Agent, user_message: &str) -> String {
    let mut prompt = String::new();
    
    // ... existing identity, project context ...
    
    // Add relevant knowledge from graph
    if let Ok(context) = agent.load_relevant_context(user_message) {
        if !context.is_empty() {
            prompt.push_str("\n---\n\n# Relevant Knowledge\n\n");
            prompt.push_str(&context);
        }
    }
    
    prompt
}
```

## Implementation Phases

### Phase 1: Foundation (MVP)
**Goal**: Get Cozo integrated with basic entity/relationship storage

1. Add `cozo` dependency
2. Create `src/knowledge.rs` with:
   - `KnowledgeGraph::open()` 
   - Schema creation (core relations)
   - `store_entities()`, `store_relationships()`
   - `get_related()` basic graph traversal
3. Seed initial schema types
4. Add `imp knowledge` CLI for debugging:
   - `imp knowledge stats` — count entities, relationships
   - `imp knowledge query "NASDAQ"` — show related entities

**No embedding yet, no automatic extraction yet.**

### Phase 2: Embeddings
**Goal**: Add semantic search capability

1. Add `fastembed` dependency  
2. Create `src/embeddings.rs` with `Embedder` singleton
3. Use **BGE-large-en-v1.5** model (1024 dimensions, high quality)
4. Add `memory_chunk` relation to Cozo with HNSW index
5. Add `store_chunk()`, `search_similar()`
6. Add `imp knowledge search "retry logic"` CLI
7. First embed call downloads model (~335MB) — subsequent calls fast (~10-20ms)

### Phase 3: Automatic Extraction
**Goal**: Agent extracts knowledge during conversations

1. Add extraction prompt logic
2. Implement `maybe_extract_knowledge()` in agent
3. Add schema evolution (new types/relationships)
4. Heuristics for when to extract (avoid noise)

### Phase 4: Context Enhancement  
**Goal**: Knowledge graph informs conversations

1. Implement `query_hybrid()` 
2. Integrate into system prompt building
3. Add `load_relevant_context()`
4. Tune retrieval (k values, ranking)

### Phase 5: Reflect Integration
**Goal**: Daily reflection updates knowledge graph

1. Add extraction to reflect job
2. Chunk and embed daily notes
3. Link chunks to entities
4. Summary stats in reflect output

## Queries Cheat Sheet

```datalog
# Get all entity types in schema
?[type_name, description] := *schema_type{type_name, description}

# Get all entities of a type
?[name, properties] := *entity{entity_type: "project", name, properties}

# Find entity and all direct relationships
?[rel_type, direction, other_name, other_type] := 
    *entity{id: eid, name: "NASDAQ"},
    (
        *relationship{from_id: eid, rel_type, to_id: other_id};
        *relationship{from_id: other_id, rel_type, to_id: eid}
    ),
    *entity{id: other_id, name: other_name, entity_type: other_type},
    direction = if(*relationship{from_id: eid}, "->", "<-")

# Semantic search (top 5 similar chunks)
?[content, source_type, dist] := 
    ~memory_chunk:embedding_index{
        content, source_type, dist | 
        query: $query_vec, 
        k: 5, 
        ef: 50
    }

# Hybrid: semantic search + expand to related entities
similar[chunk_id, content, dist] := 
    ~memory_chunk:embedding_index{id: chunk_id, content, dist | query: $query_vec, k: 10}

mentioned[entity_id] := 
    similar[chunk_id, _, _],
    *chunk_entity{chunk_id, entity_id}

?[entity_name, entity_type, chunk_content] :=
    mentioned[eid],
    *entity{id: eid, name: entity_name, entity_type},
    *chunk_entity{chunk_id, entity_id: eid},
    *memory_chunk{id: chunk_id, content: chunk_content}
```

## Configuration

Add to `~/.imp/config.toml`:

```toml
[knowledge]
enabled = true
auto_extract = true           # Extract during conversations
extract_confidence = 0.8      # Minimum confidence for new schema types

[embeddings]
# Model options: BGELargeENV15 (1024d, best), BGEBaseENV15 (768d), BGESmallENV15 (384d)
model = "BGELargeENV15"
# cache_dir = "~/.cache/fastembed"  # Optional: override model cache location

[retrieval]
semantic_k = 10               # How many chunks to retrieve
graph_depth = 2               # How many hops to traverse
max_entities = 5              # Max entities in context
max_chunks = 5                # Max chunks in context
```

## Error Handling

- **First run model download**: fastembed downloads model on first use — warn user, show progress
- **Cozo corruption**: Backup strategy, rebuild from imp.db conversations
- **Extraction fails**: Log, don't block conversation
- **Schema conflicts**: LLM should check existing before proposing; merge if duplicate

## Graceful Degradation (Embeddings Unavailable)

If embeddings fail (model download fails, ONNX runtime issues, disk full, etc.), the knowledge graph should still work — just without semantic search.

### Embedder with Fallback

```rust
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static EMBEDDING_MODEL: OnceLock<Option<TextEmbedding>> = OnceLock::new();
static EMBEDDINGS_WARNED: AtomicBool = AtomicBool::new(false);

pub struct Embedder;

impl Embedder {
    /// Try to initialize model, return None if it fails
    fn try_model() -> Option<&'static TextEmbedding> {
        EMBEDDING_MODEL.get_or_init(|| {
            match TextEmbedding::try_new(InitOptions {
                model_name: EmbeddingModel::BGELargeENV15,
                show_download_progress: true,
                ..Default::default()
            }) {
                Ok(model) => Some(model),
                Err(e) => {
                    eprintln!("⚠️  Failed to load embedding model: {}", e);
                    eprintln!("   Knowledge graph will work without semantic search.");
                    eprintln!("   Entity lookup and graph traversal still available.");
                    None
                }
            }
        }).as_ref()
    }
    
    /// Embed text, returns None if embeddings unavailable
    pub fn embed(text: &str) -> Option<Vec<f32>> {
        Self::try_model().and_then(|model| {
            model.embed(vec![text], None)
                .ok()
                .and_then(|mut v| v.pop())
        })
    }
    
    /// Check if embeddings are available
    pub fn available() -> bool {
        Self::try_model().is_some()
    }
    
    /// Warn once if embeddings unavailable
    pub fn warn_if_unavailable() {
        if !Self::available() && !EMBEDDINGS_WARNED.swap(true, Ordering::Relaxed) {
            eprintln!("⚠️  Semantic search disabled (embedding model unavailable)");
        }
    }
}
```

### Retrieval Without Embeddings

```rust
impl KnowledgeGraph {
    /// Hybrid query with fallback when embeddings unavailable
    pub fn query_hybrid(&self, query: &str, params: HybridParams) -> Result<HybridResult> {
        let mut result = HybridResult::default();
        
        // ══════════════════════════════════════════════════════════════
        // SEMANTIC SEARCH (if embeddings available)
        // ══════════════════════════════════════════════════════════════
        if let Some(embedding) = Embedder::embed(query) {
            let chunks = self.search_chunks_by_embedding(embedding, params.semantic_k)?;
            result.chunks = chunks;
        } else {
            // Fallback: keyword/text search in Cozo
            let chunks = self.search_chunks_by_text(query, params.semantic_k)?;
            result.chunks = chunks;
        }
        
        // ══════════════════════════════════════════════════════════════
        // ENTITY EXTRACTION (always works - no embeddings needed)
        // ══════════════════════════════════════════════════════════════
        // Simple keyword extraction from query
        let keywords = extract_keywords(query);  // Basic NLP or just split words
        
        for keyword in keywords {
            // Look up entities by name (case-insensitive)
            if let Some(entity) = self.find_entity_by_name(&keyword)? {
                result.entities.push(entity);
            }
        }
        
        // ══════════════════════════════════════════════════════════════
        // GRAPH TRAVERSAL (always works - no embeddings needed)
        // ══════════════════════════════════════════════════════════════
        // From found entities, traverse relationships
        let mut related_entities = Vec::new();
        for entity in &result.entities {
            let connected = self.get_connected_entities(&entity.id, params.graph_depth)?;
            related_entities.extend(connected);
        }
        result.related_entities = related_entities;
        
        // Also get entities mentioned in retrieved chunks
        for chunk in &result.chunks {
            let mentioned = self.get_entities_for_chunk(&chunk.id)?;
            result.entities.extend(mentioned);
        }
        
        // Deduplicate
        result.entities.dedup_by_key(|e| e.id.clone());
        result.related_entities.dedup_by_key(|e| e.id.clone());
        
        Ok(result)
    }
    
    /// Text-based chunk search (fallback when no embeddings)
    fn search_chunks_by_text(&self, query: &str, k: usize) -> Result<Vec<MemoryChunk>> {
        // Cozo supports text search via LIKE or regex
        let results = self.db.run_script(r#"
            ?[id, content, source_type, score] :=
                *memory_chunk{id, content, source_type},
                content ~~ $pattern,
                score = 1.0
            
            :order -score
            :limit $k
        "#, btmap! {
            "pattern" => format!("%{}%", query.to_lowercase()),
            "k" => k,
        })?;
        
        // Parse results into MemoryChunk structs
        Ok(parse_chunk_results(results))
    }
}
```

### Storage Without Embeddings

```rust
impl KnowledgeGraph {
    /// Store chunk, skip embedding if unavailable
    pub fn store_chunk(&self, content: &str, source: ChunkSource) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = timestamp_now();
        
        // Try to get embedding, use empty vec if unavailable
        let embedding = Embedder::embed(content).unwrap_or_default();
        let has_embedding = !embedding.is_empty();
        
        self.db.run_script(r#"
            ?[id, content, source_type, source_id, created_at, embedding, has_embedding] <- [[
                $id, $content, $source_type, $source_id, $now, $embedding, $has_embedding
            ]]
            :put memory_chunk {id, content, source_type, source_id, created_at, embedding, has_embedding}
        "#, btmap! {
            "id" => id.clone(),
            "content" => content,
            "source_type" => source.type_str(),
            "source_id" => source.id_str(),
            "now" => now,
            "embedding" => embedding,
            "has_embedding" => has_embedding,
        })?;
        
        if !has_embedding {
            Embedder::warn_if_unavailable();
        }
        
        Ok(id)
    }
}
```

### Updated Schema (track which chunks have embeddings)

```datalog
:create memory_chunk {
    id: String,
    =>
    content: String,
    source_type: String,
    source_id: String,
    created_at: Float,
    embedding: <F32; 1024>,
    has_embedding: Bool default false,  # Track if embedding was successful
    access_count: Int default 0,
    last_accessed: Float default 0,
}
```

### Backfill Command

When embeddings become available later, backfill chunks that are missing embeddings:

```bash
imp knowledge backfill-embeddings
# Found 45 chunks without embeddings
# Processing... ████████████████████████ 45/45
# Done. All chunks now have embeddings.
```

```rust
pub fn backfill_embeddings(knowledge: &KnowledgeGraph) -> Result<BackfillStats> {
    if !Embedder::available() {
        return Err(anyhow!("Embedding model not available"));
    }
    
    // Find chunks without embeddings
    let chunks_to_update = knowledge.query(r#"
        ?[id, content] := *memory_chunk{id, content, has_embedding: false}
    "#)?;
    
    let total = chunks_to_update.len();
    let mut updated = 0;
    
    for (id, content) in chunks_to_update {
        if let Some(embedding) = Embedder::embed(&content) {
            knowledge.update_chunk_embedding(&id, embedding)?;
            updated += 1;
        }
    }
    
    Ok(BackfillStats { total, updated })
}
```

### What Works Without Embeddings

| Feature | Without Embeddings | With Embeddings |
|---------|-------------------|-----------------|
| Store entities | ✅ | ✅ |
| Store relationships | ✅ | ✅ |
| Store chunks | ✅ (no vector) | ✅ |
| Entity lookup by name | ✅ | ✅ |
| Graph traversal | ✅ | ✅ |
| Keyword chunk search | ✅ (text match) | ✅ |
| Semantic chunk search | ❌ | ✅ |
| Similar entity detection | ❌ | ✅ |
| Fuzzy deduplication | ❌ | ✅ |

**Bottom line**: The knowledge graph is still useful without embeddings — you just lose semantic search. Entity/relationship storage, graph traversal, and keyword search all work fine.

## Future Enhancements

- **Schema visualization**: `imp knowledge graph` outputs DOT/Mermaid
- **Time decay**: Weight recent knowledge higher
- **Confidence scores**: Track extraction confidence, prune low-confidence over time
- **Cross-project knowledge**: "This pattern in project A also applies to project B"
- **Export/import**: Backup knowledge graph, share between machines

## Testing

1. **Unit tests** for Cozo queries (use in-memory instance)
2. **Integration tests** for extraction prompt (mock LLM responses)
3. **Embedding tests** — fastembed works offline after first download, can test with real embeddings
4. **End-to-end**: Conversation → extraction → retrieval round-trip

---

## Summary

This adds a hybrid knowledge graph to imp:

| Component | Technology | Purpose |
|-----------|------------|---------|
| Graph storage | Cozo (embedded) | Entities, relationships, schema |
| Vector storage | Cozo HNSW | Semantic search over memory chunks |
| Embeddings | fastembed BGE-large (1024d) | High-quality text vectors |
| Schema management | LLM-driven | Evolves based on conversations |

**Retrieval flow:**
1. User message comes in
2. Embed query → semantic search for similar chunks
3. Extract entities from query/chunks → graph traversal
4. Merge results → append to system prompt as "Retrieved Knowledge"
5. After response, extract new knowledge to store

**Relationship with markdown files:**
- MEMORY.md, USER.md, SOUL.md — still loaded every conversation (curated highlights)
- Knowledge graph — retrieved on-demand based on query relevance
- Reflect job updates BOTH (markdown for human-readable summary, graph for searchable detail)

**Everything runs in-process** — no external services, no network calls (after initial model download). Single binary, fully self-contained.
