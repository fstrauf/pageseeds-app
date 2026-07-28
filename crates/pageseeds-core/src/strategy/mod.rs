//! Project content strategy from `project.md`.
//!
//! Typed loader for Search Keywords + Content Clusters sections used by the
//! research pipeline (final selection gate + research-context package).
//!
//! # NOTE: parse contract (v1)
//!
//! Best-effort markdown walk — never fails research. Missing file/sections or
//! malformed headings yield an empty or partial [`ProjectStrategy`].
//!
//! Recognized structure (heading wording is flexible, case-insensitive):
//!
//! - Under a `## Search Keywords` (or similar) section:
//!   - `### Primary Keywords` → `primary_keywords`
//!   - `### Problem Keywords` → `problem_keywords`
//!   - `### Audience Keywords` → `audience_keywords`
//!   - `### …Legacy…` / headings containing `do not expand` → `do_not_expand`
//! - Under `## Content Clusters…` (or `Content Clusters And Priorities`):
//!   - `### Cluster N: Name (STATUS…)` — status token in parentheses:
//!     `ACTIVE` / `MAINTAIN` / `LEGACY` / `PLANNED` (unknown → [`ClusterStatus::Unknown`])
//!   - Bullet keywords under each cluster heading
//!
//! Bullet lines: `- keyword`, `* keyword`, or `+ keyword` (optional leading
//! whitespace). HTML comments and empty lines are ignored.

use std::path::Path;

use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────────────

/// Parsed content strategy from a project's `project.md`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectStrategy {
    pub primary_keywords: Vec<String>,
    pub problem_keywords: Vec<String>,
    pub audience_keywords: Vec<String>,
    /// Phrases that must not seed or expand research (hard gate).
    pub do_not_expand: Vec<String>,
    pub clusters: Vec<StrategyCluster>,
}

impl ProjectStrategy {
    /// True when nothing was parsed (missing file or empty sections).
    pub fn is_empty(&self) -> bool {
        self.primary_keywords.is_empty()
            && self.problem_keywords.is_empty()
            && self.audience_keywords.is_empty()
            && self.do_not_expand.is_empty()
            && self.clusters.is_empty()
    }
}

/// One content cluster with operator-declared lifecycle status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrategyCluster {
    pub name: String,
    pub status: ClusterStatus,
    /// Optional pillar / seed keywords listed under the cluster heading.
    pub keywords: Vec<String>,
}

/// Cluster lifecycle token from parentheses in the cluster heading.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ClusterStatus {
    Active,
    Maintain,
    Legacy,
    Planned,
    Unknown,
}

impl ClusterStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Maintain => "maintain",
            Self::Legacy => "legacy",
            Self::Planned => "planned",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a status token (case-insensitive); unknown tokens → [`Unknown`].
    pub fn parse_token(token: &str) -> Self {
        let t = token.trim().to_ascii_lowercase();
        let t = t.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        match t {
            "active" => Self::Active,
            "maintain" => Self::Maintain,
            "legacy" => Self::Legacy,
            "planned" => Self::Planned,
            _ => Self::Unknown,
        }
    }
}

// ─── Load ────────────────────────────────────────────────────────────────────

/// Load strategy from `{automation_dir}/project.md`.
///
/// Missing or unreadable file → empty strategy (logged). Never panics.
pub fn load_project_strategy(automation_dir: &Path) -> ProjectStrategy {
    let path = automation_dir.join("project.md");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let strategy = parse_project_strategy(&content);
            if strategy.is_empty() {
                log::info!(
                    "[strategy] project.md at {} produced empty strategy (missing Search Keywords / Content Clusters sections?)",
                    path.display()
                );
            } else {
                log::info!(
                    "[strategy] loaded from {}: primary={}, do_not_expand={}, clusters={}",
                    path.display(),
                    strategy.primary_keywords.len(),
                    strategy.do_not_expand.len(),
                    strategy.clusters.len()
                );
            }
            strategy
        }
        Err(e) => {
            log::info!(
                "[strategy] no project.md at {} ({}) — empty strategy",
                path.display(),
                e
            );
            ProjectStrategy::default()
        }
    }
}

