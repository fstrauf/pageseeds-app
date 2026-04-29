use crate::error::{Error, Result};
/// Global application settings storage.
///
/// These settings are NOT project-specific and apply to the entire application.
/// Use this for user preferences like agent provider, UI settings, etc.
use rusqlite::{Connection, OptionalExtension};

/// Default agent provider if none is set globally.
pub const DEFAULT_AGENT_PROVIDER: &str = "kimi";

/// Default Kimi backend mode: "auto" tries bridge then falls back to direct CLI.
pub const DEFAULT_KIMI_BACKEND_MODE: &str = "auto";

/// Get a global setting value by key.
/// Returns None if the key doesn't exist.
pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM global_settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.into())
}

/// Set a global setting value.
/// Creates the key if it doesn't exist, updates if it does.
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO global_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now],
    )?;
    Ok(())
}

/// Get the global agent provider setting.
/// Falls back to DEFAULT_AGENT_PROVIDER if not set.
pub fn get_agent_provider(conn: &Connection) -> String {
    get(conn, "agent_provider")
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_AGENT_PROVIDER.to_string())
}

/// Resolve the agent provider for a project.
/// Uses the project's legacy `agent_provider` if set and non-empty,
/// otherwise falls back to the global setting.
pub fn resolve_agent_provider(conn: &Connection, legacy: Option<&str>) -> String {
    legacy
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| get_agent_provider(conn))
}

/// Set the global agent provider setting.
pub fn set_agent_provider(conn: &Connection, provider: &str) -> Result<()> {
    log::info!("[global_settings] Setting agent_provider to '{}'", provider);
    set(conn, "agent_provider", provider)?;

    // Verify it was saved
    let saved = get_agent_provider(conn);
    if saved != provider {
        return Err(Error::Other(format!(
            "Failed to save agent_provider: expected '{}', got '{}'",
            provider, saved
        )));
    }

    log::info!(
        "[global_settings] Successfully saved agent_provider '{}'",
        provider
    );
    Ok(())
}

/// Get the global Kimi backend mode setting.
/// Falls back to DEFAULT_KIMI_BACKEND_MODE if not set.
pub fn get_kimi_backend_mode(conn: &Connection) -> String {
    get(conn, "kimi_backend_mode")
        .ok()
        .flatten()
        .unwrap_or_else(|| DEFAULT_KIMI_BACKEND_MODE.to_string())
}

/// Set the global Kimi backend mode setting.
/// Valid values: "auto", "bridge", "direct".
pub fn set_kimi_backend_mode(conn: &Connection, mode: &str) -> Result<()> {
    log::info!("[global_settings] Setting kimi_backend_mode to '{}'", mode);
    set(conn, "kimi_backend_mode", mode)?;

    let saved = get_kimi_backend_mode(conn);
    if saved != mode {
        return Err(Error::Other(format!(
            "Failed to save kimi_backend_mode: expected '{}', got '{}'",
            mode, saved
        )));
    }

    log::info!(
        "[global_settings] Successfully saved kimi_backend_mode '{}'",
        mode
    );
    Ok(())
}

/// Get all global settings as a Vec of (key, value) tuples.
pub fn get_all(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT key, value FROM global_settings ORDER BY key")?;

    let settings: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(settings)
}

/// Delete a global setting.
pub fn delete(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM global_settings WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE global_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_get_set() {
        let conn = in_memory_db();

        // Initially not set
        assert_eq!(get(&conn, "test_key").unwrap(), None);

        // Set it
        set(&conn, "test_key", "test_value").unwrap();
        assert_eq!(
            get(&conn, "test_key").unwrap(),
            Some("test_value".to_string())
        );

        // Update it
        set(&conn, "test_key", "new_value").unwrap();
        assert_eq!(
            get(&conn, "test_key").unwrap(),
            Some("new_value".to_string())
        );
    }

    #[test]
    fn test_agent_provider() {
        let conn = in_memory_db();

        // Default value
        assert_eq!(get_agent_provider(&conn), DEFAULT_AGENT_PROVIDER);

        // Set custom value
        set_agent_provider(&conn, "kimi").unwrap();
        assert_eq!(get_agent_provider(&conn), "kimi");

        // Set another value
        set_agent_provider(&conn, "claude").unwrap();
        assert_eq!(get_agent_provider(&conn), "claude");
    }

    #[test]
    fn test_get_all() {
        let conn = in_memory_db();

        set(&conn, "key1", "value1").unwrap();
        set(&conn, "key2", "value2").unwrap();

        let all = get_all(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&("key1".to_string(), "value1".to_string())));
        assert!(all.contains(&("key2".to_string(), "value2".to_string())));
    }

    #[test]
    fn test_delete() {
        let conn = in_memory_db();

        set(&conn, "to_delete", "value").unwrap();
        assert_eq!(get(&conn, "to_delete").unwrap(), Some("value".to_string()));

        delete(&conn, "to_delete").unwrap();
        assert_eq!(get(&conn, "to_delete").unwrap(), None);
    }
}
