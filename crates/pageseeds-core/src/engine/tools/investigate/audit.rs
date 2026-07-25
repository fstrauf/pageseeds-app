use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
use super::shared::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tool: content_audit_report
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContentAuditReportArgs;

#[derive(Debug, Clone)]
pub struct ContentAuditReportTool { pub(crate) ctx: InvestigationContext }

impl Tool for ContentAuditReportTool {
    const NAME: &'static str = "content_audit_report";
    type Error = InvestigationToolError;
    type Args = ContentAuditReportArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get the full content_audit.json report with 21 checks per article \
                (keyword usage, meta quality, readability, temporal URLs, page bloat, \
                exact duplicates, literal template variables, title token duplication). \
                Includes health scores and priority rankings.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Primary: read from database
        let db = rusqlite::Connection::open(crate::db::default_db_path())
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to open DB: {e}")))?;
        if let Ok(Some(json)) = crate::db::content_audit::get_audit_report_as_json(&db, &self.ctx.project_id) {
            return Ok(json);
        }

        // Fallback: legacy JSON file during transition
        let paths = self.ctx.paths();
        let audit_path = paths.automation_dir.join("content_audit.json");
        if !audit_path.exists() {
            return Err(InvestigationToolError::NotAvailable(
                "No content audit found. Run run_content_audit first.".into()
            ));
        }
        let content = std::fs::read_to_string(&audit_path)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))?;
        Ok(value)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: run_content_audit
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunContentAuditArgs;

#[derive(Debug, Clone)]
pub struct RunContentAuditTool { pub(crate) ctx: InvestigationContext }

impl Tool for RunContentAuditTool {
    const NAME: &'static str = "run_content_audit";
    type Error = InvestigationToolError;
    type Args = RunContentAuditArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run the 21-check deterministic content audit on all published articles. \
                Writes content_audit.json. Returns summary counts. Must wait for completion \
                before calling content_audit_report to read results.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        use crate::models::task::{
            AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRunPolicy, TaskStatus,
        };

        // Build a minimal task-like struct for the audit function
        let task = Task {
            id: "investigate-audit".to_string(),
            task_type: "content_audit".to_string(),
            project_id: self.ctx.project_id.clone(),
            title: Some("Investigation content audit".to_string()),
            description: None,
            status: TaskStatus::InProgress,
            phase: "audit".to_string(),
            priority: Priority::Medium,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            depends_on: vec![],
            artifacts: vec![],
            run: Default::default(),
        };

        let result = crate::engine::exec::content_audit::exec_content_audit(
            &task, &self.ctx.project_path,
        );

        if !result.success {
            return Err(InvestigationToolError::Execution(result.message));
        }

        // Return the summary
        serde_json::from_str(result.output.as_deref().unwrap_or("{}"))
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to parse audit output: {e}")))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: cannibalization_clusters
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CannibalizationClustersArgs;

#[derive(Debug, Clone)]
pub struct CannibalizationClustersTool { pub(crate) ctx: InvestigationContext }

impl Tool for CannibalizationClustersTool {
    const NAME: &'static str = "cannibalization_clusters";
    type Error = InvestigationToolError;
    type Args = CannibalizationClustersArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get cannibalization clusters and merge recommendations. \
                Shows which articles compete for the same keywords and suggests consolidations. \
                Empty if cannibalization_audit hasn't run yet.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let strategy_path = paths.automation_dir.join("cannibalization_strategy.json");
        if !strategy_path.exists() {
            return Ok(json!({ "clusters": [], "message": "No cannibalization strategy found. Run cannibalization_audit first." }));
        }
        let content = std::fs::read_to_string(&strategy_path)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))?;
        Ok(value)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: indexing_status
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IndexingStatusArgs;

#[derive(Debug, Clone)]
pub struct IndexingStatusTool { pub(crate) ctx: InvestigationContext }

impl Tool for IndexingStatusTool {
    const NAME: &'static str = "indexing_status";
    type Error = InvestigationToolError;
    type Args = IndexingStatusArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get URL indexing status from GSC: how many pages are indexed vs not, \
                reasons for non-indexing, and last inspection dates.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let statuses = crate::gsc::db::list_by_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to load indexing status: {e}")))?;

        let total = statuses.len();
        let indexed = statuses.iter().filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass")).count();
        let not_indexed = total.saturating_sub(indexed);

        let mut reason_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for s in &statuses {
            if let Some(reason) = &s.last_reason_code {
                if reason != "indexed_pass" {
                    *reason_counts.entry(reason.clone()).or_insert(0) += 1;
                }
            }
        }
        let issues_by_reason: Vec<serde_json::Value> = reason_counts
            .into_iter()
            .map(|(reason, count)| json!({ "reason": reason, "count": count }))
            .collect();

        Ok(json!({
            "total_urls": total,
            "indexed": indexed,
            "not_indexed": not_indexed,
            "issues_by_reason": issues_by_reason,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: ctr_health
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CtrHealthArgs;

#[derive(Debug, Clone)]
pub struct CtrHealthTool { pub(crate) ctx: InvestigationContext }

impl Tool for CtrHealthTool {
    const NAME: &'static str = "ctr_health";
    type Error = InvestigationToolError;
    type Args = CtrHealthArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get per-article CTR health: title length, meta description quality, \
                snippet optimization, FAQ schema presence. Shows healthy vs unhealthy counts \
                and specific issues per article.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let project = crate::engine::task_store::get_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Project not found: {e}")))?;
        let project_path = project.path.clone();

        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;

        let repo_root = std::path::Path::new(&project_path);
        let summary = crate::content::ops::build_ctr_health_summary(
            repo_root,
            &articles,
            0,  // pending_fix_tasks
            0,  // completed_audits
            &db,
            &self.ctx.project_id,
        );

        Ok(serde_json::to_value(&summary).unwrap_or(json!({})))
    }
}

