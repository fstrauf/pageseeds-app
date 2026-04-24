use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProjectMode {
    #[default]
    Workspace,
    LiveSite,
}

impl ProjectMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectMode::Workspace => "workspace",
            ProjectMode::LiveSite => "live_site",
        }
    }
}

impl std::fmt::Display for ProjectMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for ProjectMode {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for ProjectMode {
    fn column_result(
        value: rusqlite::types::ValueRef<'_>,
    ) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "workspace" => Ok(ProjectMode::Workspace),
            "live_site" => Ok(ProjectMode::LiveSite),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown project mode: {other}").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sitemap_url: Option<String>,
    #[serde(default)]
    pub project_mode: ProjectMode,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seo_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ProjectCreate {
    pub name: String,
    pub path: Option<String>,
    pub content_dir: Option<String>,
    pub site_url: Option<String>,
    pub site_id: Option<String>,
    pub sitemap_url: Option<String>,
    #[serde(default)]
    pub project_mode: ProjectMode,
}
