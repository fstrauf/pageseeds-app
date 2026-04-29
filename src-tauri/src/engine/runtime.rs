/// Runtime abstraction for executing async code with SQLite connections.
///
/// This module provides helper functions to reduce boilerplate when running
/// async executor functions from Tauri commands.
///
/// # Architecture
///
/// ```text
/// Tauri Command (async)
///     ↓
/// Runtime helpers (spawn_blocking + local runtime)
///     ↓
/// SQLite Connection::open() (per-thread connection)
///     ↓
/// tokio::runtime::Runtime::new() (local per-task runtime)
///     ↓
/// async { executor::function(&db, ...).await }
/// ```
///
/// # Future Optimization
///
/// Phase 3: Replace per-task thread spawning with a connection pool using
/// deadpool-sqlite or similar. This would allow true async connection reuse.
use std::path::Path;

/// Open a SQLite connection with proper timeout settings.
///
/// This is a helper to ensure consistent connection configuration.
pub fn open_connection(db_path: &Path) -> Result<rusqlite::Connection, String> {
    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("Failed to open DB: {}", e))?;

    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| format!("Failed to set busy timeout: {}", e))?;

    Ok(conn)
}

/// Create a new Tokio runtime for local async execution.
///
/// This is used inside spawn_blocking to run async code.
pub fn create_local_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Runtime::new().map_err(|e| format!("Failed to create runtime: {}", e))
}

/// Convenience function to spawn a blocking task that opens a DB connection.
///
/// Returns a JoinHandle that can be awaited. The closure receives the opened connection.
pub fn spawn_with_db<F, T>(
    db_path: std::path::PathBuf,
    f: F,
) -> tokio::task::JoinHandle<Result<T, String>>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = open_connection(&db_path)?;
        f(&conn)
    })
}

/// Convenience function to spawn an async blocking task that opens a DB connection.
///
/// Similar to spawn_with_db but creates a local runtime to run async code.
pub fn spawn_async_with_db<F, Fut, T>(
    db_path: std::path::PathBuf,
    f: F,
) -> tokio::task::JoinHandle<Result<T, String>>
where
    F: FnOnce(&rusqlite::Connection) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = open_connection(&db_path)?;
        let rt = create_local_runtime()?;
        rt.block_on(async { f(&conn).await })
    })
}