/// Convenience: resolve automation dir from a project repo root path.
pub fn load_project_strategy_from_project_path(project_path: &str) -> ProjectStrategy {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    load_project_strategy(paths.automation_dir())
}

/// Canonical loader for call sites that have a DB connection + project id
/// (research-context package, territory analysis). Resolves the project,
/// derives the automation dir, and loads `project.md`. Graceful empty on any
/// failure — strategy must never fail the surrounding workflow.
pub fn load_for_project(conn: &rusqlite::Connection, project_id: &str) -> ProjectStrategy {
    match crate::engine::task_store::get_project(conn, project_id) {
        Ok(project) => {
            let paths = crate::engine::project_paths::ProjectPaths::from_project(&project);
            load_project_strategy(paths.automation_dir())
        }
        Err(e) => {
            log::info!(
                "[strategy] could not resolve project {} — strategy empty: {}",
                project_id,
                e
            );
            ProjectStrategy::default()
        }
    }
}

// ─── Parse ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum TopSection {
    None,
    SearchKeywords,
    ContentClusters,
    Other,
}

#[derive(Clone, Copy)]
enum KeywordBucket {
    None,
    Primary,
    Problem,
    Audience,
    DoNotExpand,
}

/// Parse strategy sections from project.md body. Pure; never panics.
pub fn parse_project_strategy(markdown: &str) -> ProjectStrategy {
    let mut strategy = ProjectStrategy::default();
    let mut top = TopSection::None;
    let mut keyword_bucket = KeywordBucket::None;
    let mut current_cluster: Option<StrategyCluster> = None;

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("<!--") {
            continue;
        }

        if let Some(heading) = strip_heading(line, 2) {
            flush_cluster(&mut strategy, &mut current_cluster);
            keyword_bucket = KeywordBucket::None;
            let h = heading.to_ascii_lowercase();
            top = if h.contains("search keyword") {
                TopSection::SearchKeywords
            } else if h.contains("content cluster") {
                TopSection::ContentClusters
            } else {
                TopSection::Other
            };
            continue;
        }

        if let Some(heading) = strip_heading(line, 3) {
            match top {
                TopSection::SearchKeywords => {
                    flush_cluster(&mut strategy, &mut current_cluster);
                    keyword_bucket = classify_keyword_heading(heading);
                }
                TopSection::ContentClusters => {
                    flush_cluster(&mut strategy, &mut current_cluster);
                    keyword_bucket = KeywordBucket::None;
                    current_cluster = Some(parse_cluster_heading(heading));
                }
                _ => {
                    // Partial recovery: cluster-like ### outside a known ##.
                    if looks_like_cluster_heading(heading) {
                        flush_cluster(&mut strategy, &mut current_cluster);
                        top = TopSection::ContentClusters;
                        current_cluster = Some(parse_cluster_heading(heading));
                        keyword_bucket = KeywordBucket::None;
                    } else {
                        keyword_bucket = KeywordBucket::None;
                    }
                }
            }
            continue;
        }

        if let Some(bullet) = strip_bullet(line) {
            match top {
                TopSection::SearchKeywords => match keyword_bucket {
                    KeywordBucket::Primary => strategy.primary_keywords.push(bullet),
                    KeywordBucket::Problem => strategy.problem_keywords.push(bullet),
                    KeywordBucket::Audience => strategy.audience_keywords.push(bullet),
                    KeywordBucket::DoNotExpand => strategy.do_not_expand.push(bullet),
                    KeywordBucket::None => {}
                },
                TopSection::ContentClusters => {
                    if let Some(ref mut c) = current_cluster {
                        c.keywords.push(bullet);
                    }
                }
                _ => {}
            }
        }
    }

    flush_cluster(&mut strategy, &mut current_cluster);
    strategy
}

fn flush_cluster(strategy: &mut ProjectStrategy, cluster: &mut Option<StrategyCluster>) {
    if let Some(c) = cluster.take() {
        if !c.name.is_empty() {
            strategy.clusters.push(c);
        }
    }
}

