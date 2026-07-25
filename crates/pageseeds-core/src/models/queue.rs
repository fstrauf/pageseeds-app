use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Status of a queue run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QueueRunStatus {
    #[default]
    Idle,
    Running,
    Paused,
    Finished,
    Failed,
}

impl QueueRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueueRunStatus::Idle => "idle",
            QueueRunStatus::Running => "running",
            QueueRunStatus::Paused => "paused",
            QueueRunStatus::Finished => "finished",
            QueueRunStatus::Failed => "failed",
        }
    }
}

impl std::fmt::Display for QueueRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for QueueRunStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for QueueRunStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "idle" => Ok(QueueRunStatus::Idle),
            "running" => Ok(QueueRunStatus::Running),
            "paused" => Ok(QueueRunStatus::Paused),
            "finished" => Ok(QueueRunStatus::Finished),
            "failed" => Ok(QueueRunStatus::Failed),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown queue run status: {other}").into(),
            )),
        }
    }
}

/// Status of an individual item within a queue run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QueueItemStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl QueueItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueueItemStatus::Pending => "pending",
            QueueItemStatus::Running => "running",
            QueueItemStatus::Completed => "completed",
            QueueItemStatus::Failed => "failed",
            QueueItemStatus::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for QueueItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for QueueItemStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for QueueItemStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "pending" => Ok(QueueItemStatus::Pending),
            "running" => Ok(QueueItemStatus::Running),
            "completed" => Ok(QueueItemStatus::Completed),
            "failed" => Ok(QueueItemStatus::Failed),
            "skipped" => Ok(QueueItemStatus::Skipped),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown queue item status: {other}").into(),
            )),
        }
    }
}

/// A durable queue run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct QueueRun {
    pub id: String,
    pub status: QueueRunStatus,
    pub pause_on_error: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// An item within a queue run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct QueueItem {
    pub run_id: String,
    pub position: i64,
    pub task_id: String,
    pub project_id: String,
    pub status: QueueItemStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    // Populated at query time from tasks table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
}

/// Full snapshot of the active queue for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct QueueSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<QueueRun>,
    pub items: Vec<QueueItem>,
}

/// Request to enqueue tasks.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct EnqueueRequest {
    pub items: Vec<EnqueueItem>,
    #[serde(default)]
    pub mode: EnqueueMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EnqueueMode {
    #[default]
    Append,
    Next,
}

/// A single task to enqueue.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct EnqueueItem {
    pub task_id: String,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
}
