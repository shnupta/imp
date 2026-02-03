use crate::client::Message;
use crate::error::{ImpError, Result};
use rusqlite::{params, Connection};
use serde_json::Value;

pub struct SessionInfo {
    pub id: String,
    pub project: Option<String>,
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

        Ok(Self { conn })
    }

    /// Create a new session row and return its UUID.
    pub fn create_session(&self, project: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO sessions (id, project, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, project, now, now],
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
                "SELECT id, project, created_at, updated_at, title, message_count \
                 FROM sessions ORDER BY updated_at DESC LIMIT ?1",
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    title: row.get(4)?,
                    message_count: row.get(5)?,
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
                        "SELECT id, project, created_at, updated_at, title, message_count \
                         FROM sessions WHERE project = ?1 ORDER BY updated_at DESC LIMIT 1",
                    )
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                let mut rows = stmt
                    .query_map(params![p], |row| {
                        Ok(SessionInfo {
                            id: row.get(0)?,
                            project: row.get(1)?,
                            created_at: row.get(2)?,
                            updated_at: row.get(3)?,
                            title: row.get(4)?,
                            message_count: row.get(5)?,
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
                        "SELECT id, project, created_at, updated_at, title, message_count \
                         FROM sessions ORDER BY updated_at DESC LIMIT 1",
                    )
                    .map_err(|e| ImpError::Database(e.to_string()))?;

                let mut rows = stmt
                    .query_map([], |row| {
                        Ok(SessionInfo {
                            id: row.get(0)?,
                            project: row.get(1)?,
                            created_at: row.get(2)?,
                            updated_at: row.get(3)?,
                            title: row.get(4)?,
                            message_count: row.get(5)?,
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
                "SELECT id, project, created_at, updated_at, title, message_count \
                 FROM sessions WHERE project = ?1 AND id != ?2 AND message_count > 0 \
                 ORDER BY updated_at DESC LIMIT ?3",
            )
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![project, exclude_id, limit as i64], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    title: row.get(4)?,
                    message_count: row.get(5)?,
                })
            })
            .map_err(|e| ImpError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| ImpError::Database(e.to_string()))?);
        }
        Ok(result)
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
}
