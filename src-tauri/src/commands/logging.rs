//! Logging commands for frontend integration

use crate::commands::AppState;
use crate::logging::{LogEntry, LogLevel, LogSource, LogQueryFilters, LogStats};
use serde::{Deserialize, Serialize};

/// Submit a log entry from the frontend
#[tauri::command]
pub async fn submit_log(
    entry: LogEntryInput,
    state: tauri::State<'_, AppState>,
) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    
    let log_entry = crate::logging::LogEntry {
        id: None,
        timestamp: entry.timestamp,
        level: entry.level.into(),
        source: LogSource::Frontend,
        component: entry.component,
        message: entry.message,
        metadata: entry.metadata,
        session_id: entry.session_id,
    };
    
    crate::logging::store_log(&conn, &log_entry)
}

/// Query logs with filters
#[tauri::command]
pub async fn query_logs(
    filters: LogFiltersInput,
    limit: usize,
    offset: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<LogEntryOutput>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    
    let query_filters = LogQueryFilters {
        level: filters.level.map(|l| l.into()),
        source: filters.source.map(|s| s.into()),
        component: filters.component,
        session_id: filters.session_id,
        search_query: filters.search_query,
    };
    
    let logs = crate::logging::query_logs(&conn, &query_filters, limit, offset)?;
    
    Ok(logs.into_iter().map(|l| LogEntryOutput {
        id: l.id,
        timestamp: l.timestamp,
        level: l.level.to_string(),
        source: l.source.to_string(),
        component: l.component,
        message: l.message,
        metadata: l.metadata,
        session_id: l.session_id,
    }).collect())
}

/// Get recent logs (fast, from memory)
#[tauri::command]
pub async fn get_recent_logs(
    limit: usize,
) -> Result<Vec<LogEntryOutput>, String> {
    let logs = crate::logging::get_recent_logs(limit);
    
    Ok(logs.into_iter().map(|l| LogEntryOutput {
        id: l.id,
        timestamp: l.timestamp,
        level: l.level.to_string(),
        source: l.source.to_string(),
        component: l.component,
        message: l.message,
        metadata: l.metadata,
        session_id: l.session_id,
    }).collect())
}

/// Get log statistics
#[tauri::command]
pub async fn get_log_stats(
    state: tauri::State<'_, AppState>,
) -> Result<LogStats, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    crate::logging::get_log_stats(&conn)
}

/// Clear old logs
#[tauri::command]
pub async fn clear_old_logs(
    days_to_keep: i64,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    crate::logging::clear_old_logs(&conn, days_to_keep)
}

/// Export logs to JSON
#[tauri::command]
pub async fn export_logs(
    filters: LogFiltersInput,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    
    let query_filters = LogQueryFilters {
        level: filters.level.map(|l| l.into()),
        source: filters.source.map(|s| s.into()),
        component: filters.component,
        session_id: filters.session_id,
        search_query: filters.search_query,
    };
    
    crate::logging::export_logs_to_json(&conn, &query_filters)
}

// Input/Output types for Tauri commands

#[derive(Debug, Deserialize)]
pub struct LogEntryInput {
    pub timestamp: String,
    pub level: String,
    pub component: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct LogEntryOutput {
    pub id: Option<i64>,
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub component: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub session_id: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct LogFiltersInput {
    pub level: Option<String>,
    pub source: Option<String>,
    pub component: Option<String>,
    pub session_id: Option<String>,
    pub search_query: Option<String>,
}

/// Batch submit multiple logs (for frontend buffering)
#[tauri::command]
pub async fn submit_logs_batch(
    entries: Vec<LogEntryInput>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<i64>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
    
    let mut ids = Vec::new();
    for entry in entries {
        let log_entry = crate::logging::LogEntry {
            id: None,
            timestamp: entry.timestamp,
            level: entry.level.into(),
            source: LogSource::Frontend,
            component: entry.component,
            message: entry.message,
            metadata: entry.metadata,
            session_id: entry.session_id,
        };
        
        match crate::logging::store_log(&conn, &log_entry) {
            Ok(id) => ids.push(id),
            Err(e) => eprintln!("Failed to store log: {}", e),
        }
    }
    
    Ok(ids)
}
