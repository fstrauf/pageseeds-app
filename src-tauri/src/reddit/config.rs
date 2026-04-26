/// Parser for `{repo}/.github/automation/reddit_config.md`.
///
/// Extracts structured project configuration used by the Reddit workflow:
/// - product name and mention stance
/// - trigger topics (search queries)
/// - seed subreddits and excluded subreddits
///
/// Section headers follow the same format as the PageSeeds CLI SKILL.md.

use std::path::Path;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MentionStance {
    Required,
    Recommended,
    Optional,
    Omit,
}

impl MentionStance {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "REQUIRED" => MentionStance::Required,
            "RECOMMENDED" => MentionStance::Recommended,
            "OPTIONAL" => MentionStance::Optional,
            "OMIT" => MentionStance::Omit,
            _ => MentionStance::Optional,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MentionStance::Required => "REQUIRED",
            MentionStance::Recommended => "RECOMMENDED",
            MentionStance::Optional => "OPTIONAL",
            MentionStance::Omit => "OMIT",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RedditProjectConfig {
    pub product_name: Option<String>,
    pub mention_stance: MentionStance,
    pub trigger_topics: Vec<String>,
    pub seed_subreddits: Vec<String>,
    pub excluded_subreddits: Vec<String>,
}

// ─── Required config files ────────────────────────────────────────────────────

/// The config files required to run Reddit opportunity search.
/// Uses consolidated project.md (primary) with fallback to legacy files.
#[allow(dead_code)]
pub fn required_config_files(automation_dir: &Path) -> Vec<std::path::PathBuf> {
    vec![
        automation_dir.join("project.md"), // consolidated
        automation_dir.join("reddit_config.md"),
        automation_dir.join("reddit").join("_reply_guardrails.md"),
    ]
}

/// Returns names of any missing required config files.
/// If project.md is missing, checks for legacy files before reporting missing.
pub fn missing_config_files(automation_dir: &Path) -> Vec<String> {
    let mut missing = Vec::new();

    // Check for consolidated project.md first
    let project_md = automation_dir.join("project.md");
    let has_project_md = project_md.exists();

    // If no project.md, check for legacy files as fallback
    if !has_project_md {
        let legacy_files = [
            automation_dir.join("project_summary.md"),
            automation_dir.join("brandvoice.md"),
        ];
        let has_legacy = legacy_files.iter().any(|p| p.exists());

        if !has_legacy {
            // Neither consolidated nor legacy files exist
            missing.push("project.md (or legacy project_summary.md + brandvoice.md)".to_string());
        }
    }

    // Check other required files
    for path in [
        automation_dir.join("reddit_config.md"),
        automation_dir.join("reddit").join("_reply_guardrails.md"),
    ] {
        if !path.exists() {
            missing.push(
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            );
        }
    }

    missing
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Load and parse `reddit_config.md` from the automation directory.
pub fn load_reddit_config(automation_dir: &Path) -> Result<RedditProjectConfig, String> {
    let path = automation_dir.join("reddit_config.md");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read reddit_config.md at {:?}: {}", path, e))?;
    Ok(parse_reddit_config(&content))
}

/// Parse `reddit_config.md` content into a `RedditProjectConfig`.
pub fn parse_reddit_config(content: &str) -> RedditProjectConfig {
    RedditProjectConfig {
        product_name: extract_product_name(content),
        mention_stance: extract_mention_stance(content),
        trigger_topics: extract_list_section(content, "Trigger Topics"),
        seed_subreddits: extract_subreddits(content, "Seed Subreddits"),
        excluded_subreddits: extract_subreddits(content, "Excluded Subreddits"),
    }
}

// ─── Section extractors ───────────────────────────────────────────────────────

/// Extract product name from `## Product Name` section or inline `Product:` line.
/// Base parser - kept simple as agentic parsing is the primary path.
fn extract_product_name(content: &str) -> Option<String> {
    // Try "## Product Name" section first
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Product Name") || trimmed.starts_with("## Product") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") {
                break;
            }
            // Accept bullet `- ProductName` or bare text
            let candidate = trimmed.trim_start_matches("- ").trim();
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }
    }

    // Fallback: look for inline `Product Name: ...` or `Product: ...`
    for line in content.lines() {
        let t = line.trim();
        for prefix in &["Product Name:", "Product:"] {
            if let Some(val) = t.strip_prefix(prefix) {
                let v = val.trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }

    None
}

/// Extract mention stance from `## Mention Stance` section or inline `Mention Stance:` line.
/// Base parser - kept simple as agentic parsing is the primary path.
fn extract_mention_stance(content: &str) -> MentionStance {
    // Try "## Mention Stance" section
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Mention Stance") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") {
                break;
            }
            let candidate = trimmed.trim_start_matches("- ").trim();
            if !candidate.is_empty() {
                return MentionStance::from_str(candidate);
            }
        }
    }

    // Fallback: inline `Mention Stance: REQUIRED`
    for line in content.lines() {
        let t = line.trim();
        for prefix in &["Mention Stance:", "Mention stance:"] {
            if let Some(val) = t.strip_prefix(prefix) {
                let v = val.trim();
                if !v.is_empty() {
                    return MentionStance::from_str(v);
                }
            }
        }
    }

    MentionStance::Optional
}

