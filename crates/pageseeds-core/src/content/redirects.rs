/// Redirect map (`.github/automation/redirects.csv`) helpers.
///
/// `consolidate_cluster` appends `source,destination,status` rows to this CSV
/// when articles are merged. A source slug has been redirected away: it must
/// no longer validate as an internal link target, and inbound links to it are
/// rewritten to the destination by `merge_rewrite_inbound_links`.
use std::collections::{HashMap, HashSet};

/// Load the full source→destination slug map from the project's redirect CSV.
///
/// Both columns are normalized via [`crate::content::slug::normalize_url_slug`].
/// Returns an empty map when the file is missing or unreadable. Rows with an
/// empty source after normalize are skipped.
pub fn load_redirect_map(project_path: &str) -> HashMap<String, String> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let csv_path = paths.automation_dir.join("redirects.csv");
    let Ok(csv) = std::fs::read_to_string(&csv_path) else {
        return HashMap::new();
    };

    parse_redirect_map_csv(&csv)
}

/// Parse `source,destination,status` CSV body into a normalized slug map.
fn parse_redirect_map_csv(csv: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in csv.lines().skip(1) {
        // header: source,destination,status
        let mut cols = line.split(',');
        let Some(source_raw) = cols.next() else {
            continue;
        };
        let dest_raw = cols.next().unwrap_or("");
        let source = crate::content::slug::normalize_url_slug(source_raw.trim());
        if source.is_empty() {
            continue;
        }
        let destination = crate::content::slug::normalize_url_slug(dest_raw.trim());
        map.insert(source, destination);
    }
    map
}

/// Load the normalized SOURCE slugs from the project's redirect map.
///
/// Returns an empty set when the file is missing or unreadable (most projects
/// have never run a consolidation), so callers can treat "no redirect map" as
/// "nothing redirected".
///
/// Thin view over [`load_redirect_map`] keys — same source set as before.
pub fn load_redirect_source_slugs(project_path: &str) -> HashSet<String> {
    load_redirect_map(project_path).into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_redirects_csv_returns_empty() {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-redirects-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let slugs = load_redirect_source_slugs(dir.to_str().unwrap());
        assert!(slugs.is_empty());
        let map = load_redirect_map(dir.to_str().unwrap());
        assert!(map.is_empty());
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

    #[test]
    fn load_redirect_map_parses_source_and_destination() {
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
             old-legacy-slug,/blog/hub-coffee,301\n\
             ,/blog/orphan-dest,301\n",
        )
        .unwrap();

        let map = load_redirect_map(dir.to_str().unwrap());
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("roast-profile-management").map(String::as_str),
            Some("roast-profile-management")
        );
        assert_eq!(
            map.get("old-legacy-slug").map(String::as_str),
            Some("hub-coffee")
        );
        // Destinations must not appear as map keys (unless also a source).
        assert!(!map.contains_key("hub-coffee"));
        // Empty source after normalize is skipped.
        assert!(!map.values().any(|d| d == "orphan-dest"));

        // Sources-only view still excludes destinations as keys.
        let sources = load_redirect_source_slugs(dir.to_str().unwrap());
        assert_eq!(sources.len(), 2);
        assert!(sources.contains("roast-profile-management"));
        assert!(sources.contains("old-legacy-slug"));
        assert!(!sources.contains("hub-coffee"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
