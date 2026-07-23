//! Tool catalog loaded from `src-tauri/config/tool_catalog.toml`.
//!
//! TOML is the source of truth for preamble text and `mutates` flags.
//! Runtime constructors live in [`super::build_tool`] and must share names.

use once_cell::sync::Lazy;
use serde::Deserialize;

/// Bundled catalog — path relative to this file under `src/engine/tools/investigate/`.
const TOOL_CATALOG_TOML: &str = include_str!("../../../../config/tool_catalog.toml");

/// One tool's catalog metadata (name + fields from TOML).
#[derive(Debug, Clone)]
pub(crate) struct CatalogEntry {
    pub name: String,
    pub purpose: String,
    pub when_to_use: String,
    pub when_not_to_use: String,
    pub mutates: bool,
}

#[derive(Debug, Deserialize)]
struct ToolDef {
    purpose: String,
    when_to_use: String,
    #[serde(default)]
    when_not_to_use: String,
    mutates: bool,
}

/// Parsed catalog in TOML declaration order (toml::Table preserves order).
pub(crate) static TOOL_CATALOG: Lazy<Vec<CatalogEntry>> = Lazy::new(|| {
    parse_tool_catalog(TOOL_CATALOG_TOML)
        .unwrap_or_else(|e| panic!("tool_catalog.toml failed to parse: {e}"))
});

fn parse_tool_catalog(raw: &str) -> Result<Vec<CatalogEntry>, String> {
    let table: toml::Table =
        toml::from_str(raw).map_err(|e| format!("invalid TOML: {e}"))?;
    let tools = table
        .get("tools")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "missing [tools] table".to_string())?;

    let mut entries = Vec::with_capacity(tools.len());
    for (name, value) in tools {
        let def: ToolDef = value.clone().try_into().map_err(|e| {
            format!("tool '{name}' has invalid fields: {e}")
        })?;
        entries.push(CatalogEntry {
            name: name.clone(),
            purpose: def.purpose,
            when_to_use: def.when_to_use,
            when_not_to_use: def.when_not_to_use,
            mutates: def.mutates,
        });
    }
    Ok(entries)
}

/// Format catalog preamble text for the given access mode.
pub(crate) fn catalog_text_for_access(access: super::InvestigationAccess) -> String {
    use super::InvestigationAccess;

    let header = match access {
        InvestigationAccess::Full => "# Tool catalog for agentic investigation.\n",
        InvestigationAccess::ReadOnly => {
            "# Tool catalog for agentic investigation (read-only).\n"
        }
    };
    let mut s = String::from(header);
    for entry in TOOL_CATALOG.iter().filter(|e| entry_included(e, access)) {
        s.push_str(&format_catalog_section(entry));
    }
    s
}

pub(crate) fn entry_included(entry: &CatalogEntry, access: super::InvestigationAccess) -> bool {
    match access {
        super::InvestigationAccess::Full => true,
        super::InvestigationAccess::ReadOnly => !entry.mutates,
    }
}

fn format_catalog_section(entry: &CatalogEntry) -> String {
    format!(
        "\n[tools.{}]\npurpose = \"{}\"\nwhen_to_use = \"{}\"\nwhen_not_to_use = \"{}\"\nmutates = {}\n",
        entry.name,
        entry.purpose,
        entry.when_to_use,
        entry.when_not_to_use,
        if entry.mutates { "true" } else { "false" },
    )
}

/// All catalog tool names in TOML order.
#[cfg(test)]
pub(crate) fn all_catalog_names() -> Vec<&'static str> {
    TOOL_CATALOG.iter().map(|e| e.name.as_str()).collect()
}

/// Catalog names included for an access mode.
#[cfg(test)]
pub(crate) fn catalog_names_for_access(access: super::InvestigationAccess) -> Vec<&'static str> {
    TOOL_CATALOG
        .iter()
        .filter(|e| entry_included(e, access))
        .map(|e| e.name.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tools::investigate::InvestigationAccess;

    #[test]
    fn catalog_parses_all_expected_tools() {
        let names = all_catalog_names();
        assert_eq!(names.len(), 19, "expected 19 tools in catalog, got {names:?}");
        for required in [
            "gsc_performance",
            "gsc_queries",
            "gsc_movers",
            "article_list",
            "article_frontmatter",
            "article_body_hash",
            "article_title_scan",
            "validate_article",
            "content_audit_report",
            "run_content_audit",
            "cannibalization_clusters",
            "indexing_status",
            "ctr_health",
            "framework_files",
            "article_link_graph",
            "create_task",
            "enqueue_task",
            "get_task_status",
            "write_feature_spec",
        ] {
            assert!(
                names.contains(&required),
                "catalog missing tool {required}; got {names:?}"
            );
        }
    }

    #[test]
    fn catalog_mutates_flags() {
        let mutators: Vec<_> = TOOL_CATALOG
            .iter()
            .filter(|e| e.mutates)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(
            mutators,
            vec![
                "run_content_audit",
                "create_task",
                "enqueue_task",
                "write_feature_spec",
            ]
        );
        assert_eq!(catalog_names_for_access(InvestigationAccess::Full).len(), 19);
        assert_eq!(
            catalog_names_for_access(InvestigationAccess::ReadOnly).len(),
            15
        );
    }

    #[test]
    fn catalog_ro_text_excludes_mutators() {
        let ro = catalog_text_for_access(InvestigationAccess::ReadOnly);
        assert!(ro.contains("[tools.get_task_status]"));
        assert!(ro.contains("mutates = false"));
        for mutator in [
            "create_task",
            "enqueue_task",
            "run_content_audit",
            "write_feature_spec",
        ] {
            assert!(
                !ro.contains(&format!("[tools.{mutator}]")),
                "RO catalog must not contain mutator section [{mutator}]"
            );
        }
        assert!(!ro.contains("mutates = true"));
    }

    #[test]
    fn catalog_full_text_includes_mutators() {
        let full = catalog_text_for_access(InvestigationAccess::Full);
        assert!(full.contains("[tools.get_task_status]"));
        assert!(full.contains("[tools.create_task]"));
        assert!(full.contains("[tools.enqueue_task]"));
        assert!(full.contains("[tools.run_content_audit]"));
        assert!(full.contains("[tools.write_feature_spec]"));
        assert!(full.contains("mutates = true"));
    }
}
