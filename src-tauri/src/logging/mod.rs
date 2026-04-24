//! Centralized logging system for PageSeeds
//!
//! Captures logs from both frontend and backend, stores them persistently,
//! and provides retrieval capabilities for debugging and agentic analysis.

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

// Global log buffer for recent logs (in-memory cache)
static LOG_BUFFER: OnceLock<Mutex<Vec<LogEntry>>> = OnceLock::new();

fn get_log_buffer() -> &'static Mutex<Vec<LogEntry>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(Vec::with_capacity(1000)))
}

/// Log entry from any source (frontend, backend, agent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: Option<i64>,
    pub timestamp: String,
    pub level: LogLevel,
    pub source: LogSource,
    pub component: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

impl LogLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "debug" => LogLevel::Debug,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    Frontend,
    Backend,
    Agent,
    System,
}

impl std::fmt::Display for LogSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogSource::Frontend => write!(f, "frontend"),
            LogSource::Backend => write!(f, "backend"),
            LogSource::Agent => write!(f, "agent"),
            LogSource::System => write!(f, "system"),
        }
    }
}

impl LogSource {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "frontend" | "ui" => LogSource::Frontend,
            "agent" | "kimi" | "copilot" | "claude" => LogSource::Agent,
            "system" => LogSource::System,
            _ => LogSource::Backend,
        }
    }
}

/// SQL for creating the logs table
pub const LOGS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS app_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'info',
    source TEXT NOT NULL DEFAULT 'backend',
    component TEXT NOT NULL,
    message TEXT NOT NULL,
    metadata TEXT, -- JSON blob
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON app_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_logs_level ON app_logs(level);
CREATE INDEX IF NOT EXISTS idx_logs_source ON app_logs(source);
CREATE INDEX IF NOT EXISTS idx_logs_component ON app_logs(component);
CREATE INDEX IF NOT EXISTS idx_logs_session ON app_logs(session_id);
"#;

/// Initialize logging tables
pub fn init_logs_table(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(LOGS_TABLE_SQL)
        .map_err(|e| format!("Failed to create logs table: {}", e))
}

/// Store a log entry in the database
pub fn store_log(conn: &Connection, entry: &LogEntry) -> Result<i64, String> {
    let metadata_json = entry.metadata.as_ref()
        .map(|m| serde_json::to_string(m).unwrap_or_default())
        .unwrap_or_default();
    
    conn.execute(
        "INSERT INTO app_logs (timestamp, level, source, component, message, metadata, session_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            &entry.timestamp,
            entry.level.to_string(),
            entry.source.to_string(),
            &entry.component,
            &entry.message,
            if metadata_json.is_empty() { None } else { Some(metadata_json) },
            &entry.session_id,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| format!("Failed to store log: {}", e))?;
    
    let id = conn.last_insert_rowid();
    
    // Also add to in-memory buffer
    if let Ok(mut buffer) = get_log_buffer().lock() {
        let mut entry_with_id = entry.clone();
        entry_with_id.id = Some(id);
        buffer.push(entry_with_id);
        
        // Keep only last 1000 entries in memory
        if buffer.len() > 1000 {
            buffer.remove(0);
        }
    }
    
    Ok(id)
}

/// Quick log helper for backend code
pub fn log(
    conn: &Connection,
    level: LogLevel,
    component: &str,
    message: &str,
    metadata: Option<serde_json::Value>,
) {
    let source = if component.starts_with("frontend") {
        LogSource::Frontend
    } else if component.starts_with("agent") {
        LogSource::Agent
    } else if component.starts_with("system") {
        LogSource::System
    } else {
        LogSource::Backend
    };
    let entry = LogEntry {
        id: None,
        timestamp: Utc::now().to_rfc3339(),
        level,
        source,
        component: component.to_string(),
        message: message.to_string(),
        metadata,
        session_id: get_session_id(),
    };
    
    let _ = store_log(conn, &entry);
}

/// Get session ID for grouping logs
fn get_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
    
    thread_local! {
        static SESSION_ID: String = format!("session-{}", 
            SESSION_COUNTER.fetch_add(1, Ordering::SeqCst));
    }
    
    SESSION_ID.with(|id| id.clone())
}

