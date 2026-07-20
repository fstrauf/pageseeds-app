use rig::Embed;
use serde::{Deserialize, Serialize};
/// Skill registry — scans `.github/skills/*/SKILL.md` in a repo root,
/// parses metadata from the content, and returns typed skill descriptors.
///
/// App-level default skills are embedded into the binary at compile time via
/// `include_str!`.  Project-level skills in `{repo}/.github/skills/` always
/// take precedence, allowing per-project overrides.
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default, rig::Embed)]
pub struct Skill {
    /// Directory name (e.g. "reddit-opportunity-search")
    pub name: String,
    /// Relative path from repo root (e.g. ".github/skills/reddit-opportunity-search")
    pub skill_dir: String,
    /// Short description extracted from SKILL.md (first non-empty paragraph after the title)
    pub description: String,
    /// Full raw SKILL.md content
    #[embed]
    pub content: String,
}

// ─── Embedded skills ─────────────────────────────────────────────────────────

const EMBEDDED_SKILL_NAMES: &[&str] = &[
    "cannibalization-strategy",
    "clarity-investigate",
    "content-fix-apply",
    "content-write",
    "ctr-fix-apply",
    "ctr-optimization",
    "ctr-schema-renderer",
    "ctr-template-fix",
    "gsc-investigate",
    "hub-outline",
    "hub-write",
    "indexing-fix",
    "merge-content",
    "reddit-enrich",
    "territory-strategy",
];

fn load_embedded_skill(skill_name: &str) -> Option<Skill> {
    let content = match skill_name {
        "cannibalization-strategy" => {
            include_str!("../../skills/cannibalization-strategy/SKILL.md")
        }
        "clarity-investigate" => include_str!("../../skills/clarity-investigate/SKILL.md"),
        "content-fix-apply" => include_str!("../../skills/content-fix-apply/SKILL.md"),
        "content-write" => include_str!("../../skills/content-write/SKILL.md"),
        "ctr-fix-apply" => include_str!("../../skills/ctr-fix-apply/SKILL.md"),
        "ctr-optimization" => include_str!("../../skills/ctr-optimization/SKILL.md"),
        "ctr-schema-renderer" => include_str!("../../skills/ctr-schema-renderer/SKILL.md"),
        "ctr-template-fix" => include_str!("../../skills/ctr-template-fix/SKILL.md"),
        "gsc-investigate" => include_str!("../../skills/gsc-investigate/SKILL.md"),
        "hub-outline" => include_str!("../../skills/hub-outline/SKILL.md"),
        "hub-write" => include_str!("../../skills/hub-write/SKILL.md"),
        "indexing-fix" => include_str!("../../skills/indexing-fix/SKILL.md"),
        "merge-content" => include_str!("../../skills/merge-content/SKILL.md"),
        "reddit-enrich" => include_str!("../../skills/reddit-enrich/SKILL.md"),
        "territory-strategy" => include_str!("../../skills/territory-strategy/SKILL.md"),
        _ => return None,
    };

    Some(Skill {
        name: skill_name.to_string(),
        skill_dir: format!(".github/skills/{}", skill_name),
        description: extract_description(content, skill_name),
        content: content.to_string(),
    })
}

// ─── Functions ───────────────────────────────────────────────────────────────

