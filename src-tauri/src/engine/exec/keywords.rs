/// Keyword research execution module.
///
/// Native Rust pipeline:
///   1. `get_keyword_ideas` per theme → keywords WITH volume
///   2. Dedupe against articles.json
///   3. `get_keyword_difficulty` per top-N keyword → KD scores
///   4. Merge into the standard output schema so KeywordPicker shows both volume and KD.

use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

fn parse_themes_from_agent_artifact(task: &Task) -> Vec<String> {
    let content = task
        .artifacts
        .iter()
        .rev()
        .find(|a| a.key == "research_theme_selection_agent")
        .and_then(|a| a.content.as_deref());

    let Some(raw) = content else {
        return vec![];
    };

    // Agent output is often wrapped with prose/tool logs + fenced JSON.
    // Normalize first, then parse direct JSON as a fallback.
    let normalized = crate::engine::normalizer::normalize_agent_output(raw);
    let parsed = normalized
        .json_artifact
        .or_else(|| serde_json::from_str::<serde_json::Value>(raw).ok());

    if let Some(json) = parsed {
        let themes = themes_from_json(&json);
        if !themes.is_empty() {
            return themes;
        }
    }

    // Last-resort fallback: parse bullet/numbered list text from the agent output.
    raw.lines()
        .filter_map(|line| {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("- ") {
                return clean_theme_str(rest);
            }
            if let Some(rest) = t.strip_prefix("* ") {
                return clean_theme_str(rest);
            }
            if let Some(dot) = t.find(". ") {
                if t[..dot].chars().all(|c| c.is_ascii_digit()) {
                    return clean_theme_str(&t[dot + 2..]);
                }
            }
            None
        })
        .collect()
}

fn themes_from_json(v: &serde_json::Value) -> Vec<String> {
    let from_array = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .filter_map(clean_theme_str)
            .collect::<Vec<String>>()
    };

    // Accept either object-based or array-based contracts.
    if let Some(arr) = v.as_array() {
        return from_array(arr);
    }

    for key in ["themes", "selected_themes", "keyword_themes"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            return from_array(arr);
        }
    }

    vec![]
}

fn estimate_volume(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Ahrefs free tools often return enum-like labels instead of numeric ranges.
    match s {
        "MoreThanTenThousand" => return Some(10000),
        "MoreThanOneThousand" => return Some(1000),
        "MoreThanOneHundred" => return Some(100),
        "LessThanOneHundred" => return Some(50),
        _ => {}
    }

    let mut raw_chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == ',' {
            current.push(ch);
        } else if !current.is_empty() {
            raw_chunks.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        raw_chunks.push(current);
    }

    let nums: Vec<i64> = raw_chunks
        .into_iter()
        .map(|c| c.replace(',', ""))
        .filter_map(|p| p.parse::<i64>().ok())
        .collect();

    match nums.as_slice() {
        [] => None,
        [single] => Some(*single),
        [a, b, ..] => Some((a + b) / 2),
    }
}

fn best_serp_metric(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.flatten().fold(None, |acc, v| match acc {
        Some(current) if current >= v => Some(current),
        _ => Some(v),
    })
}