/// Extract a bullet list from a `## {title}` section.
/// Flexible parsing: accepts "## Title", "## Title:", and case-insensitive matches
fn extract_list_section(content: &str, section_title: &str) -> Vec<String> {
    let mut in_section = false;
    let mut items = Vec::new();
    let section_lower = section_title.to_lowercase();
    for line in content.lines() {
        let trimmed = line.trim();
        // Check for section header (flexible matching)
        let trimmed_lower = trimmed.to_lowercase();
        let is_section = trimmed_lower.starts_with(&format!("## {}", section_lower))
            || trimmed_lower.starts_with(&format!("## {}:", section_lower));
        if is_section {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") {
                break;
            }
            // Accept various list formats: -, *, +, 1., etc.
            if let Some(item) = trimmed.strip_prefix("- ") {
                let item = item.trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
            } else if let Some(item) = trimmed.strip_prefix("* ") {
                let item = item.trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
            } else if let Some(item) = trimmed.strip_prefix("+ ") {
                let item = item.trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
            } else if trimmed.len() > 2 && trimmed.chars().next().unwrap().is_ascii_digit() && trimmed.chars().nth(1).unwrap() == '.' {
                // Numbered list item: "1. item"
                let item = trimmed[2..].trim().to_string();
                if !item.is_empty() {
                    items.push(item);
                }
            }
        }
    }
    items
}

/// Extract subreddit names from a `## {section_title}` section, stripping `r/` prefix.
fn extract_subreddits(content: &str, section_title: &str) -> Vec<String> {
    extract_list_section(content, section_title)
        .into_iter()
        .map(|s| s.trim_start_matches("r/").to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
## Product Name
- Days to Expiry

## Mention Stance
- REQUIRED

## Trigger Topics
- options trading strategies
- DTE tracking for options
- managing expiry risk

## Seed Subreddits
- r/options
- r/thetagang

## Excluded Subreddits
- r/wallstreetbets
"#;

    #[test]
    fn parses_product_name() {
        let cfg = parse_reddit_config(SAMPLE);
        assert_eq!(cfg.product_name, Some("Days to Expiry".to_string()));
    }

    #[test]
    fn parses_mention_stance_required() {
        let cfg = parse_reddit_config(SAMPLE);
        assert_eq!(cfg.mention_stance, MentionStance::Required);
    }

    #[test]
    fn parses_trigger_topics() {
        let cfg = parse_reddit_config(SAMPLE);
        assert_eq!(cfg.trigger_topics.len(), 3);
        assert_eq!(cfg.trigger_topics[0], "options trading strategies");
    }

    #[test]
    fn parses_seed_subreddits_strips_r_prefix() {
        let cfg = parse_reddit_config(SAMPLE);
        assert_eq!(cfg.seed_subreddits, vec!["options", "thetagang"]);
    }

    #[test]
    fn parses_excluded_subreddits() {
        let cfg = parse_reddit_config(SAMPLE);
        assert_eq!(cfg.excluded_subreddits, vec!["wallstreetbets"]);
    }

    #[test]
    fn defaults_to_optional_stance_when_absent() {
        let cfg = parse_reddit_config("## Trigger Topics\n- foo\n");
        assert_eq!(cfg.mention_stance, MentionStance::Optional);
    }

    #[test]
    fn inline_product_name_fallback() {
        let cfg = parse_reddit_config("Product Name: MyApp\n## Trigger Topics\n- foo\n");
        assert_eq!(cfg.product_name, Some("MyApp".to_string()));
    }

    /// Deterministic parser doesn't handle H1 titles — agentic parse is the primary path.
    #[test]
    fn product_name_none_when_only_h1_title() {
        let cfg = parse_reddit_config("# Reddit Config: PageSeeds\n\n## Trigger Topics\n- foo\n");
        assert_eq!(cfg.product_name, None);
    }

    /// Deterministic parser doesn't strip bold markdown — agentic parse is the primary path.
    #[test]
    fn mention_stance_defaults_with_bold_markdown() {
        let cfg = parse_reddit_config(
            "## Mention Stance\n**RECOMMENDED** - Include product name when natural\n\n## Trigger Topics\n- foo\n"
        );
        // Bold markers cause no match → falls back to Optional
        assert_eq!(cfg.mention_stance, MentionStance::Optional);
    }

    /// Plain text (no markdown) works fine with the deterministic parser.
    #[test]
    fn mention_stance_plain_text() {
        let cfg = parse_reddit_config(
            "## Mention Stance\n- REQUIRED\n"
        );
        assert_eq!(cfg.mention_stance, MentionStance::Required);
    }
}
