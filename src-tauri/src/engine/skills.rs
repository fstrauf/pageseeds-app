/// Skill registry — scans `.github/skills/*/SKILL.md` in a repo root,
/// parses metadata from the content, and returns typed skill descriptors.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Directory name (e.g. "reddit-opportunity-search")
    pub name: String,
    /// Relative path from repo root (e.g. ".github/skills/reddit-opportunity-search")
    pub skill_dir: String,
    /// Short description extracted from SKILL.md (first non-empty paragraph after the title)
    pub description: String,
    /// Full raw SKILL.md content
    pub content: String,
}

// ─── Functions ───────────────────────────────────────────────────────────────

/// Scan all skill directories under `{repo_root}/.github/skills/`.
/// Each skill directory must contain a `SKILL.md` to be included.
pub fn scan_skills(repo_root: &Path) -> Vec<Skill> {
    let skills_root = repo_root.join(".github").join("skills");
    if !skills_root.exists() {
        return Vec::new();
    }

    let mut skills: Vec<Skill> = WalkDir::new(&skills_root)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .filter_map(|entry| load_skill_from_dir(entry.path(), repo_root))
        .collect();

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Load a single skill by directory name.
pub fn load_skill(repo_root: &Path, skill_name: &str) -> Option<Skill> {
    let skill_dir = repo_root.join(".github").join("skills").join(skill_name);
    load_skill_from_dir(&skill_dir, repo_root)
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
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("```")
            || trimmed.starts_with("---") || trimmed.starts_with("===")
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