/// Query logs with filters
pub fn query_logs(
    conn: &Connection,
    filters: &LogQueryFilters,
    limit: usize,
    offset: usize,
) -> Result<Vec<LogEntry>, String> {
    let mut sql = String::from(
        "SELECT id, timestamp, level, source, component, message, metadata, session_id 
         FROM app_logs WHERE 1=1"
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    
    if let Some(level) = &filters.level {
        sql.push_str(" AND level = ?");
        params.push(Box::new(level.to_string()));
    }
    
    if let Some(source) = &filters.source {
        sql.push_str(" AND source = ?");
        params.push(Box::new(source.to_string()));
    }
    
    if let Some(component) = &filters.component {
        sql.push_str(" AND component LIKE ?");
        params.push(Box::new(format!("%{}%", component)));
    }
    
    if let Some(session_id) = &filters.session_id {
        sql.push_str(" AND session_id = ?");
        params.push(Box::new(session_id.clone()));
    }
    
    if let Some(query) = &filters.search_query {
        sql.push_str(" AND (message LIKE ? OR component LIKE ?)");
        let pattern = format!("%{}%", query);
        params.push(Box::new(pattern.clone()));
        params.push(Box::new(pattern));
    }
    
    sql.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
        .map(|p| p.as_ref())
        .collect();
    
    let mut stmt = conn.prepare(&sql)
        .map_err(|e| format!("Failed to prepare query: {}", e))?;
    
    let logs = stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
        let metadata_str: Option<String> = row.get(6)?;
        let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());
        
        Ok(LogEntry {
            id: Some(row.get(0)?),
            timestamp: row.get(1)?,
            level: LogLevel::from_str(row.get::<_, String>(2)?.as_str()),
            source: LogSource::from_str(row.get::<_, String>(3)?.as_str()),
            component: row.get(4)?,
            message: row.get(5)?,
            metadata,
            session_id: row.get(7)?,
        })
    })
    .map_err(|e| format!("Query failed: {}", e))?
    .filter_map(|r| r.ok())
    .collect();
    
    Ok(logs)
}

/// Get recent logs from memory buffer (fast)
pub fn get_recent_logs(limit: usize) -> Vec<LogEntry> {
    if let Ok(buffer) = get_log_buffer().lock() {
        buffer.iter().rev().take(limit).cloned().collect()
    } else {
        vec![]
    }
}

/// Clear old logs from database
pub fn clear_old_logs(conn: &Connection, days_to_keep: i64) -> Result<usize, String> {
    let cutoff = (Utc::now() - chrono::Duration::days(days_to_keep)).to_rfc3339();
    
    let count = conn.execute(
        "DELETE FROM app_logs WHERE timestamp < ?1",
        [&cutoff],
    )
    .map_err(|e| format!("Failed to clear old logs: {}", e))?;
    
    Ok(count)
}

/// Filters for querying logs
#[derive(Debug, Default)]
pub struct LogQueryFilters {
    pub level: Option<LogLevel>,
    pub source: Option<LogSource>,
    pub component: Option<String>,
    pub session_id: Option<String>,
    pub search_query: Option<String>,
}

/// Statistics about stored logs
#[derive(Debug, Serialize)]
pub struct LogStats {
    pub total_count: i64,
    pub error_count: i64,
    pub warn_count: i64,
    pub info_count: i64,
    pub debug_count: i64,
    pub frontend_count: i64,
    pub backend_count: i64,
    pub agent_count: i64,
}

pub fn get_log_stats(conn: &Connection) -> Result<LogStats, String> {
    let stats: (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT 
            COUNT(*),
            SUM(CASE WHEN level = 'error' THEN 1 ELSE 0 END),
            SUM(CASE WHEN level = 'warn' THEN 1 ELSE 0 END),
            SUM(CASE WHEN level = 'info' THEN 1 ELSE 0 END),
            SUM(CASE WHEN level = 'debug' THEN 1 ELSE 0 END)
         FROM app_logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )
    .map_err(|e| format!("Failed to get level stats: {}", e))?;
    
    let source_stats: (Option<i64>, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT 
            SUM(CASE WHEN source = 'frontend' THEN 1 ELSE 0 END),
            SUM(CASE WHEN source = 'backend' THEN 1 ELSE 0 END),
            SUM(CASE WHEN source = 'agent' THEN 1 ELSE 0 END)
         FROM app_logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|e| format!("Failed to get source stats: {}", e))?;
    
    Ok(LogStats {
        total_count: stats.0,
        error_count: stats.1.unwrap_or(0),
        warn_count: stats.2.unwrap_or(0),
        info_count: stats.3.unwrap_or(0),
        debug_count: stats.4.unwrap_or(0),
        frontend_count: source_stats.0.unwrap_or(0),
        backend_count: source_stats.1.unwrap_or(0),
        agent_count: source_stats.2.unwrap_or(0),
    })
}

/// Export logs to JSON string
pub fn export_logs_to_json(conn: &Connection, filters: &LogQueryFilters) -> Result<String, String> {
    let logs = query_logs(conn, filters, 10000, 0)?;
    serde_json::to_string_pretty(&logs)
        .map_err(|e| format!("Failed to serialize logs: {}", e))
}