pub(crate) fn exec_keyword_research_native(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;

    let paths = ProjectPaths::from_path(project_path);

    // ── Resolve CAPSOLVER_API_KEY ─────────────────────────────────────────────
    let env = EnvResolver::new(project_path).build_env(HashMap::new());
    let capsolver_key = match env.get("CAPSOLVER_API_KEY").map(|s| s.as_str()) {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: "CAPSOLVER_API_KEY not set. Add it in Settings → Secrets.".to_string(),
                output: None,
            };
        }
    };

    // ── Parse themes from task description ───────────────────────────────────
    let raw_desc = task.description.as_deref().unwrap_or("");
    let desc_themes = parse_desc_themes(raw_desc);

    let agent_themes = parse_themes_from_agent_artifact(task);

    let themes = if !desc_themes.is_empty() {
        desc_themes
    } else if !agent_themes.is_empty() {
        agent_themes
    } else {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No keyword themes available. Provide themes in task description or run agentic theme selection first. \
                 Expected artifact key: research_theme_selection_agent. Workspace: {}.",
                paths.automation_dir.display()
            ),
            output: None,
        };
    };

    log::info!("[keyword_research_native] {} themes: {:?}", themes.len(), themes);

    // ── Pre-flight: articles.json must exist ──────────────────────────────────
    let articles_json_path = paths.automation_dir.join("articles.json");
    if !articles_json_path.exists() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "Workspace not initialised: articles.json not found at {}. \
                 Run 'Init Workspace' from Project Settings first.",
                articles_json_path.display()
            ),
            output: None,
        };
    }

    // Load existing keywords from articles.json so we can skip already-covered ones.
    // articles.json format: {"nextArticleId": N, "articles": [...]}
    let existing_keywords: HashSet<String> = std::fs::read_to_string(&articles_json_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            // Support both {"articles": [...]} wrapper and bare [...] array.
            let arr = v["articles"].as_array().or_else(|| v.as_array());
            arr.map(|items| {
                items.iter()
                    .filter_map(|a| a["target_keyword"].as_str())
                    .map(|k| k.to_lowercase())
                    .collect()
            })
        })
        .unwrap_or_default();

    log::info!("[keyword_research_native] {} existing keywords to filter against", existing_keywords.len());

    // ── Bridge to tokio async runtime ─────────────────────────────────────────
    let handle = tokio::runtime::Handle::current();

    // Step 1 — Generate keyword ideas (includes volume) for each theme.
    let mut volume_map: HashMap<String, i64> = HashMap::new();
    let mut all_new_keywords: Vec<String> = vec![];
    let mut seen: HashSet<String> = HashSet::new();

    for theme in &themes {
        log::info!("[keyword_research_native] fetching ideas for theme '{}'", theme);
        match handle.block_on(crate::seo::keywords::get_keyword_ideas(
            &capsolver_key, theme, "us", "Google",
        )) {
            Ok(result) => {
                let all_ideas = result.ideas.iter().chain(result.question_ideas.iter());
                for idea in all_ideas {
                    let kw_lower = idea.keyword.to_lowercase();
                    if existing_keywords.contains(&kw_lower) {
                        continue; // already covered
                    }
                    if seen.contains(&kw_lower) {
                        continue;
                    }
                    seen.insert(kw_lower.clone());
                    if let Some(vol) = &idea.volume {
                        if let Some(n) = estimate_volume(vol) {
                            volume_map.insert(idea.keyword.clone(), n);
                        }
                    }
                    all_new_keywords.push(idea.keyword.clone());
                }
                log::info!("[keyword_research_native] theme '{}' → {} new keywords so far", theme, all_new_keywords.len());
            }
            Err(e) => {
                log::warn!("[keyword_research_native] keyword ideas failed for '{}': {}", theme, e);
            }
        }
    }

    if all_new_keywords.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No new keyword ideas found for themes: {}. All suggestions may already be covered.",
                themes.join(", ")
            ),
            output: None,
        };
    }

    // Step 2 — Analyse difficulty, over-sampling to fill 10 with-data results.
    // Ahrefs free tier often returns no data for long-tail keywords. Rather than
    // analyzing exactly 10 and accepting 3 hits, iterate through candidates until
    // we have 10 with-data results or exhaust a budget of 30 API calls.
    let target_with_data = 10usize;
    let max_api_calls = 30usize;
    log::info!(
        "[keyword_research_native] analyzing difficulty (target {} with data, max {} calls, {} candidates)",
        target_with_data, max_api_calls, all_new_keywords.len()
    );

    let mut with_data_results: Vec<serde_json::Value> = vec![];
    let mut no_data_results: Vec<serde_json::Value> = vec![];
    let mut api_calls = 0usize;
    let mut analyzed_count = 0usize;

    for kw in &all_new_keywords {
        if with_data_results.len() >= target_with_data || api_calls >= max_api_calls {
            break;
        }
        api_calls += 1;
        analyzed_count += 1;

        match handle.block_on(crate::seo::keywords::get_keyword_difficulty(
            &capsolver_key, kw, "us",
        )) {
            Ok(kd) => {
                let has_data = kd.difficulty.is_some() && !kd.last_update.is_empty();
                let vol = volume_map.get(kw).copied();
                let top_traffic = best_serp_metric(kd.serp.iter().map(|s| s.traffic));
                let top_volume = best_serp_metric(kd.serp.iter().map(|s| s.top_volume));
                let entry = serde_json::json!({
                    "keyword": kw,
                    "difficulty": kd.difficulty,
                    "volume": vol,
                    "traffic": top_traffic,
                    "topVolume": top_volume,
                    "shortage": kd.shortage,
                    "has_data": has_data,
                    "serp_count": kd.serp.len(),
                    "top_result": kd.serp.first().map(|s| s.url.as_str()).unwrap_or(""),
                    "last_update": kd.last_update,
                });
                log::info!(
                    "[keyword_research_native] '{}' kd={:?} vol={:?} top_traffic={:?} has_data={}",
                    kw, kd.difficulty, vol, top_traffic, has_data,
                );
                if has_data {
                    with_data_results.push(entry);
                } else {
                    no_data_results.push(entry);
                }
            }
            Err(e) => {
                log::warn!("[keyword_research_native] difficulty failed for '{}': {}", kw, e);
                let vol = volume_map.get(kw).copied();
                no_data_results.push(serde_json::json!({
                    "keyword": kw,
                    "difficulty": serde_json::Value::Null,
                    "volume": vol,
                    "has_data": false,
                    "serp_count": 0,
                    "top_result": "",
                    "last_update": "",
                }));
            }
        }
    }

    // Present with-data results first, then pad with no-data up to 10 total.
    let mut difficulty_results = with_data_results;
    let remaining_slots = target_with_data.saturating_sub(difficulty_results.len());
    difficulty_results.extend(no_data_results.into_iter().take(remaining_slots));

    log::info!(
        "[keyword_research_native] {} with data, {} total shown (checked {} keywords)",
        difficulty_results.iter().filter(|r| r["has_data"] == true).count(),
        difficulty_results.len(),
        analyzed_count,
    );

    let total_candidates = all_new_keywords.len();
    let skipped_keywords: Vec<String> = all_new_keywords.iter().skip(analyzed_count).cloned().collect();
    let output = serde_json::json!({
        "themes": themes,
        "total_candidates": total_candidates,
        "new_keywords": all_new_keywords,
        "filtered_out": 0,
        "difficulty": {
            "total": analyzed_count,
            "successful": difficulty_results.iter().filter(|r| r["has_data"] == true).count(),
            "failed": difficulty_results.iter().filter(|r| r["has_data"] != true).count(),
            "results": difficulty_results,
        },
        "difficulty_skipped_keywords": skipped_keywords,
    });

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Keyword research complete ({} themes, {} candidates, {} analyzed)",
            themes.len(), total_candidates, analyzed_count
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
    }
}

