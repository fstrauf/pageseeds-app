use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ─── Status / mode enums ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskStatus {
    #[default]
    Todo,
    Queued,
    InProgress,
    Review,
    Done,
    Cancelled,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Queued => "queued",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Failed => "failed",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for TaskStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for TaskStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "todo" => Ok(TaskStatus::Todo),
            "queued" => Ok(TaskStatus::Queued),
            "in_progress" => Ok(TaskStatus::InProgress),
            "review" => Ok(TaskStatus::Review),
            "done" => Ok(TaskStatus::Done),
            "cancelled" => Ok(TaskStatus::Cancelled),
            "failed" => Ok(TaskStatus::Failed),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown task status: {other}").into(),
            )),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskRunPolicy {
    /// System may auto-enqueue this task; user may also enqueue it manually.
    AutoEnqueue,
    /// User must explicitly enqueue; system will not auto-enqueue.
    #[default]
    UserEnqueue,
}

impl TaskRunPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskRunPolicy::AutoEnqueue => "auto_enqueue",
            TaskRunPolicy::UserEnqueue => "user_enqueue",
        }
    }
}

impl std::fmt::Display for TaskRunPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for TaskRunPolicy {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for TaskRunPolicy {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "auto_enqueue" => Ok(TaskRunPolicy::AutoEnqueue),
            "user_enqueue" => Ok(TaskRunPolicy::UserEnqueue),
            // Legacy execution_mode values
            "automatic" | "batchable" => Ok(TaskRunPolicy::AutoEnqueue),
            "manual" | "spec" | _ => Ok(TaskRunPolicy::UserEnqueue),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskReviewSurface {
    #[default]
    None,
    KeywordPicker,
    RedditPicker,
    CannibalizationPicker,
    FollowUpTasks,
    ArtifactReview,
}

impl TaskReviewSurface {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskReviewSurface::None => "none",
            TaskReviewSurface::KeywordPicker => "keyword_picker",
            TaskReviewSurface::RedditPicker => "reddit_picker",
            TaskReviewSurface::CannibalizationPicker => "cannibalization_picker",
            TaskReviewSurface::FollowUpTasks => "follow_up_tasks",
            TaskReviewSurface::ArtifactReview => "artifact_review",
        }
    }
}

impl std::fmt::Display for TaskReviewSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for TaskReviewSurface {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for TaskReviewSurface {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "none" => Ok(TaskReviewSurface::None),
            "keyword_picker" => Ok(TaskReviewSurface::KeywordPicker),
            "reddit_picker" => Ok(TaskReviewSurface::RedditPicker),
            "cannibalization_picker" => Ok(TaskReviewSurface::CannibalizationPicker),
            "follow_up_tasks" => Ok(TaskReviewSurface::FollowUpTasks),
            "artifact_review" => Ok(TaskReviewSurface::ArtifactReview),
            _ => Ok(TaskReviewSurface::None),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FollowUpPolicy {
    #[default]
    None,
    BackendAuto,
    UserSelection,
}

impl FollowUpPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            FollowUpPolicy::None => "none",
            FollowUpPolicy::BackendAuto => "backend_auto",
            FollowUpPolicy::UserSelection => "user_selection",
        }
    }
}

impl std::fmt::Display for FollowUpPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for FollowUpPolicy {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for FollowUpPolicy {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "none" => Ok(FollowUpPolicy::None),
            "backend_auto" => Ok(FollowUpPolicy::BackendAuto),
            "user_selection" => Ok(FollowUpPolicy::UserSelection),
            _ => Ok(FollowUpPolicy::None),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentPolicy {
    #[default]
    #[serde(rename = "none")]
    None,
    Required,
    Optional,
}

impl AgentPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentPolicy::None => "none",
            AgentPolicy::Required => "required",
            AgentPolicy::Optional => "optional",
        }
    }
}

impl std::fmt::Display for AgentPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for AgentPolicy {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for AgentPolicy {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "required" => Ok(AgentPolicy::Required),
            "optional" => Ok(AgentPolicy::Optional),
            _ => Ok(AgentPolicy::None),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Priority {
    High,
    Medium,
    #[default]
    Low,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for Priority {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for Priority {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "high" => Ok(Priority::High),
            "low" => Ok(Priority::Low),
            _ => Ok(Priority::Medium),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TaskArtifact {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub artifact_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TaskRun {
    pub attempts: u32,
    pub last_error: Option<String>,
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Task {
    pub id: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub phase: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub run_policy: TaskRunPolicy,
    pub review_surface: TaskReviewSurface,
    pub follow_up_policy: FollowUpPolicy,
    pub agent_policy: AgentPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub project_id: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<TaskArtifact>,
    #[serde(default)]
    pub run: TaskRun,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_before: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