fn strip_heading(line: &str, level: usize) -> Option<&str> {
    let (with_space, bare) = match level {
        2 => ("## ", "##"),
        3 => ("### ", "###"),
        _ => return None,
    };
    if let Some(rest) = line.strip_prefix(with_space) {
        return Some(rest.trim());
    }
    // Lenient: ###Heading without space, but not ####.
    if line.starts_with(bare) {
        let rest = &line[bare.len()..];
        if rest.starts_with('#') {
            return None;
        }
        let rest = rest.trim();
        if !rest.is_empty() {
            return Some(rest);
        }
    }
    None
}

fn strip_bullet(line: &str) -> Option<String> {
    let rest = if let Some(r) = line.strip_prefix("- ") {
        r
    } else if let Some(r) = line.strip_prefix("* ") {
        r
    } else if let Some(r) = line.strip_prefix("+ ") {
        r
    } else {
        return None;
    };
    let cleaned = rest
        .split("<!--")
        .next()
        .unwrap_or(rest)
        .trim()
        .trim_start_matches(['*', '_'])
        .trim_end_matches(['*', '_'])
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_string())
}

fn classify_keyword_heading(heading: &str) -> KeywordBucket {
    let h = heading.to_ascii_lowercase();
    if h.contains("do not expand")
        || h.contains("do-not-expand")
        || (h.contains("legacy") && (h.contains("keyword") || h.contains("service")))
        || h.contains("never expand")
    {
        return KeywordBucket::DoNotExpand;
    }
    if h.contains("primary") {
        return KeywordBucket::Primary;
    }
    if h.contains("problem") {
        return KeywordBucket::Problem;
    }
    if h.contains("audience") {
        return KeywordBucket::Audience;
    }
    KeywordBucket::None
}

fn looks_like_cluster_heading(heading: &str) -> bool {
    let h = heading.to_ascii_lowercase();
    h.contains("cluster") && (heading.contains('(') || h.contains("active") || h.contains("legacy"))
}

/// Parse `Cluster 1: SEO Fundamentals (ACTIVE)` → name + status.
fn parse_cluster_heading(heading: &str) -> StrategyCluster {
    let mut name = heading.trim().to_string();
    let mut status = ClusterStatus::Unknown;

    if let Some(open) = heading.rfind('(') {
        if let Some(close) = heading[open + 1..].find(')') {
            let inside = &heading[open + 1..open + 1 + close];
            for token in inside.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '—')
            {
                let parsed = ClusterStatus::parse_token(token);
                if parsed != ClusterStatus::Unknown {
                    status = parsed;
                    break;
                }
            }
            name = heading[..open].trim().to_string();
        }
    }

    if let Some(rest) = strip_cluster_prefix(&name) {
        name = rest;
    }

    StrategyCluster {
        name: name.trim().trim_end_matches(':').trim().to_string(),
        status,
        keywords: Vec::new(),
    }
}

fn strip_cluster_prefix(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if !lower.starts_with("cluster") {
        return None;
    }
    let after = name["cluster".len()..].trim_start();
    let after = after
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start();
    let after = after
        .strip_prefix(':')
        .or_else(|| after.strip_prefix('-'))
        .or_else(|| after.strip_prefix('–'))
        .unwrap_or(after)
        .trim();
    if after.is_empty() {
        None
    } else {
        Some(after.to_string())
    }
}

// ─── Matching helpers (used by final selection) ──────────────────────────────

/// Match policy for strategy gates (v1): **case-insensitive substring**.
///
/// A candidate keyword matches a strategy phrase when the lowercased candidate
/// contains the lowercased phrase as a contiguous substring (after trim).
/// Empty phrases never match. Prefer this over word-boundary matching so
/// multi-word `do_not_expand` entries like `"custom web design"` catch
/// `"best custom web design agency"` without tokenization edge cases.
pub fn keyword_matches_phrase(keyword: &str, phrase: &str) -> bool {
    let phrase = phrase.trim();
    if phrase.is_empty() {
        return false;
    }
    let kw = keyword.to_ascii_lowercase();
    let ph = phrase.to_ascii_lowercase();
    kw.contains(&ph)
}

/// True when the keyword is hard-blocked by `do_not_expand`.
pub fn matches_do_not_expand(keyword: &str, strategy: &ProjectStrategy) -> bool {
    strategy
        .do_not_expand
        .iter()
        .any(|p| keyword_matches_phrase(keyword, p))
}