// ─── Theme string cleaning ──────────────────────────────────────────────────

/// Strip markdown heading markers (`###`), resolve `Cluster N: Topic` → `"Topic"`,
/// and return `None` for bare cluster labels like `"Cluster 7"` or `"### Cluster 9"`
/// that carry no real search topic.
fn clean_theme_str(raw: &str) -> Option<String> {
    let s = raw.trim().trim_start_matches('#').trim();
    if s.is_empty() {
        return None;
    }

    let s: String = if let Some(colon_pos) = s.find(':') {
        // "Cluster 4: SEO Tools (PLANNED)" → "SEO Tools"
        let after = s[colon_pos + 1..].trim();
        after.split('(').next().unwrap_or(after).trim().to_string()
    } else {
        // Strip trailing parenthetical FIRST, then check for bare cluster label.
        // "Cluster 7 (PLANNED)" → "Cluster 7" → bare → None.
        let s_no_paren = s.split('(').next().unwrap_or(s).trim();
        let words: Vec<&str> = s_no_paren.split_whitespace().collect();
        let is_bare_cluster = words.len() <= 2
            && words
                .first()
                .map(|w| w.eq_ignore_ascii_case("cluster"))
                .unwrap_or(false);
        if is_bare_cluster {
            return None;
        }
        s_no_paren.to_string()
    };

    if s.is_empty() || s.len() <= 2 {
        None
    } else {
        Some(s)
    }
}

/// Parse comma-/newline-separated themes from a task description string,
/// applying the same cleaning rules as brief parsing.
///
/// Returns an empty vec when the description contains only junk cluster labels.
pub(crate) fn parse_desc_themes(raw: &str) -> Vec<String> {
    raw.lines()
        .flat_map(|line| line.split(','))
        .filter_map(clean_theme_str)
        .collect()
}

// ─── Theme auto-derivation ────────────────────────────────────────────────────

/// Try to derive keyword themes from existing project configuration files.
///
/// Priority order:
///   1. `*seo_content_brief*.md` — PLANNED cluster topics (🎯) and gap cluster names
///   2. `*project_summary*.md`   — Content Pillar names
///   3. `articles.json`          — unique existing target_keywords (as baseline coverage)
pub(crate) fn derive_themes_from_project(automation_dir: &std::path::Path) -> Vec<String> {
    if let Some(brief) = find_file_by_suffix(automation_dir, "seo_content_brief.md") {
        log::info!("[keyword_research] using brief: {:?}", brief);
        let themes = extract_from_brief(&brief);
        if !themes.is_empty() {
            return themes;
        }
    }

    if let Some(summary) = find_file_by_suffix(automation_dir, "project_summary.md") {
        log::info!("[keyword_research] using summary: {:?}", summary);
        let themes = extract_from_summary(&summary);
        if !themes.is_empty() {
            return themes;
        }
    }

    let articles_json = automation_dir.join("articles.json");
    if articles_json.exists() {
        let themes = extract_from_articles(&articles_json);
        if !themes.is_empty() {
            return themes;
        }
    }

    vec![]
}

