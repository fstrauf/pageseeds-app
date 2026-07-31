//! MDX filename conventions and post-write rename helpers.
//!
//! Extracted verbatim from `engine/workflows/handlers.rs` as part of Stage A.1
//! of the structural-debt cleanup (issue #4). The only edits are visibility
//! adjustments (`fn` → `pub(crate) fn`, and `next_id` field → `pub(crate)`)
//! required to satisfy the new module boundary; the function bodies are
//! byte-for-byte identical to the pre-move source.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub(crate) struct NumberedMdxStyle {
    pub(crate) next_id: i64,
}

pub(crate) fn detect_numbered_mdx_style(dir: &std::path::Path) -> Option<NumberedMdxStyle> {
    let mut count = 0i64;
    let mut max_id = 0i64;

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if let Some(id) = parse_numeric_prefix(name) {
            count += 1;
            if id > max_id {
                max_id = id;
            }
        }
    }

    // Only enforce when this style is clearly established in the repo.
    if count >= 5 {
        Some(NumberedMdxStyle {
            next_id: max_id + 1,
        })
    } else {
        None
    }
}

/// Parse a leading `{digits}_` numeric prefix from a markdown filename.
///
/// Shared by numbered-style path allocation and sync duplicate-prefix checks.
pub(crate) fn parse_numeric_prefix(filename: &str) -> Option<i64> {
    let prefix = filename.split_once('_')?.0;
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        prefix.parse::<i64>().ok()
    } else {
        None
    }
}

/// Scan `dir` once and return every numeric prefix already used by a markdown
/// file (e.g. `189_other.mdx` occupies id 189 regardless of slug).
///
/// Single source of truth for prefix occupancy: both [`next_article_path`] and
/// [`rename_new_files_to_numbered_mdx`] consult this set so allocation never
/// collides with an existing `{id}_*` file under a different stem.
fn occupied_numeric_prefixes(dir: &std::path::Path) -> HashSet<i64> {
    crate::content::locator::collect_markdown_files(dir)
        .into_iter()
        .filter_map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(parse_numeric_prefix)
        })
        .collect()
}

fn normalize_slug_underscored(stem: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;

    for ch in stem.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "article".to_string()
    } else {
        trimmed
    }
}

/// Compute the exact target path for a new article file, fully deterministically.
///
/// With an established numbered style the file is `{next_id}_{slug}.mdx`.
/// Prefix occupancy is the single invariant: any existing markdown file whose
/// name starts with `{id}_` occupies that id (regardless of slug), and
/// allocation walks `next_id` upward until a free id is found. Without a
/// numbered style the file is `{slug}.mdx`, with a numeric suffix appended if
/// that name is taken.
///
/// Used both to tell the agent the exact path to write (prompt directive) and
/// by the executor when it must persist returned MDX content itself.
pub(crate) fn next_article_path(
    dir: &std::path::Path,
    style: Option<NumberedMdxStyle>,
    stem: &str,
) -> std::path::PathBuf {
    let slug = normalize_slug_underscored(stem);
    match style {
        Some(style) => {
            let occupied = occupied_numeric_prefixes(dir);
            let mut next_id = style.next_id;
            while occupied.contains(&next_id) {
                next_id += 1;
            }
            dir.join(format!("{}_{}.mdx", next_id, slug))
        }
        None => {
            let mut candidate = dir.join(format!("{}.mdx", slug));
            let mut n = 2i64;
            while candidate.exists() {
                candidate = dir.join(format!("{}_{}.mdx", slug, n));
                n += 1;
            }
            candidate
        }
    }
}

pub(crate) fn rename_new_files_to_numbered_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    start_id: i64,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();
    let mut next_id = start_id;
    // Shared occupancy set with next_article_path. Insert each allocated id so
    // later renames in this same pass cannot reuse it.
    let mut occupied = occupied_numeric_prefixes(dir);

    for path in crate::content::locator::collect_markdown_files(dir) {
        // Rename only newly created files from this run, not existing repo files.
        if before.contains_key(&path) {
            continue;
        }

        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if parse_numeric_prefix(name).is_some() {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("article");
        let slug = normalize_slug_underscored(stem);

        while occupied.contains(&next_id) {
            next_id += 1;
        }
        let target = dir.join(format!("{}_{}.mdx", next_id, slug));

        if std::fs::rename(&path, &target).is_ok() {
            occupied.insert(next_id);
            renamed.push((path, target));
            next_id += 1;
        }
    }

    renamed
}

pub(crate) fn snapshot_markdown_mtime(
    dir: &std::path::Path,
) -> std::collections::HashMap<std::path::PathBuf, std::time::SystemTime> {
    let mut out = std::collections::HashMap::new();
    for path in crate::content::locator::collect_markdown_files(dir) {
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(mtime) = meta.modified() {
                out.insert(path, mtime);
            }
        }
    }
    out
}