/// True when the keyword clearly maps to a LEGACY cluster (hard drop).
///
/// Hard mapping only: listed cluster keywords (substring phrase policy) and/or
/// multi-token name overlap (`token_overlap_match`, ≥2 significant tokens).
/// Single-token / short name substring matching is **not** used here — a LEGACY
/// cluster named `"Services"` must not ban every keyword containing `"services"`.
pub fn matches_legacy_cluster(keyword: &str, strategy: &ProjectStrategy) -> bool {
    strategy
        .clusters
        .iter()
        .filter(|c| c.status == ClusterStatus::Legacy)
        .any(|c| maps_to_cluster_hard(keyword, c))
}

/// Hard-block policy for shortlist themes/seeds used as research expansion fuel.
///
/// Same gates as [`apply_strategy_filter`]: `do_not_expand` + LEGACY cluster map.
/// Empty strategy → `false` (no-op; full shortlist, never falsely empty).
///
/// Shared by produce (`sync_theme_to_shortlist`) and consume (`read_pending_shortlist`)
/// so the two sides cannot drift from final selection policy.
pub fn strategy_blocks_expansion(theme_or_seed: &str, strategy: &ProjectStrategy) -> bool {
    if strategy.is_empty() {
        return false;
    }
    matches_do_not_expand(theme_or_seed, strategy)
        || matches_legacy_cluster(theme_or_seed, strategy)
}

/// True when the keyword maps to a MAINTAIN cluster (deprioritize, not drop).
pub fn matches_maintain_cluster(keyword: &str, strategy: &ProjectStrategy) -> bool {
    strategy
        .clusters
        .iter()
        .filter(|c| c.status == ClusterStatus::Maintain)
        .any(|c| maps_to_cluster_soft(keyword, c))
}

/// True when the keyword aligns with ACTIVE cluster keywords or primary keywords.
pub fn matches_active_or_primary(keyword: &str, strategy: &ProjectStrategy) -> bool {
    if strategy
        .primary_keywords
        .iter()
        .any(|p| keyword_matches_phrase(keyword, p) || keyword_matches_phrase(p, keyword))
    {
        return true;
    }
    strategy
        .clusters
        .iter()
        .filter(|c| c.status == ClusterStatus::Active)
        .any(|c| maps_to_cluster_soft(keyword, c))
}

/// Best-matching cluster for a keyword, for annotation (shortlist rows,
/// diagnostics). Returns `None` when nothing matches or the strategy is empty.
///
/// Match policy: soft map (keyword bullets, name substring ≥4 chars,
/// multi-token name overlap) — the same policy as the rank hints in
/// [`apply_strategy_filter`]. On multiple matches, status precedence is
/// Active > Maintain > Planned > Legacy > Unknown, so a LEGACY cluster never
/// shadows an ACTIVE match for the same theme.
pub fn match_cluster<'a>(
    strategy: &'a ProjectStrategy,
    keyword: &str,
) -> Option<(&'a str, ClusterStatus)> {
    const PRECEDENCE: [ClusterStatus; 5] = [
        ClusterStatus::Active,
        ClusterStatus::Maintain,
        ClusterStatus::Planned,
        ClusterStatus::Legacy,
        ClusterStatus::Unknown,
    ];
    for status in PRECEDENCE {
        if let Some(c) = strategy
            .clusters
            .iter()
            .find(|c| c.status == status && maps_to_cluster_soft(keyword, c))
        {
            return Some((c.name.as_str(), c.status));
        }
    }
    None
}

/// Hard cluster map (LEGACY / hard reject): explicit keyword bullets + multi-token
/// name overlap only. No single-token name substring.
fn maps_to_cluster_hard(keyword: &str, cluster: &StrategyCluster) -> bool {
    for k in &cluster.keywords {
        if keyword_matches_phrase(keyword, k) {
            return true;
        }
    }
    token_overlap_match(keyword, cluster.name.trim())
}

/// Soft cluster map (MAINTAIN/ACTIVE rank hints): keyword bullets, name substring
/// when the name is ≥4 chars, and multi-token name overlap.
fn maps_to_cluster_soft(keyword: &str, cluster: &StrategyCluster) -> bool {
    for k in &cluster.keywords {
        if keyword_matches_phrase(keyword, k) {
            return true;
        }
    }
    let name = cluster.name.trim();
    if name.len() >= 4 && keyword_matches_phrase(keyword, name) {
        return true;
    }
    token_overlap_match(keyword, name)
}

