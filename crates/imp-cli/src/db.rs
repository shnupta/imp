use crate::client::Message;
use crate::error::{ImpError, Result};
use rusqlite::{params, Connection};
use serde_json::Value;

/// Extract human-readable text from a message's JSON content.
/// Handles both plain string content and structured content blocks,
/// filtering out tool_use, tool_result, and thinking blocks.
fn extract_readable_text(content_json: &str) -> String {
    let value: Value = match serde_json::from_str(content_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    match value {
        Value::String(s) => s,
        Value::Array(blocks) => {
            blocks.iter()
                .filter_map(|block| {
                    let block_type = block.get("type")?.as_str()?;
                    match block_type {
                        "text" => block.get("text")?.as_str().map(|s| s.to_string()),
                        _ => None, // Skip tool_use, tool_result, thinking
                    }
                })
                .collect::<Vec<_>>()
                .join("")
        }
        _ => String::new(),
    }
}

pub struct SessionInfo {
    pub id: String,
    pub project: Option<String>,
    pub workdir: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub title: Option<String>,
    pub message_count: i64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at `~/.imp/imp.db` and run migrations.
    pub fn open() -> Result<Self> {
        let imp_home = crate::config::imp_home()?;
        std::fs::create_dir_all(&imp_home)?;
        let db_path = imp_home.join("imp.db");

        let conn =
            Connection::open(&db_path).map_err(|e| ImpError::Database(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project TEXT,
                workdir TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                title TEXT,
                message_count INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tool_calls INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);",
        )
        .map_err(|e| ImpError::Database(e.to_string()))?;

        // Migration: add workdir column to existing databases
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN workdir TEXT", []);

        Ok(Self { conn })
    }

    /// Create a new session row and return its UUID.
    pub fn create_session(&self, project: Option<&str>, workdir: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO sessions (id, project, workdir, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, project, workdir, now, now],
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;
        Ok(id)
    }

    /// Persist a single message (user or assistant) into the database.
    pub fn save_message(
        &self,
        session_id: &str,
        role: &str,
        content: &Value,
        tool_calls: usize,
    ) -> Result<()> {
        let content_json = serde_json::to_string(content)?;
        let now = chrono::Utc::now().to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO messages (session_id, role, content, created_at, tool_calls) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, role, content_json, now, tool_calls as i64],
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        self.conn
            .execute(
                "UPDATE sessions SET updated_at = ?1, message_count = message_count + 1 WHERE id = ?2",
                params![now, session_id],
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        Ok(())
    }

    /// Reload every message for a session, ordered by insertion.
    pub fn load_session_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self
            .conn
            .prepare("SELECT role, content FROM messages WHERE session_id = ?1 ORDER BY id ASC")
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let role: String = row.get(0)?;
                let content_json: String = row.get(1)?;
                Ok((role, content_json))
            })
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            let (role, content_json) = row.map_err(|e| ImpError::Database(e.to_string()))?;
            let content: Value = serde_json::from_str(&content_json)?;
            result.push(Message::with_content(&role, content));
        }
        Ok(result)
    }

    /// List the most recent sessions, newest first.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, workdir, created_at, updated_at, title, message_count \
                 FROM sessions ORDER BY updated_at DESC LIMIT ?1",
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    workdir: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    title: row.get(5)?,
                    message_count: row.get(6)?,
                })
            })
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| ImpError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Return the most recent session, optionally scoped to a project.
    pub fn get_latest_session(&self, project: Option<&str>) -> Result<Option<SessionInfo>> {
        match project {
            Some(p) => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, project, workdir, created_at, updated_at, title, message_count \
                         FROM sessions WHERE project = ?1 \
                         AND project NOT LIKE 'subagent-%' \
                         ORDER BY updated_at DESC LIMIT 1",
                    )
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                let mut rows = stmt
                    .query_map(params![p], |row| {
                        Ok(SessionInfo {
                            id: row.get(0)?,
                            project: row.get(1)?,
                            workdir: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                            title: row.get(5)?,
                            message_count: row.get(6)?,
                        })
                    })
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                match rows.next() {
                    Some(row) => Ok(Some(row.map_err(|e| ImpError::Database(e.to_string()))?)),
                    None => Ok(None),
                }
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, project, workdir, created_at, updated_at, title, message_count \
                         FROM sessions WHERE project IS NULL \
                         OR project NOT LIKE 'subagent-%' \
                         ORDER BY updated_at DESC LIMIT 1",
                    )
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                let mut rows = stmt
                    .query_map([], |row| {
                        Ok(SessionInfo {
                            id: row.get(0)?,
                            project: row.get(1)?,
                            workdir: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                            title: row.get(5)?,
                            message_count: row.get(6)?,
                        })
                    })
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                match rows.next() {
                    Some(row) => Ok(Some(row.map_err(|e| ImpError::Database(e.to_string()))?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List recent sessions for a specific project, newest first.
    /// Excludes the given `exclude_id` (the current session just created).
    pub fn list_sessions_for_project(
        &self,
        project: &str,
        exclude_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionInfo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, workdir, created_at, updated_at, title, message_count \
                 FROM sessions WHERE project = ?1 AND id != ?2 AND message_count > 0 \
                 ORDER BY updated_at DESC LIMIT ?3",
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![project, exclude_id, limit as i64], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    workdir: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    title: row.get(5)?,
                    message_count: row.get(6)?,
                })
            })
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| ImpError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Load all conversations for a given date, formatted as readable text.
    /// Returns a vec of (session_title, conversation_text) pairs.
    /// Extracts only human-readable content (user text + assistant text),
    /// skipping tool_use, tool_result, and thinking blocks.
    pub fn load_conversations_for_date(&self, date: &str) -> Result<Vec<(String, String)>> {
        // Find sessions active on this date
        let mut session_stmt = self.conn.prepare(
            "SELECT id, title FROM sessions \
             WHERE date(created_at) = ?1 OR date(updated_at) = ?1 \
             ORDER BY created_at ASC"
        ).map_err(|e| ImpError::Database(e.to_string()))?;

        let sessions: Vec<(String, Option<String>)> = session_stmt
            .query_map(params![date], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| ImpError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut conversations = Vec::new();

        for (session_id, title) in sessions {
            // Skip subagent sessions — they're internal implementation detail
            if title.as_deref().map_or(false, |t| t.starts_with("subagent")) {
                continue;
            }

            let mut msg_stmt = self.conn.prepare(
                "SELECT role, content FROM messages \
                 WHERE session_id = ?1 AND date(created_at) = ?2 \
                 ORDER BY id ASC"
            ).map_err(|e| ImpError::Database(e.to_string()))?;

            let messages: Vec<(String, String)> = msg_stmt
                .query_map(params![session_id, date], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| ImpError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();

            if messages.is_empty() {
                continue;
            }

            let display_title = title.unwrap_or_else(|| session_id[..8.min(session_id.len())].to_string());
            let mut conv_text = String::new();

            for (role, content_json) in &messages {
                let text = extract_readable_text(content_json);
                if text.is_empty() {
                    continue;
                }
                let label = if role == "user" { "Human" } else { "Assistant" };
                conv_text.push_str(&format!("**{}:** {}\n\n", label, text));
            }

            if !conv_text.is_empty() {
                conversations.push((display_title, conv_text));
            }
        }

        Ok(conversations)
    }

    /// Set a human-readable title on a session.
    pub fn update_session_title(&self, session_id: &str, title: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![title, session_id],
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;
        Ok(())
    }

    /// List sessions created/updated on a specific date.
    /// Returns (session_id, project, workdir, created_at) tuples.
    pub fn list_sessions_for_date(&self, date: &str) -> Result<Vec<(String, Option<String>, Option<String>, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project, workdir, created_at FROM sessions \
             WHERE date(created_at) = ?1 OR date(updated_at) = ?1 \
             ORDER BY updated_at DESC"
        ).map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![date], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| ImpError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Get a session by its ID.
    pub fn get_session_by_id(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project, workdir, created_at, updated_at, title, message_count \
             FROM sessions WHERE id = ?1"
        ).map_err(|e| ImpError::Database(e.to_string()))?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok(SessionInfo {
                id: row.get(0)?,
                project: row.get(1)?,
                workdir: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                title: row.get(5)?,
                message_count: row.get(6)?,
            })
        });

        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ImpError::Database(e.to_string())),
        }
    }

    /// Get the first user message for a session (for preview).
    pub fn get_first_user_message(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT content FROM messages \
             WHERE session_id = ?1 AND role = 'user' \
             ORDER BY id ASC LIMIT 1"
        ).map_err(|e| ImpError::Database(e.to_string()))?;

        let result: Option<String> = stmt
            .query_row(params![session_id], |row| row.get(0))
            .ok();

        match result {
            Some(content_json) => Ok(Some(extract_readable_text(&content_json))),
            None => Ok(None),
        }
    }
}