pub(crate) fn rename_new_or_modified_md_to_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if !is_md {
            continue;
        }

        let modified = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());

        let changed_since_before = match (before.get(&path), modified) {
            (None, Some(_)) => true,
            (Some(prev), Some(now)) => now > *prev,
            _ => false,
        };

        if !changed_since_before {
            continue;
        }

        let target = path.with_extension("mdx");
        if target.exists() {
            log::warn!(
                "[content_mdx] skipping rename {} -> {} because target exists",
                path.display(),
                target.display()
            );
            continue;
        }

        if std::fs::rename(&path, &target).is_ok() {
            renamed.push((path, target));
        }
    }

    renamed
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-naming-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn next_article_path_uses_numbered_style() {
        let dir = temp_dir();
        let path = next_article_path(&dir, Some(NumberedMdxStyle { next_id: 7 }), "Gamma Scalping!");
        assert_eq!(path, dir.join("7_gamma_scalping.mdx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_article_path_increments_past_occupied_ids() {
        let dir = temp_dir();
        std::fs::write(dir.join("7_gamma_scalping.mdx"), "x").unwrap();
        std::fs::write(dir.join("8_gamma_scalping.mdx"), "x").unwrap();
        let path = next_article_path(&dir, Some(NumberedMdxStyle { next_id: 7 }), "gamma scalping");
        assert_eq!(path, dir.join("9_gamma_scalping.mdx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_article_path_skips_prefix_occupied_by_different_slug() {
        // 189_other.mdx occupies prefix 189; asking for slug "something" at
        // next_id 189 must not return 189_something.mdx.
        let dir = temp_dir();
        std::fs::write(dir.join("189_other.mdx"), "x").unwrap();
        let path = next_article_path(
            &dir,
            Some(NumberedMdxStyle { next_id: 189 }),
            "something",
        );
        assert_eq!(path, dir.join("190_something.mdx"));
        assert_ne!(path, dir.join("189_something.mdx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_article_path_without_style_suffixes_collisions() {
        let dir = temp_dir();
        std::fs::write(dir.join("gamma_scalping.mdx"), "x").unwrap();
        let path = next_article_path(&dir, None, "gamma scalping");
        assert_eq!(path, dir.join("gamma_scalping_2.mdx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_article_path_empty_stem_falls_back_to_article() {
        let dir = temp_dir();
        let path = next_article_path(&dir, None, "!!!");
        assert_eq!(path, dir.join("article.mdx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_new_files_skips_prefix_occupied_by_different_slug() {
        // Existing 189_other.mdx occupies 189; a new unnumbered file must not
        // be renamed to 189_something.mdx under exact-path-only collision.
        let dir = temp_dir();
        std::fs::write(dir.join("189_other.mdx"), "existing").unwrap();
        let new_path = dir.join("something.mdx");
        std::fs::write(&new_path, "new").unwrap();

        // Snapshot pretends only the numbered file existed before the run.
        let mut before: HashMap<std::path::PathBuf, SystemTime> = HashMap::new();
        let mtime = std::fs::metadata(dir.join("189_other.mdx"))
            .unwrap()
            .modified()
            .unwrap();
        before.insert(dir.join("189_other.mdx"), mtime);

        let renamed = rename_new_files_to_numbered_mdx(&dir, &before, 189);
        assert_eq!(renamed.len(), 1);
        assert_eq!(renamed[0].0, new_path);
        assert_eq!(renamed[0].1, dir.join("190_something.mdx"));
        assert!(!dir.join("189_something.mdx").exists());
        assert!(dir.join("190_something.mdx").exists());
        assert!(dir.join("189_other.mdx").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_new_files_same_pass_does_not_reuse_allocated_ids() {
        let dir = temp_dir();
        std::fs::write(dir.join("alpha.mdx"), "a").unwrap();
        std::fs::write(dir.join("beta.mdx"), "b").unwrap();
        let before: HashMap<std::path::PathBuf, SystemTime> = HashMap::new();

        let renamed = rename_new_files_to_numbered_mdx(&dir, &before, 10);
        assert_eq!(renamed.len(), 2);
        let targets: HashSet<_> = renamed.iter().map(|(_, t)| t.clone()).collect();
        assert_eq!(targets.len(), 2, "two renames must receive distinct ids");
        assert!(targets.contains(&dir.join("10_alpha.mdx")) || targets.contains(&dir.join("10_beta.mdx")));
        assert!(targets.contains(&dir.join("11_alpha.mdx")) || targets.contains(&dir.join("11_beta.mdx")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