/// Scan all skill directories under `{repo_root}/.github/skills/`.
/// Each skill directory must contain a `SKILL.md` to be included.
///
/// Embedded app-level skills are merged in so the skill browser shows
/// every skill that is available (project overrides take precedence).
pub fn scan_skills(repo_root: &Path) -> Vec<Skill> {
    let mut skills: Vec<Skill> = Vec::new();

    // 1. Project-level skills
    let skills_root = repo_root.join(".github").join("skills");
    if skills_root.exists() {
        let project_skills: Vec<Skill> = WalkDir::new(&skills_root)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
            .filter_map(|entry| load_skill_from_dir(entry.path(), repo_root))
            .collect();
        skills.extend(project_skills);
    }

    // 2. Embedded skills not already present at project-level
    for name in EMBEDDED_SKILL_NAMES {
        if !skills.iter().any(|s| s.name == *name) {
            if let Some(skill) = load_embedded_skill(name) {
                skills.push(skill);
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Load a single skill by directory name.
///
/// Resolution order:
/// 1. Project repo: `{repo_root}/.github/skills/{skill_name}/SKILL.md`
/// 2. Embedded app default: compiled into the binary via `include_str!`
pub fn load_skill(repo_root: &Path, skill_name: &str) -> Option<Skill> {
    // 1. Project-level skill first
    let skill_dir = repo_root.join(".github").join("skills").join(skill_name);
    if let Some(skill) = load_skill_from_dir(&skill_dir, repo_root) {
        // TODO(issue #4): version-comparison drift detection is a stopgap until
        // skills carry content hashes/semver.
        if let Some((override_version, embedded_version)) = skill_version_drift(&skill, skill_name) {
            log::warn!(
                "[skills] Project-level skill '{}' overrides the embedded app default but its version ({}) differs from the embedded version ({}). The override may have drifted — delete .github/skills/{}/SKILL.md from the project repo to inherit the app default.",
                skill_name,
                override_version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                embedded_version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                skill_name,
            );
        }
        return Some(skill);
    }

    // 2. Fall back to embedded app-level skill
    load_embedded_skill(skill_name)
}

/// Extract the `<!-- skill-version: N -->` marker from skill content.
///
/// Only the first 5 lines are scanned — the marker is a header convention,
/// not content that may appear anywhere in the body.
pub fn extract_skill_version(content: &str) -> Option<u32> {
    for line in content.lines().take(5) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- skill-version:") {
            if let Some(num) = rest.strip_suffix("-->") {
                if let Ok(version) = num.trim().parse::<u32>() {
                    return Some(version);
                }
            }
        }
    }
    None
}

/// Detect version drift between a project-level override and its embedded
/// counterpart. Returns `Some((override_version, embedded_version))` when an
/// embedded baseline exists and the versions differ (a missing override
/// marker counts as drift). Project-only skills have no baseline and never
/// drift, so this returns `None` for them.
fn skill_version_drift(
    override_skill: &Skill,
    skill_name: &str,
) -> Option<(Option<u32>, Option<u32>)> {
    let embedded = load_embedded_skill(skill_name)?;
    let override_version = extract_skill_version(&override_skill.content);
    let embedded_version = extract_skill_version(&embedded.content);
    (override_version != embedded_version).then_some((override_version, embedded_version))
}

/// Load a skill or return a `StepResult` error message.
///
/// Standardizes the duplicated pattern across all agentic exec modules:
/// ```ignore
/// let skill = match skills::load_skill(repo_root, "name") {
///     Some(s) => s,
///     None => return StepResult { success: false, ... },
/// };
/// ```
pub fn load_skill_or_fail<'a>(
    repo_root: &Path,
    skill_name: &str,
) -> Result<Skill, String> {
    load_skill(repo_root, skill_name)
        .ok_or_else(|| format!("Skill '{}' not found in .github/skills/ or app defaults", skill_name))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn load_skill_from_dir(dir: &Path, repo_root: &Path) -> Option<Skill> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&skill_md).ok()?;
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let description = extract_description(&content, &name);

    let skill_dir = relative_path(dir, repo_root);

    Some(Skill {
        name,
        skill_dir,
        description,
        content,
    })
}