fn token_overlap_match(keyword: &str, phrase: &str) -> bool {
    let stop = [
        "and", "the", "for", "with", "from", "into", "a", "an", "of", "to", "in", "on",
    ];
    let tokens: Vec<String> = phrase
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| t.len() >= 3 && !stop.contains(&t.as_str()))
        .collect();
    if tokens.len() < 2 {
        return false;
    }
    let kw = keyword.to_ascii_lowercase();
    let hits = tokens.iter().filter(|t| kw.contains(t.as_str())).count();
    hits >= 2
}

// ─── Apply filter (pure) ─────────────────────────────────────────────────────

/// Rank hint after strategy hard-drops: lower sorts first among volume/KD ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrategyRank {
    /// ACTIVE / primary alignment — mild tiebreak boost.
    ActiveBoost = 0,
    /// No strategy signal.
    Neutral = 1,
    /// MAINTAIN cluster — still allowed, sorts after Active/Neutral.
    Maintain = 2,
}

/// Result of applying hard strategy gates to a candidate list.
#[derive(Debug, Clone)]
pub struct StrategyFilterOutcome<T> {
    pub kept: Vec<(T, StrategyRank)>,
    /// Count of candidates hard-dropped (do_not_expand + LEGACY).
    pub strategy_rejected_count: usize,
}

/// Apply strategy hard gates and rank hints to candidates.
///
/// - `do_not_expand` phrase match → hard drop
/// - LEGACY cluster map → hard drop
/// - MAINTAIN map → keep with [`StrategyRank::Maintain`]
/// - ACTIVE / primary alignment → [`StrategyRank::ActiveBoost`]
/// - Empty strategy → all kept as [`StrategyRank::Neutral`] (no-op)
///
/// `keyword_of` extracts the phrase used for matching.
pub fn apply_strategy_filter<T, F>(
    candidates: Vec<T>,
    strategy: &ProjectStrategy,
    keyword_of: F,
) -> StrategyFilterOutcome<T>
where
    F: Fn(&T) -> &str,
{
    if strategy.is_empty() {
        let kept = candidates
            .into_iter()
            .map(|c| (c, StrategyRank::Neutral))
            .collect();
        return StrategyFilterOutcome {
            kept,
            strategy_rejected_count: 0,
        };
    }

    let mut kept = Vec::new();
    let mut strategy_rejected_count = 0usize;

    for c in candidates {
        let kw = keyword_of(&c);
        if matches_do_not_expand(kw, strategy) || matches_legacy_cluster(kw, strategy) {
            strategy_rejected_count += 1;
            continue;
        }
        let rank = if matches_maintain_cluster(kw, strategy) {
            StrategyRank::Maintain
        } else if matches_active_or_primary(kw, strategy) {
            StrategyRank::ActiveBoost
        } else {
            StrategyRank::Neutral
        };
        kept.push((c, rank));
    }

    StrategyFilterOutcome {
        kept,
        strategy_rejected_count,
    }
}

// ─── Operator-facing summary (research-context) ──────────────────────────────

/// Compact strategy view for `research-context` JSON (no full markdown dump).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentStrategySummary {
    pub primary_keywords: Vec<String>,
    pub problem_keywords: Vec<String>,
    pub audience_keywords: Vec<String>,
    pub do_not_expand: Vec<String>,
    pub active_clusters: Vec<ClusterSummary>,
    pub maintain_clusters: Vec<ClusterSummary>,
    pub legacy_clusters: Vec<ClusterSummary>,
    pub planned_clusters: Vec<ClusterSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterSummary {
    pub name: String,
    pub keywords: Vec<String>,
}