/// Find the first file in `dir` whose name contains `suffix` (case-insensitive).
fn find_file_by_suffix(dir: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
    let exact = dir.join(suffix);
    if exact.exists() {
        return Some(exact);
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    let suffix_lower = suffix.to_lowercase();
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase().contains(&suffix_lower))
                .unwrap_or(false)
        })
}

/// Extract themes from `seo_content_brief.md`.
fn extract_from_brief(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let planned_items: Vec<String> = content
        .lines()
        .filter(|l| l.contains('🎯'))
        .filter_map(|l| {
            // Strip all decorators first, then delegate to clean_theme_str which:
            // - strips '#' heading markers
            // - resolves "Cluster N: Topic (annotation)" → "Topic"
            // - rejects bare "Cluster N" / "### Cluster N" labels
            let stripped = l.trim()
                .trim_start_matches("- [ ] ")
                .trim_start_matches("- [x] ")
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .replace('🎯', "")
                .replace("**", "")
                .trim()
                .to_string();
            clean_theme_str(&stripped)
        })
        .take(8)
        .collect();

    if !planned_items.is_empty() {
        return planned_items;
    }

    let planned_clusters: Vec<String> = content
        .lines()
        .filter(|l| l.contains("PLANNED") && l.starts_with("###"))
        .filter_map(clean_theme_str)
        .take(8)
        .collect();

    if !planned_clusters.is_empty() {
        return planned_clusters;
    }

    content
        .lines()
        .filter(|l| l.starts_with("### Cluster"))
        .filter_map(clean_theme_str)
        .take(6)
        .collect()
}