/// Extract a short description from SKILL.md content.
///
/// Strategy (in order):
/// 1. Look for a YAML frontmatter `description:` field.
/// 2. Return the first non-heading, non-empty paragraph (up to 200 chars).
/// 3. Fall back to the H1 title.
/// 4. Use the skill name.
fn extract_description(content: &str, fallback_name: &str) -> String {
    // 1. YAML frontmatter description:
    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let frontmatter = &content[3..end + 3];
            if let Some(line) = frontmatter
                .lines()
                .find(|l| l.trim_start().starts_with("description:"))
            {
                let val = line
                    .splitn(2, ':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !val.is_empty() {
                    return val;
                }
            }
        }
    }

    let mut first_h1: Option<String> = None;
    let mut found_h1 = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Capture H1
        if trimmed.starts_with("# ") && first_h1.is_none() {
            first_h1 = Some(trimmed[2..].trim().to_string());
            found_h1 = true;
            continue;
        }

        // Skip headings, code fences, horizontal rules, empty lines before H1
        if !found_h1 {
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with("---")
            || trimmed.starts_with("===")
        {
            continue;
        }

        // 2. First real paragraph
        let desc = if trimmed.len() > 200 {
            format!("{}…", crate::engine::text::char_prefix(trimmed, 200))
        } else {
            trimmed.to_string()
        };
        return desc;
    }

    // 3. H1 fallback
    if let Some(h1) = first_h1 {
        if !h1.is_empty() {
            return h1;
        }
    }

    // 4. Name fallback
    fallback_name
        .replace('-', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn relative_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Return the absolute path to a skill directory given the repo root and skill name.
pub fn skill_dir_path(repo_root: &Path, skill_name: &str) -> PathBuf {
    repo_root.join(".github").join("skills").join(skill_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("skills_test_{}_{}", std::process::id(), n))
    }

    fn write_project_skill(repo_root: &Path, name: &str, content: &str) {
        let dir = repo_root.join(".github").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_extract_skill_version_marker_in_header() {
        let content = "# Skill\n\n<!-- skill-version: 3 -->\n\nBody text.\n";
        assert_eq!(extract_skill_version(content), Some(3));
    }

    #[test]
    fn test_extract_skill_version_missing_marker() {
        assert_eq!(extract_skill_version("# Skill\n\nNo marker here.\n"), None);
    }

    #[test]
    fn test_extract_skill_version_ignores_marker_beyond_first_lines() {
        let content = "# Skill\n\n## Section\n\n## Another\n\n## More\n\n<!-- skill-version: 9 -->\n";
        assert_eq!(extract_skill_version(content), None);
    }

    #[test]
    fn test_extract_skill_version_malformed_marker() {
        assert_eq!(
            extract_skill_version("# Skill\n\n<!-- skill-version: abc -->\n"),
            None
        );
    }

    #[test]
    fn test_all_embedded_skills_have_version_markers() {
        for name in EMBEDDED_SKILL_NAMES {
            let skill = load_embedded_skill(name)
                .unwrap_or_else(|| panic!("embedded skill '{}' failed to load", name));
            assert_eq!(
                extract_skill_version(&skill.content),
                Some(1),
                "embedded skill '{}' is missing a `<!-- skill-version: 1 -->` marker in its first 5 lines",
                name
            );
        }
    }

    #[test]
    fn test_skill_version_drift_flags_missing_override_version() {
        let embedded = load_embedded_skill("cannibalization-strategy").unwrap();
        let override_skill = Skill {
            content: "# Stale copy without a version marker\n".to_string(),
            ..embedded.clone()
        };
        let drift = skill_version_drift(&override_skill, "cannibalization-strategy");
        assert_eq!(drift, Some((None, Some(1))));
    }

    #[test]
    fn test_skill_version_drift_flags_mismatched_version() {
        let override_skill = Skill {
            content: "# Skill\n\n<!-- skill-version: 99 -->\n".to_string(),
            ..Default::default()
        };
        let drift = skill_version_drift(&override_skill, "cannibalization-strategy");
        assert_eq!(drift, Some((Some(99), Some(1))));
    }

    #[test]
    fn test_skill_version_drift_none_when_versions_match() {
        let embedded = load_embedded_skill("cannibalization-strategy").unwrap();
        let drift = skill_version_drift(&embedded, "cannibalization-strategy");
        assert_eq!(drift, None);
    }

    #[test]
    fn test_skill_version_drift_none_for_project_only_skill() {
        // Project-only skills have no embedded baseline — nothing to drift from.
        let override_skill = Skill {
            content: "# Project-only skill\n".to_string(),
            ..Default::default()
        };
        assert_eq!(skill_version_drift(&override_skill, "indexing-distinctiveness"), None);
    }

    #[test]
    fn test_load_skill_project_override_still_wins() {
        let repo = test_dir();
        let _ = std::fs::remove_dir_all(&repo);
        write_project_skill(
            &repo,
            "cannibalization-strategy",
            "# Stale override\n\nkeep_url based contract\n",
        );

        let skill = load_skill(&repo, "cannibalization-strategy").unwrap();
        // Resolution is unchanged by drift detection: the override still wins.
        assert!(skill.content.contains("Stale override"));

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn test_load_skill_falls_back_to_embedded() {
        let repo = test_dir();
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();

        let skill = load_skill(&repo, "cannibalization-strategy").unwrap();
        assert!(skill.content.contains("keep_id"));

        let _ = std::fs::remove_dir_all(&repo);
    }
}