impl From<&ProjectStrategy> for ContentStrategySummary {
    fn from(s: &ProjectStrategy) -> Self {
        let mut active = Vec::new();
        let mut maintain = Vec::new();
        let mut legacy = Vec::new();
        let mut planned = Vec::new();
        for c in &s.clusters {
            let summary = ClusterSummary {
                name: c.name.clone(),
                keywords: c.keywords.clone(),
            };
            match c.status {
                ClusterStatus::Active => active.push(summary),
                ClusterStatus::Maintain => maintain.push(summary),
                ClusterStatus::Legacy => legacy.push(summary),
                ClusterStatus::Planned => planned.push(summary),
                ClusterStatus::Unknown => {}
            }
        }
        Self {
            primary_keywords: s.primary_keywords.clone(),
            problem_keywords: s.problem_keywords.clone(),
            audience_keywords: s.audience_keywords.clone(),
            do_not_expand: s.do_not_expand.clone(),
            active_clusters: active,
            maintain_clusters: maintain,
            legacy_clusters: legacy,
            planned_clusters: planned,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = r#"# Example Project

## Search Keywords

### Primary Keywords
- seo tools
- keyword research

### Problem Keywords
- thin content

### Audience Keywords
- content marketers

### Legacy Service Keywords (do not expand)
- custom web design
- wordpress agency

## Content Clusters And Priorities

### Cluster 1: SEO Fundamentals (ACTIVE)
- on-page seo
- technical seo

### Cluster 2: Alternatives (MAINTAIN)
- competitor alternatives

### Cluster 3: Services (LEGACY)
- web design packages

### Cluster 4: New Pillar (PLANNED)
- ai content ops
"#;

    #[test]
    fn parses_fixture_keywords_and_clusters() {
        let s = parse_project_strategy(FIXTURE);
        assert_eq!(s.primary_keywords, vec!["seo tools", "keyword research"]);
        assert_eq!(s.problem_keywords, vec!["thin content"]);
        assert_eq!(s.audience_keywords, vec!["content marketers"]);
        assert_eq!(
            s.do_not_expand,
            vec!["custom web design", "wordpress agency"]
        );
        assert_eq!(s.clusters.len(), 4);

        assert_eq!(s.clusters[0].name, "SEO Fundamentals");
        assert_eq!(s.clusters[0].status, ClusterStatus::Active);
        assert_eq!(
            s.clusters[0].keywords,
            vec!["on-page seo", "technical seo"]
        );

        assert_eq!(s.clusters[1].name, "Alternatives");
        assert_eq!(s.clusters[1].status, ClusterStatus::Maintain);
        assert_eq!(s.clusters[1].keywords, vec!["competitor alternatives"]);

        assert_eq!(s.clusters[2].name, "Services");
        assert_eq!(s.clusters[2].status, ClusterStatus::Legacy);
        assert_eq!(s.clusters[2].keywords, vec!["web design packages"]);

        assert_eq!(s.clusters[3].name, "New Pillar");
        assert_eq!(s.clusters[3].status, ClusterStatus::Planned);
        assert_eq!(s.clusters[3].keywords, vec!["ai content ops"]);
    }

    #[test]
    fn missing_sections_yield_empty_strategy() {
        let s = parse_project_strategy("# Just a title\n\nSome prose.\n");
        assert!(s.is_empty());
    }

    #[test]
    fn empty_and_malformed_do_not_panic() {
        let _ = parse_project_strategy("");
        let _ = parse_project_strategy("### Cluster 1: Broken (NOTASTATUS)\n- still a keyword\n");
        let _ = parse_project_strategy("## Search Keywords\n### Primary Keywords\nnot a bullet\n");
        let s = parse_project_strategy(
            "## Search Keywords\n### Primary Keywords\n- only primary\n",
        );
        assert_eq!(s.primary_keywords, vec!["only primary"]);
        assert!(s.do_not_expand.is_empty());
        assert!(s.clusters.is_empty());
    }

    #[test]
    fn tolerates_heading_wording_variation() {
        let md = r#"
## Search keywords

### Primary
- alpha

### Legacy keywords — do not expand
- beta banned

## Content Clusters & Status

### Cluster 1: Foo Bar (ACTIVE — expand freely)
- foo bar seed
"#;
        let s = parse_project_strategy(md);
        assert_eq!(s.primary_keywords, vec!["alpha"]);
        assert_eq!(s.do_not_expand, vec!["beta banned"]);
        assert_eq!(s.clusters.len(), 1);
        assert_eq!(s.clusters[0].status, ClusterStatus::Active);
        assert_eq!(s.clusters[0].name, "Foo Bar");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile_dir("missing");
        let s = load_project_strategy(&dir);
        assert!(s.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_reads_project_md_from_automation_dir() {
        let dir = tempfile_dir("present");
        let mut f = std::fs::File::create(dir.join("project.md")).unwrap();
        f.write_all(FIXTURE.as_bytes()).unwrap();
        let s = load_project_strategy(&dir);
        assert!(!s.is_empty());
        assert_eq!(s.primary_keywords.len(), 2);
        assert_eq!(s.clusters.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn match_policy_substring_case_insensitive() {
        assert!(keyword_matches_phrase(
            "Best Custom Web Design Agency",
            "custom web design"
        ));
        assert!(!keyword_matches_phrase("seo tools", "wordpress agency"));
        assert!(!keyword_matches_phrase("anything", "  "));
    }

    #[test]
    fn apply_strategy_filter_hard_drops_and_ranks() {
        let strategy = parse_project_strategy(FIXTURE);
        let candidates = vec![
            "seo tools guide".to_string(),
            "custom web design packages".to_string(), // do_not_expand
            "web design packages pricing".to_string(), // LEGACY
            "competitor alternatives list".to_string(), // MAINTAIN
            "random unrelated keyword".to_string(),
            "on-page seo checklist".to_string(), // ACTIVE
        ];
        let outcome = apply_strategy_filter(candidates, &strategy, |s| s.as_str());
        assert_eq!(outcome.strategy_rejected_count, 2);
        let kept: Vec<_> = outcome
            .kept
            .iter()
            .map(|(k, r)| (k.as_str(), *r))
            .collect();
        assert!(kept
            .iter()
            .any(|(k, r)| *k == "seo tools guide" && *r == StrategyRank::ActiveBoost));
        assert!(kept
            .iter()
            .any(|(k, r)| *k == "on-page seo checklist" && *r == StrategyRank::ActiveBoost));
        assert!(kept.iter().any(|(k, r)| {
            *k == "competitor alternatives list" && *r == StrategyRank::Maintain
        }));
        assert!(kept
            .iter()
            .any(|(k, r)| *k == "random unrelated keyword" && *r == StrategyRank::Neutral));
        assert!(!kept.iter().any(|(k, _)| k.contains("custom web design")));
        assert!(!kept.iter().any(|(k, _)| k.contains("web design packages")));
    }

    #[test]
    fn legacy_hard_drop_ignores_single_token_cluster_name() {
        // LEGACY cluster "Services" must not ban every keyword containing "services".
        // Only explicit bullet phrases (and multi-token name overlap) hard-drop.
        let strategy = ProjectStrategy {
            clusters: vec![StrategyCluster {
                name: "Services".to_string(),
                status: ClusterStatus::Legacy,
                keywords: vec!["web design packages".to_string()],
            }],
            ..Default::default()
        };

        assert!(
            !matches_legacy_cluster("cloud services pricing", &strategy),
            "single-token name substring must not hard-drop"
        );
        assert!(
            !matches_legacy_cluster("managed services checklist", &strategy),
            "unrelated *services* keyword must not hard-drop"
        );
        assert!(
            matches_legacy_cluster("web design packages pricing", &strategy),
            "explicit bullet phrase must still hard-drop"
        );
        assert!(
            matches_legacy_cluster("best web design packages", &strategy),
            "substring of bullet phrase must still hard-drop"
        );

        let outcome = apply_strategy_filter(
            vec![
                "cloud services pricing".to_string(),
                "managed services checklist".to_string(),
                "web design packages guide".to_string(),
            ],
            &strategy,
            |s| s.as_str(),
        );
        assert_eq!(outcome.strategy_rejected_count, 1);
        let kept: Vec<&str> = outcome.kept.iter().map(|(k, _)| k.as_str()).collect();
        assert!(kept.contains(&"cloud services pricing"));
        assert!(kept.contains(&"managed services checklist"));
        assert!(!kept.iter().any(|k| k.contains("web design packages")));
    }

    #[test]
    fn legacy_hard_drop_uses_multi_token_name_overlap() {
        let strategy = ProjectStrategy {
            clusters: vec![StrategyCluster {
                name: "Custom Web Design".to_string(),
                status: ClusterStatus::Legacy,
                keywords: vec![],
            }],
            ..Default::default()
        };
        // ≥2 significant tokens from the multi-word name → hard drop.
        assert!(matches_legacy_cluster(
            "custom web design agency near me",
            &strategy
        ));
        // Only one token in common → keep.
        assert!(!matches_legacy_cluster("custom furniture pricing", &strategy));
    }

    #[test]
    fn empty_strategy_filter_is_noop() {
        let strategy = ProjectStrategy::default();
        let candidates = vec!["a".to_string(), "b".to_string()];
        let outcome = apply_strategy_filter(candidates, &strategy, |s| s.as_str());
        assert_eq!(outcome.strategy_rejected_count, 0);
        assert_eq!(outcome.kept.len(), 2);
        assert!(outcome
            .kept
            .iter()
            .all(|(_, r)| *r == StrategyRank::Neutral));
    }

    #[test]
    fn strategy_blocks_expansion_matches_final_selection_hard_gates() {
        let strategy = parse_project_strategy(FIXTURE);
        // do_not_expand
        assert!(strategy_blocks_expansion("custom web design agency", &strategy));
        // LEGACY cluster bullet
        assert!(strategy_blocks_expansion("web design packages pricing", &strategy));
        // ACTIVE / primary / unmatched → not blocked
        assert!(!strategy_blocks_expansion("technical seo checklist", &strategy));
        assert!(!strategy_blocks_expansion("seo tools guide", &strategy));
        assert!(!strategy_blocks_expansion("random unrelated", &strategy));
        // MAINTAIN is deprioritize, not hard block
        assert!(!strategy_blocks_expansion(
            "competitor alternatives list",
            &strategy
        ));
    }

    #[test]
    fn strategy_blocks_expansion_empty_strategy_is_noop() {
        let strategy = ProjectStrategy::default();
        assert!(!strategy_blocks_expansion("custom web design", &strategy));
        assert!(!strategy_blocks_expansion("web design packages", &strategy));
        assert!(!strategy_blocks_expansion("anything", &strategy));
    }

    #[test]
    fn content_strategy_summary_groups_clusters() {
        let s = parse_project_strategy(FIXTURE);
        let summary = ContentStrategySummary::from(&s);
        assert_eq!(summary.active_clusters.len(), 1);
        assert_eq!(summary.maintain_clusters.len(), 1);
        assert_eq!(summary.legacy_clusters.len(), 1);
        assert_eq!(summary.planned_clusters.len(), 1);
        assert_eq!(summary.do_not_expand.len(), 2);
    }

    #[test]
    fn match_cluster_annotates_name_and_status() {
        let s = parse_project_strategy(FIXTURE);
        assert_eq!(
            match_cluster(&s, "on-page seo checklist"),
            Some(("SEO Fundamentals", ClusterStatus::Active))
        );
        assert_eq!(
            match_cluster(&s, "competitor alternatives list"),
            Some(("Alternatives", ClusterStatus::Maintain))
        );
        assert_eq!(
            match_cluster(&s, "web design packages pricing"),
            Some(("Services", ClusterStatus::Legacy))
        );
        assert_eq!(
            match_cluster(&s, "ai content ops guide"),
            Some(("New Pillar", ClusterStatus::Planned))
        );
        assert_eq!(match_cluster(&s, "totally unrelated topic"), None);
        assert_eq!(match_cluster(&ProjectStrategy::default(), "anything"), None);
    }

    #[test]
    fn match_cluster_prefers_active_over_legacy_on_overlap() {
        let s = ProjectStrategy {
            clusters: vec![
                StrategyCluster {
                    name: "Old Services".to_string(),
                    status: ClusterStatus::Legacy,
                    keywords: vec!["web design packages".to_string()],
                },
                StrategyCluster {
                    name: "Design Growth".to_string(),
                    status: ClusterStatus::Active,
                    keywords: vec!["web design packages".to_string()],
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            match_cluster(&s, "web design packages pricing"),
            Some(("Design Growth", ClusterStatus::Active)),
            "ACTIVE match must win over LEGACY for the same keyword"
        );
    }

    fn tempfile_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-strategy-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