/// Extract content pillar topics from `project_summary.md`.
fn extract_from_summary(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let mut themes: Vec<String> = content
        .lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("pillar") && l.starts_with("###")
        })
        .map(|l| {
            let s = l.trim_start_matches('#').trim();
            let s = s.split(':').nth(1).unwrap_or(s).trim();
            s.split('(').next().unwrap_or(s).trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .take(6)
        .collect();

    if themes.is_empty() {
        let mut in_keywords = false;
        for line in content.lines() {
            if line.contains("Search Keywords") {
                in_keywords = true;
                continue;
            }
            if in_keywords {
                if line.trim().starts_with('-') || line.trim().starts_with('*') {
                    let kw = line.trim()
                        .trim_start_matches('-')
                        .trim_start_matches('*')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                    if !kw.is_empty() {
                        themes.push(kw);
                    }
                    if themes.len() >= 8 { break; }
                } else if line.trim().is_empty() || line.starts_with('#') {
                    in_keywords = false;
                }
            }
        }
    }

    themes
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Write `content` to `<tmp>/ps_kw_test_<name>.md` and return the path.
    fn write_tmp(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("ps_kw_test_{name}.md"));
        fs::write(&path, content).unwrap();
        path
    }

    // ── extract_from_brief: 🎯 items ─────────────────────────────────────────

    #[test]
    fn brief_goal_markers_extract_topic_names() {
        let path = write_tmp("brief_goals", "\
## Gap Analysis\n\
- [ ] 🎯 SEO Tools for Beginners (PLANNED)\n\
- [ ] 🎯 Content Marketing Strategy\n\
- No marker here\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"SEO Tools for Beginners".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Marketing Strategy".to_string()),
            "got: {themes:?}"
        );
        // Non-goal lines must not appear.
        assert!(!themes.iter().any(|t| t.contains("No marker")));
    }

    #[test]
    fn brief_goal_heading_cluster_style_extracts_topic() {
        // Exact format from the failing brief: "### Cluster N: Topic (annotation) 🎯"
        // Old code returned ["### Cluster 7", "### Cluster 8"] — sending markdown
        // heading tokens straight to Ahrefs.
        let path = write_tmp("brief_goals_heading", "\
### Cluster 7: Risk Management (EMERGING) 🎯\n\
### Cluster 8: Advanced Topics (EMERGING) 🎯\n\
**Cluster 9: IRA / Retirement Account Options (NEW) 🎯**\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) 🎯**\n");
        let themes = extract_from_brief(&path);
        assert!(!themes.iter().any(|t| t.contains('#')), "no # markers: {themes:?}");
        assert!(!themes.iter().any(|t| t.starts_with("Cluster ")), "no bare cluster labels: {themes:?}");
        assert!(themes.contains(&"Risk Management".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Advanced Topics".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"IRA / Retirement Account Options".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Protective Put / Portfolio Hedging".to_string()), "got: {themes:?}");
    }

    // ── extract_from_brief: PLANNED clusters ──────────────────────────────────

    #[test]
    fn brief_planned_cluster_with_colon_extracts_topic() {
        let path = write_tmp("brief_planned", "\
### Cluster 4: Advanced SEO Tactics (PLANNED)\n\
### Cluster 5: Link Building (PLANNED)\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"Advanced SEO Tactics".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Link Building".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_planned_heading_without_colon_is_filtered_out() {
        // "### Cluster 7 (PLANNED)" has no colon → no real topic → must be dropped.
        let path = write_tmp("brief_planned_no_colon", "### Cluster 7 (PLANNED)\n");
        let themes = extract_from_brief(&path);
        assert!(themes.is_empty(), "bare cluster label should be filtered: {themes:?}");
    }

    // ── extract_from_brief: all-clusters fallback ─────────────────────────────

    #[test]
    fn brief_cluster_headings_without_planned_uses_last_resort() {
        let path = write_tmp("brief_clusters", "\
### Cluster 1: On-Page SEO\n\
### Cluster 2: Technical SEO\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"On-Page SEO".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Technical SEO".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_empty_file_returns_empty() {
        let path = write_tmp("brief_empty", "");
        assert!(extract_from_brief(&path).is_empty());
    }

    #[test]
    fn brief_missing_file_returns_empty() {
        assert!(extract_from_brief(std::path::Path::new("/nonexistent/ps_kw_missing.md")).is_empty());
    }

    // ── extract_from_summary ──────────────────────────────────────────────────

    #[test]
    fn summary_pillar_headings_extract_names() {
        let path = write_tmp("summary_pillars", "\
### Pillar 1: Keyword Research\n\
### Pillar 2: Content Creation\n\
## Other section\n");
        let themes = extract_from_summary(&path);
        assert!(
            themes.contains(&"Keyword Research".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Creation".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_search_keywords_list_fallback() {
        let path = write_tmp("summary_keywords", "\
## Search Keywords\n\
- seo tips\n\
- content strategy\n\
## Other\n");
        let themes = extract_from_summary(&path);
        assert!(
            themes.contains(&"seo tips".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"content strategy".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_empty_file_returns_empty() {
        let path = write_tmp("summary_empty", "");
        assert!(extract_from_summary(&path).is_empty());
    }

    // ── find_file_by_suffix ───────────────────────────────────────────────────

    #[test]
    fn find_file_locates_by_partial_name() {
        let dir = std::env::temp_dir().join("ps_kw_find_test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("my_seo_content_brief_v2.md");
        fs::write(&file, "content").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief");
        assert!(found.is_some(), "expected to find file");

        fs::remove_dir_all(&dir).ok();
    }

    // ── clean_theme_str ──────────────────────────────────────────────────────────────

    #[test]
    fn clean_theme_markdown_heading_no_colon_rejected() {
        // Exact inputs from the log: ["### Cluster 7", "### Cluster 8", ...]
        assert_eq!(clean_theme_str("### Cluster 7"), None);
        assert_eq!(clean_theme_str("### Cluster 8"), None);
    }

    #[test]
    fn clean_theme_bare_cluster_label_rejected() {
        assert_eq!(clean_theme_str("Cluster 9"), None);
        assert_eq!(clean_theme_str("Cluster 10"), None);
    }

    #[test]
    fn clean_theme_heading_with_colon_extracts_topic() {
        assert_eq!(
            clean_theme_str("### Cluster 4: SEO Tools"),
            Some("SEO Tools".to_string())
        );
    }

    #[test]
    fn clean_theme_strips_planned_annotation() {
        assert_eq!(
            clean_theme_str("### Cluster 5: Link Building (PLANNED)"),
            Some("Link Building".to_string())
        );
    }

    #[test]
    fn clean_theme_plain_topic_passes_through() {
        assert_eq!(
            clean_theme_str("content marketing"),
            Some("content marketing".to_string())
        );
    }

    #[test]
    fn clean_theme_empty_returns_none() {
        assert_eq!(clean_theme_str(""), None);
        assert_eq!(clean_theme_str("  "), None);
    }

    // ── parse_desc_themes ──────────────────────────────────────────────────────────

    #[test]
    fn parse_desc_exact_failing_log_payload_returns_empty() {
        // This is the exact string that caused the CapSolver failure.
        // After the fix it must produce zero themes so the fallback kicks in.
        let raw = "### Cluster 7, ### Cluster 8, Cluster 9, Cluster 10";
        assert!(
            parse_desc_themes(raw).is_empty(),
            "bare cluster labels must all be filtered out"
        );
    }

    #[test]
    fn parse_desc_topics_with_colon_extracted() {
        let raw = "### Cluster 4: SEO Tools, ### Cluster 5: Link Building";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Tools", "Link Building"]);
    }

    #[test]
    fn parse_desc_plain_comma_list_passes_through() {
        let raw = "seo tools, content marketing, link building";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["seo tools", "content marketing", "link building"]);
    }

    #[test]
    fn parse_desc_newline_separated_works() {
        let raw = "seo tools\ncontent marketing\n";
        assert_eq!(parse_desc_themes(raw), vec!["seo tools", "content marketing"]);
    }

    #[test]
    fn parse_desc_mixed_valid_and_bare_clusters() {
        // If a description has some good themes AND some bare cluster junk,
        // only the good ones should survive.
        let raw = "### Cluster 7, SEO Automation, Cluster 9";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Automation"]);
    }
    // ── derive_themes_from_project integration ────────────────────────────────

    #[test]
    fn derive_themes_real_brief_format_returns_clean_topics() {
        // Exact content structure from the brief that caused the CapSolver failure.
        // Verifies the full stack: find_file → extract_from_brief → clean_theme_str.
        let dir = std::env::temp_dir().join("ps_kw_derive_real");
        fs::create_dir_all(&dir).unwrap();

        let brief = "\
## Existing Clusters\n\
### Cluster 7: Risk Management (EMERGING) \u{1f3af}\n\
**Pillar Content:** Risk management principles\n\
\n\
### Cluster 8: Advanced Topics (EMERGING) \u{1f3af}\n\
**Pillar Content:** Advanced strategies\n\
\n\
### New Clusters Discovered\n\
**Cluster 9: IRA / Retirement Account Options (NEW) \u{1f3af}**\n\
\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) \u{1f3af}**\n";

        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(!themes.is_empty(), "should derive themes, got none");
        assert!(
            !themes.iter().any(|t| t.contains('#')),
            "no markdown heading markers in themes: {themes:?}"
        );
        assert!(
            !themes.iter().any(|t| {
                let w: Vec<_> = t.split_whitespace().collect();
                w.len() <= 2 && w.first().map(|s| s.eq_ignore_ascii_case("cluster")).unwrap_or(false)
            }),
            "no bare 'Cluster N' labels in themes: {themes:?}"
        );
        assert!(themes.contains(&"Risk Management".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Advanced Topics".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"IRA / Retirement Account Options".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Protective Put / Portfolio Hedging".to_string()), "got: {themes:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_bare_cluster_only_brief_returns_empty() {
        // If a brief has ONLY bare "### Cluster N" headings (no colon → no topic),
        // derive_themes should return empty so the executor fails with a clear
        // "No themes found" message instead of sending junk strings to Ahrefs.
        let dir = std::env::temp_dir().join("ps_kw_derive_bare");
        fs::create_dir_all(&dir).unwrap();

        let brief = "### Cluster 7 (PLANNED)\n### Cluster 8 (PLANNED)\nCluster 9\nCluster 10\n";
        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(
            themes.is_empty(),
            "bare cluster labels must produce empty themes (not sent to Ahrefs): {themes:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_without_project_summary_uses_brief_only() {
        // Regression: missing project_summary.md must not crash or block theme derivation.
        let dir = std::env::temp_dir().join("ps_kw_derive_no_summary");
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("seo_content_brief.md"),
            "### Cluster 1: Protective Put (PLANNED)\n",
        )
        .unwrap();

        let themes = derive_themes_from_project(&dir);
        assert_eq!(themes, vec!["Protective Put".to_string()]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_missing_brief_and_summary_returns_empty() {
        // Regression: no brief + no summary should fail gracefully with empty themes.
        let dir = std::env::temp_dir().join("ps_kw_derive_missing_all");
        fs::create_dir_all(&dir).unwrap();

        let themes = derive_themes_from_project(&dir);
        assert!(themes.is_empty(), "expected empty themes, got {themes:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_agent_themes_handles_fenced_json_with_tool_logs() {
        let raw = r#"● Read seo_content_brief.md
 │ .github/automation/seo_content_brief.md
 └ 1 line read

```json
{
  "themes": ["Protective Put", "IRA Options", "Portfolio Hedging"]
}
```
"#;

        let task = crate::models::task::Task {
            id: "t1".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_theme_selection_agent".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some(raw.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let themes = parse_themes_from_agent_artifact(&task);
        assert_eq!(themes, vec!["Protective Put", "IRA Options", "Portfolio Hedging"]);
    }

    #[test]
    fn parse_agent_themes_handles_list_fallback() {
        let raw = "1. Protective Put\n2. IRA Options\n3. Portfolio Hedging";

        let task = crate::models::task::Task {
            id: "t2".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_theme_selection_agent".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some(raw.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let themes = parse_themes_from_agent_artifact(&task);
        assert_eq!(themes, vec!["Protective Put", "IRA Options", "Portfolio Hedging"]);
    }

    #[test]
    fn parse_agent_themes_supports_array_json_contract() {
        let task = crate::models::task::Task {
            id: "t3".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_theme_selection_agent".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some("[\"Protective Put\", \"IRA Options\"]".to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let themes = parse_themes_from_agent_artifact(&task);
        assert_eq!(themes, vec!["Protective Put", "IRA Options"]);
    }
    // ── find_file_by_suffix ──────────────────────────────────────────────────────

    #[test]
    fn find_file_exact_match_returned_first() {
        let dir = std::env::temp_dir().join("ps_kw_find_exact");
        fs::create_dir_all(&dir).unwrap();
        let exact = dir.join("seo_content_brief.md");
        fs::write(&exact, "exact").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief.md");
        assert_eq!(found.unwrap(), exact);

        fs::remove_dir_all(&dir).ok();
    }
}

/// Extract unique target_keywords from `articles.json` as theme seeds.
fn extract_from_articles(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };
    let Ok(articles) = serde_json::from_str::<Vec<serde_json::Value>>(&content) else { return vec![] };

    let mut seen = std::collections::HashSet::new();
    let mut themes = Vec::new();

    for article in &articles {
        if let Some(kw) = article.get("target_keyword").and_then(|v| v.as_str()) {
            if kw.is_empty() { continue; }
            let short: String = kw.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            let lower = short.to_lowercase();
            if seen.insert(lower.clone()) {
                themes.push(short);
            }
        }
        if themes.len() >= 6 { break; }
    }

    themes
}

// ─── Integration tests (require live credentials) ─────────────────────────────
//
// These tests call real external APIs (CapSolver → Ahrefs).
// They are marked `#[ignore]` so normal `cargo test` skips them.
//
// Run with:
//   CAPSOLVER_API_KEY=<key> cargo test --lib keyword_research_integration -- --ignored --nocapture
//
// Requirements:
//   - CAPSOLVER_API_KEY must be set (in env or ~/.config/automation/secrets.env)
//   - Network access to CapSolver and Ahrefs must be available

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::engine::workflows::StepResult;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Build a unique temp directory for a test run.
    fn unique_temp_project_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}_{nanos}"))
    }

    /// Helper: build a minimal fake repo at `dir` with:
    ///   - `.github/automation/seo_content_brief.md` containing `theme`
    ///   - `.github/automation/articles.json` (empty array)
    fn setup_dummy_project(dir: &std::path::Path, theme: &str) {
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&automation).unwrap();

        let brief = format!("## Clusters\n\n### Cluster 1: {theme} (PLANNED)\n");
        fs::write(automation.join("seo_content_brief.md"), brief).unwrap();
        fs::write(automation.join("articles.json"), "[]").unwrap();
    }

    /// Run the full native keyword research flow against a temp dummy project.
    fn run_dummy_project_flow(theme: &str) -> StepResult {
        let dir = unique_temp_project_dir("ps_kw_integration_test");
        setup_dummy_project(&dir, theme);

        let project_path = dir.to_string_lossy().to_string();

        let task = crate::models::task::Task {
            id: "integration-test".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Integration test".to_string()),
            description: None,
            project_id: "test".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        // Need a tokio runtime because exec_keyword_research_native uses block_on.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            tokio::task::spawn_blocking(move || exec_keyword_research_native(&task, &project_path))
                .await
                .unwrap()
        });

        fs::remove_dir_all(&dir).ok();
        result
    }

    /// Full end-to-end: brief → theme extraction → CapSolver → Ahrefs keyword ideas
    /// → difficulty analysis → structured JSON output.
    ///
    /// This is what the "Run" button triggers. If it fails here, it will fail in the app.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn full_keyword_research_pipeline_single_theme() {
        // Resolve CAPSOLVER_API_KEY the same way the app does.
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            // Use a throwaway project path — we only need the secrets resolution.
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY")
                .cloned()
                .unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!("SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env");
            return;
        }

        // Build and run against a minimal throwaway dummy project.
        let result = run_dummy_project_flow("options risk management");

        eprintln!("=== StepResult ===");
        eprintln!("success: {}", result.success);
        eprintln!("message: {}", result.message);
        if let Some(output) = &result.output {
            let v: serde_json::Value = serde_json::from_str(output).unwrap_or_default();
            eprintln!("themes:   {:?}", v["themes"]);
            eprintln!("candidates: {}", v["total_candidates"]);
            eprintln!("analyzed:   {}", v["difficulty"]["total"]);
            eprintln!("results:    {}", v["difficulty"]["results"]);
        }

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("No themes found")
                    || result.message.contains("CAPSOLVER"),
                "unexpected pipeline failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap();

        // Themes must be clean (no # markers, no bare "Cluster N").
        let themes = output["themes"].as_array().unwrap();
        assert!(!themes.is_empty(), "no themes derived");
        for t in themes {
            let s = t.as_str().unwrap();
            assert!(!s.contains('#'), "theme contains # marker: {s}");
            assert!(
                !(s.split_whitespace().count() <= 2
                    && s.split_whitespace().next().map(|w| w.eq_ignore_ascii_case("cluster")).unwrap_or(false)),
                "bare cluster label sent to API: {s}"
            );
        }

        // Must have analysed at least one keyword with KD data.
        let results = output["difficulty"]["results"].as_array().unwrap();
        assert!(!results.is_empty(), "no difficulty results returned");

    }

    /// Lightweight dummy-project smoke flow that still exercises the full live pipeline.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn keyword_research_dummy_project_smoke_flow() {
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY")
                .cloned()
                .unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!("SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env");
            return;
        }

        let result = run_dummy_project_flow("coffee roasting profiles");
        eprintln!("smoke flow success: {}", result.success);
        eprintln!("smoke flow message: {}", result.message);

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("CAPSOLVER"),
                "unexpected smoke-flow failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap_or_default();
        assert!(output.is_object(), "expected JSON output object when successful");
    }
}

#[cfg(test)]
mod volume_tests {
    use super::estimate_volume;

    #[test]
    fn estimate_volume_maps_ahrefs_labels() {
        assert_eq!(estimate_volume("MoreThanOneHundred"), Some(100));
        assert_eq!(estimate_volume("MoreThanOneThousand"), Some(1000));
        assert_eq!(estimate_volume("LessThanOneHundred"), Some(50));
    }

    #[test]
    fn estimate_volume_parses_ranges_and_numbers() {
        assert_eq!(estimate_volume("100-1,000"), Some(550));
        assert_eq!(estimate_volume("2,400"), Some(2400));
    }
}

// Integration tests for keyword research workflow
#[cfg(test)]
mod keyword_workflow_tests {
    use super::*;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::models::task::{Task, TaskRun, TaskStatus, Priority, ExecutionMode, AgentPolicy};
    use chrono::Utc;

    fn in_memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                execution_mode TEXT NOT NULL DEFAULT 'manual',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );",
        ).unwrap();
        conn
    }

    fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
        let id = format!("proj-{}", Utc::now().timestamp_millis());
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, 'Test', ?2, 1)",
            [&id, path],
        ).unwrap();
        id
    }

    fn create_keyword_research_task(project_id: &str, themes: &[&str]) -> Task {
        Task {
            id: format!("task-{}", Utc::now().timestamp_millis()),
            project_id: project_id.to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Automatic,
            agent_policy: AgentPolicy::Optional,
            title: Some("Keyword Research".to_string()),
            description: if themes.is_empty() {
                None // No themes provided - should trigger agentic mode
            } else {
                Some(format!("Themes: {}", themes.join(", ")))
            },
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    /// Test workflow planning with explicit themes (deterministic mode).
    #[test]
    fn workflow_with_explicit_themes_uses_single_deterministic_step() {
        let conn = in_memory_db();
        let temp_dir = std::env::temp_dir().join(format!("ps_kw_test_{}", Utc::now().timestamp_millis()));
        std::fs::create_dir_all(&temp_dir.join(".github").join("automation")).unwrap();
        
        std::fs::write(
            temp_dir.join(".github").join("automation").join("articles.json"),
            r#"{"nextArticleId":1,"articles":[]}"#
        ).unwrap();

        let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());
        let task = create_keyword_research_task(&project_id, &["personal finance", "budgeting"]);

        let handlers = default_handlers();
        let handler = handlers.iter().find(|h| h.supports(&task)).expect("Should find handler");
        let steps = handler.plan(&task);
        
        assert_eq!(steps.len(), 1, "With explicit themes, should have 1 step");
        assert_eq!(steps[0].kind, "keyword_research_cli");
        
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    /// Test workflow planning without themes (requires agentic theme selection).
    #[test]
    fn workflow_without_themes_uses_agentic_plus_deterministic_steps() {
        let conn = in_memory_db();
        let temp_dir = std::env::temp_dir().join(format!("ps_kw_test_{}", Utc::now().timestamp_millis()));
        std::fs::create_dir_all(&temp_dir.join(".github").join("automation")).unwrap();
        
        std::fs::write(
            temp_dir.join(".github").join("automation").join("articles.json"),
            r#"{"nextArticleId":1,"articles":[]}"#
        ).unwrap();

        let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());
        // Create task with empty description (no themes) - this should trigger agentic mode
        let task = create_keyword_research_task(&project_id, &[]);
        
        let handlers = default_handlers();
        let handler = handlers.iter().find(|h| h.supports(&task)).expect("Should find handler");
        let steps = handler.plan(&task);
        
        assert_eq!(steps.len(), 2, "Without explicit themes, should have 2 steps");
        assert_eq!(steps[0].kind, "agentic");
        assert_eq!(steps[1].kind, "keyword_research_cli");
        
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
