/// Redirect map (`.github/automation/redirects.csv`) helpers.
///
/// `consolidate_cluster` appends `source,destination,status` rows to this CSV
/// when articles are merged. A source slug has been redirected away: it must
/// no longer validate as an internal link target, and inbound links to it are
/// rewritten to the destination by `merge_rewrite_inbound_links`.
use std::collections::HashSet;

/// Load the normalized SOURCE slugs from the project's redirect map.
///
/// Returns an empty set when the file is missing or unreadable (most projects
/// have never run a consolidation), so callers can treat "no redirect map" as
/// "nothing redirected".
pub fn load_redirect_source_slugs(project_path: &str) -> HashSet<String> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let csv_path = paths.automation_dir.join("redirects.csv");
    let Ok(csv) = std::fs::read_to_string(&csv_path) else {
        return HashSet::new();
    };

    csv.lines()
        .skip(1) // header: source,destination,status
        .filter_map(|line| line.split(',').next())
        .map(|source| crate::content::slug::normalize_url_slug(source.trim()))
        .filter(|slug| !slug.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_redirects_csv_returns_empty_set() {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-redirects-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let slugs = load_redirect_source_slugs(dir.to_str().unwrap());
        assert!(slugs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_and_normalizes_source_slugs() {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-redirects-{}",
            uuid::Uuid::new_v4()
        ));
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(
            automation.join("redirects.csv"),
            "source,destination,status\n\
             /blog/248_roast_profile_management,/blog/roast-profile-management,301\n\
             old-legacy-slug,/blog/hub-coffee,301\n",
        )
        .unwrap();

        let slugs = load_redirect_source_slugs(dir.to_str().unwrap());
        assert_eq!(slugs.len(), 2);
        assert!(slugs.contains("roast-profile-management"));
        assert!(slugs.contains("old-legacy-slug"));
        // Destinations are not sources.
        assert!(!slugs.contains("hub-coffee"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
