//! Structured content briefs attached to content tasks at creation time.
//!
//! Assembled deterministically from the research task's artifacts and the
//! article store — no LLM call. Selection/task-construction logic lives in
//! [`crate::engine::keyword_selection`]; this module owns the brief data
//! model, the research-metadata extractors that feed it, and the
//! internal-link candidate ranking.

use crate::engine::keyword_selection::{
    find_research_selection_artifact, meta_by_keyword, normalize_keyword, parse_artifact_json,
    push_unique_keyword, KeywordMetric, LandingPageCandidateMeta,
};
use crate::models::article::Article;
use crate::models::task::{Task, TaskArtifact};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Metadata for an article keyword from the research output — the article-side
/// fields of `SelectedKeyword` in the `research_final_selection` artifact's
/// `results` array (mirrors [`LandingPageCandidateMeta`] for landing pages).
#[derive(Debug, Clone)]
pub struct ArticleKeywordMeta {
    pub intent: Option<String>,
    pub recommended_title: Option<String>,
    pub selection_reason: Option<String>,
    pub winnability: Option<String>,
    pub winnability_reason: Option<String>,
}

/// Maximum number of internal-link candidates shipped with each content brief.
pub const MAX_INTERNAL_LINK_CANDIDATES: usize = 15;

/// A valid internal link target (slug + title) for the article writer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InternalLinkCandidate {
    pub slug: String,
    pub title: String,
}

/// Structured brief attached to every content task at creation time (artifact
/// key `content_brief`). Assembled deterministically from the research task's
/// artifacts and the article store — no LLM call. Landing pages and articles
/// share this single construction path; absent fields are omitted from the
/// serialized JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentBrief {
    pub keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Landing pages only (e.g. alternative|use_case|comparison).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opportunity_reason: Option<String>,
    /// Articles only: "target" | "differentiate" | "avoid".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winnability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winnability_reason: Option<String>,
    /// Articles only: why the research pipeline selected this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_territories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saturated_themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub internal_link_candidates: Vec<InternalLinkCandidate>,
}

/// Project-side context needed to assemble content briefs at task creation.
/// Kept separate from the DB so brief construction stays pure and testable.
#[derive(Debug, Clone, Default)]
pub struct ContentBriefContext {
    /// Research themes from the seed extraction step.
    pub themes: Vec<String>,
    /// Open territory names from the territory analysis step.
    pub open_territories: Vec<String>,
    /// Saturated theme names from the territory analysis step.
    pub saturated_themes: Vec<String>,
    /// Project articles (candidates for internal links).
    pub articles: Vec<Article>,
    /// Valid internal link targets (normalized slugs, redirects excluded).
    pub valid_link_targets: HashSet<String>,
}

/// Assemble the project-side context needed to build content briefs at task
/// creation time: research themes/territory from the research task's
/// artifacts, plus the project's articles and valid internal-link targets
/// from SQLite. Every piece degrades gracefully to empty when unavailable.
pub(crate) fn load_content_brief_context(
    conn: &rusqlite::Connection,
    project_id: &str,
    research_task: &Task,
) -> ContentBriefContext {
    let articles = crate::engine::task_store::list_articles(conn, project_id).unwrap_or_default();
    // Valid targets = project slugs minus redirected slugs (needs the project
    // path for redirects.csv); without a project row there are no candidates.
    let valid_link_targets = crate::engine::task_store::get_project(conn, project_id)
        .and_then(|p| crate::engine::task_store::load_valid_link_targets(conn, project_id, &p.path))
        .unwrap_or_default();
    let (open_territories, saturated_themes) = extract_territory_summary(research_task);
    ContentBriefContext {
        themes: extract_research_themes(research_task),
        open_territories,
        saturated_themes,
        articles,
        valid_link_targets,
    }
}

/// Extract article keyword metadata (intent, recommended title, selection
/// reason, winnability) keyed by normalized keyword. Mirrors
/// [`crate::engine::keyword_selection::extract_landing_page_meta`]: reads the
/// `SelectedKeyword` entries from the `research_final_selection` artifact
/// (`difficulty.results` as stored by the deterministic selection step, with a
/// top-level `results` fallback) so the article writer gets the same research
/// context the landing page writer gets.
pub fn extract_article_keyword_meta(task: &Task) -> HashMap<String, ArticleKeywordMeta> {
    let Some(v) = parse_artifact_json(task, find_research_selection_artifact(task)) else {
        return HashMap::new();
    };
    // Deterministic selection stores results wrapped in a difficulty
    // object; the unified ResearchFinalOutput uses a top-level array.
    let Some(arr) = v
        .get("difficulty")
        .and_then(|x| x.get("results"))
        .and_then(|x| x.as_array())
        .or_else(|| v.get("results").and_then(|x| x.as_array()))
    else {
        return HashMap::new();
    };
    meta_by_keyword(arr, |item| {
        let kw = item.get("keyword")?.as_str()?;
        let meta = ArticleKeywordMeta {
            intent: item
                .get("intent")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            recommended_title: item
                .get("recommended_title")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            selection_reason: item
                .get("selection_reason")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            winnability: item
                .get("winnability")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            winnability_reason: item
                .get("winnability_reason")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        };
        Some((normalize_keyword(kw), meta))
    })
}

/// Extract the research themes from the `research_seed_extraction` step
/// artifact (`themes`), falling back to the validated-seed themes from
/// `research_seed_validation`. Already computed by the research pipeline —
/// no LLM call.
pub fn extract_research_themes(task: &Task) -> Vec<String> {
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_seed_extraction")
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "research_seed_validation")
        });
    let Some(v) = parse_artifact_json(task, artifact) else {
        return Vec::new();
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    if let Some(arr) = v.get("themes").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(theme) = item.as_str() {
                push_unique_keyword(&mut out, &mut seen, theme);
            }
        }
    } else if let Some(arr) = v.get("validated_seeds").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(theme) = item.get("theme").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, theme);
            }
        }
    }
    out
}

/// Extract territory context (open territory and saturated theme names) from
/// the `research_territory_analysis` step artifact. Names only, to keep the
/// brief compact. Already computed by the research pipeline — no LLM call.
pub fn extract_territory_summary(task: &Task) -> (Vec<String>, Vec<String>) {
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_territory_analysis");
    let Some(v) = parse_artifact_json(task, artifact) else {
        return (Vec::new(), Vec::new());
    };

    let names = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("theme").and_then(|t| t.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    (names("open_territories"), names("saturated_themes"))
}

/// Assemble the structured brief for one content task. Single construction
/// path for articles and landing pages: metrics + selection metadata from the
/// research artifacts, territory/theme context and internal-link candidates
/// from the brief context. Deterministic — no LLM call.
pub fn build_content_brief(
    keyword: &str,
    metric: Option<&KeywordMetric>,
    lp_meta: Option<&LandingPageCandidateMeta>,
    article_meta: Option<&ArticleKeywordMeta>,
    ctx: &ContentBriefContext,
) -> ContentBrief {
    let mut brief = ContentBrief {
        keyword: keyword.to_string(),
        difficulty: metric.and_then(|m| m.difficulty),
        volume: metric.and_then(|m| m.volume),
        themes: ctx.themes.clone(),
        open_territories: ctx.open_territories.clone(),
        saturated_themes: ctx.saturated_themes.clone(),
        internal_link_candidates: select_internal_link_candidates(
            &ctx.articles,
            &ctx.valid_link_targets,
            keyword,
            MAX_INTERNAL_LINK_CANDIDATES,
        ),
        ..Default::default()
    };
    if let Some(lp) = lp_meta {
        brief.intent = lp.intent.clone();
        brief.page_type = lp.landing_page_type.clone();
        brief.proposed_title = lp.proposed_title.clone();
        brief.opportunity_reason = lp.opportunity_reason.clone();
    }
    if let Some(am) = article_meta {
        brief.intent = am.intent.clone();
        brief.proposed_title = am.recommended_title.clone();
        brief.selection_reason = am.selection_reason.clone();
        brief.winnability = am.winnability.clone();
        brief.winnability_reason = am.winnability_reason.clone();
    }
    brief
}

/// Human-readable one-liner summary of the brief, stored as the task
/// description. Omits lines for fields the research did not provide.
pub fn build_content_task_description(brief: &ContentBrief) -> String {
    let mut description = format!("Target keyword: {}", brief.keyword);
    if let Some(kd) = brief.difficulty {
        description.push_str(&format!("\nKD: {}", kd));
    }
    if let Some(vol) = brief.volume {
        description.push_str(&format!("\nVolume: {}", vol));
    }
    if let Some(ref intent) = brief.intent {
        description.push_str(&format!("\nIntent: {}", intent));
    }
    if let Some(ref page_type) = brief.page_type {
        description.push_str(&format!("\nPage type: {}", page_type));
    }
    if let Some(ref proposed_title) = brief.proposed_title {
        description.push_str(&format!("\nProposed title: {}", proposed_title));
    }
    if let Some(ref winnability) = brief.winnability {
        match brief.winnability_reason {
            Some(ref reason) => {
                description.push_str(&format!("\nWinnability: {} — {}", winnability, reason))
            }
            None => description.push_str(&format!("\nWinnability: {}", winnability)),
        }
    }
    if let Some(ref reason) = brief.opportunity_reason {
        description.push_str(&format!("\nOpportunity: {}", reason));
    }
    if let Some(ref reason) = brief.selection_reason {
        description.push_str(&format!("\nWhy selected: {}", reason));
    }
    description
}

/// Serialize the brief as the `content_brief` task artifact. The executor
/// inlines task artifacts into the agent prompt (`exec/agentic.rs`), so the
/// writer receives intent, winnability, territory, and the valid internal-link
/// candidates without any extra wiring.
pub fn build_content_brief_artifact(brief: &ContentBrief, research_task_id: &str) -> TaskArtifact {
    TaskArtifact {
        key: "content_brief".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(research_task_id.to_string()),
        content: serde_json::to_string_pretty(brief).ok(),
    }
}

/// Pick the top-N internal-link candidates for a keyword: articles whose slug
/// is a valid link target (redirected slugs excluded), ranked by token overlap
/// with the keyword (relevance), then word count (substance), then slug
/// (deterministic tie-break).
pub fn select_internal_link_candidates(
    articles: &[Article],
    valid_link_targets: &HashSet<String>,
    keyword: &str,
    limit: usize,
) -> Vec<InternalLinkCandidate> {
    let keyword_tokens = token_set(keyword);
    let mut scored: Vec<(usize, i64, String, String)> = articles
        .iter()
        .map(|a| (crate::content::slug::normalize_url_slug(&a.url_slug), a))
        .filter(|(slug, _)| valid_link_targets.contains(slug))
        .map(|(slug, a)| {
            let haystack = format!("{} {}", a.target_keyword.as_deref().unwrap_or(""), a.title);
            let relevance = token_set(&haystack).intersection(&keyword_tokens).count();
            (relevance, a.word_count, slug, a.title.clone())
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, slug, title)| InternalLinkCandidate { slug, title })
        .collect()
}

/// Lowercase alphanumeric tokens (length > 1) for keyword/article matching.
fn token_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(|s| s.to_lowercase())
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::keyword_selection::build_content_tasks_from_keywords;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };

    fn make_task(artifacts: Vec<TaskArtifact>) -> Task {
        Task {
            id: "test-kw".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Review,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some("Keyword test".to_string()),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts,
            run: TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            not_before: None,
        }
    }

    fn artifact(key: &str, content: serde_json::Value) -> TaskArtifact {
        TaskArtifact {
            key: key.to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some(content.to_string()),
        }
    }

    fn make_article(
        slug: &str,
        title: &str,
        target_keyword: Option<&str>,
        word_count: i64,
    ) -> Article {
        Article {
            id: 0,
            title: title.to_string(),
            url_slug: slug.to_string(),
            file: format!("{slug}.mdx"),
            target_keyword: target_keyword.map(|s| s.to_string()),
            keyword_difficulty: None,
            target_volume: 0,
            published_date: None,
            word_count,
            status: "published".to_string(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            project_id: "proj1".to_string(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            page_type: None,
            content_hash: None,
            last_edited_at: None,
        }
    }

    /// Artifact shaped like the real `research_final_selection` step output for
    /// article research: SelectedKeyword entries under `difficulty.results`.
    fn final_selection_artifact() -> TaskArtifact {
        artifact(
            "research_final_selection",
            serde_json::json!({
                "difficulty": {
                    "total": 1,
                    "successful": 1,
                    "results": [
                        {
                            "keyword": "covered call strategy",
                            "volume": 2400,
                            "difficulty": 22,
                            "selection_reason": "KD 22 with 2400 monthly searches",
                            "recommended_title": "Covered Call Strategy for Small Accounts (2026)",
                            "intent": "informational",
                            "winnability": "target",
                            "winnability_reason": "No AI Overview, weak SERP incumbents"
                        }
                    ]
                },
                "total_candidates": 10,
                "filtered_out": 9
            }),
        )
    }

    // ── extract_article_keyword_meta ──────────────────────────────────────────

    #[test]
    fn article_meta_reads_difficulty_results() {
        let task = make_task(vec![final_selection_artifact()]);
        let meta = extract_article_keyword_meta(&task);
        let m = meta.get("covered call strategy").expect("meta not found");
        assert_eq!(m.intent.as_deref(), Some("informational"));
        assert_eq!(
            m.recommended_title.as_deref(),
            Some("Covered Call Strategy for Small Accounts (2026)")
        );
        assert_eq!(m.winnability.as_deref(), Some("target"));
        assert_eq!(
            m.winnability_reason.as_deref(),
            Some("No AI Overview, weak SERP incumbents")
        );
        assert_eq!(
            m.selection_reason.as_deref(),
            Some("KD 22 with 2400 monthly searches")
        );
    }

    #[test]
    fn article_meta_reads_top_level_results_fallback() {
        let json = serde_json::json!({
            "results": [
                {"keyword": "iron condor", "selection_reason": "picked", "recommended_title": "Iron Condor Guide"}
            ]
        });
        let task = make_task(vec![artifact("research_final_selection", json)]);
        let meta = extract_article_keyword_meta(&task);
        assert!(meta.contains_key("iron condor"), "got: {meta:?}");
    }

    #[test]
    fn article_meta_empty_for_landing_page_artifact() {
        let json = serde_json::json!({
            "landing_page_candidates": [{"keyword": "pricing", "intent": "transactional"}]
        });
        let task = make_task(vec![artifact("research_final_selection", json)]);
        assert!(extract_article_keyword_meta(&task).is_empty());
    }

    // ── extract_research_themes / extract_territory_summary ──────────────────

    #[test]
    fn themes_from_seed_extraction() {
        let json = serde_json::json!({"themes": ["options income", "covered calls"], "competitors": []});
        let task = make_task(vec![artifact("research_seed_extraction", json)]);
        assert_eq!(extract_research_themes(&task), vec!["options income", "covered calls"]);
    }

    #[test]
    fn themes_fall_back_to_validated_seeds() {
        let json = serde_json::json!({"validated_seeds": [{"theme": "wheel strategy", "seeds": ["wheel"]}]});
        let task = make_task(vec![artifact("research_seed_validation", json)]);
        assert_eq!(extract_research_themes(&task), vec!["wheel strategy"]);
    }

    #[test]
    fn territory_summary_reads_theme_names() {
        let json = serde_json::json!({
            "open_territories": [{"theme": "cash secured puts", "article_count": 1, "total_impressions": 6000.0, "source_keywords": []}],
            "saturated_themes": [{"theme": "covered calls", "article_count": 7, "total_impressions": 900.0, "source_keywords": []}],
            "total_themes": 4,
            "synced_to_shortlist": 2
        });
        let task = make_task(vec![artifact("research_territory_analysis", json)]);
        let (open, saturated) = extract_territory_summary(&task);
        assert_eq!(open, vec!["cash secured puts"]);
        assert_eq!(saturated, vec!["covered calls"]);
    }

    // ── select_internal_link_candidates ───────────────────────────────────────

    #[test]
    fn link_candidates_filtered_to_valid_targets_and_ranked_by_relevance() {
        let articles = vec![
            make_article("what-is-a-covered-call", "What Is a Covered Call?", Some("covered call basics"), 900),
            make_article("unrelated-post", "Unrelated Post", Some("dividend aristocrats"), 5000),
            make_article("redirected-slug", "Redirected", Some("covered call strategy"), 3000),
        ];
        let valid: HashSet<String> = ["what-is-a-covered-call", "unrelated-post"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let candidates = select_internal_link_candidates(&articles, &valid, "covered call strategy", 15);
        // Redirected slug is not a valid target and must be excluded.
        assert_eq!(candidates.len(), 2);
        // Relevance (token overlap with the keyword) outranks raw word count.
        assert_eq!(candidates[0].slug, "what-is-a-covered-call");
        assert_eq!(candidates[1].slug, "unrelated-post");
    }

    #[test]
    fn link_candidates_respect_limit() {
        let articles: Vec<Article> = (0..20)
            .map(|i| make_article(&format!("post-{i:02}"), &format!("Post {i}"), None, 100))
            .collect();
        let valid: HashSet<String> = articles.iter().map(|a| a.url_slug.clone()).collect();
        let candidates = select_internal_link_candidates(&articles, &valid, "anything", 15);
        assert_eq!(candidates.len(), 15);
    }

    // ── build_content_brief / description / artifact ──────────────────────────

    #[test]
    fn brief_includes_article_fields_territory_and_candidates() {
        let task = make_task(vec![
            final_selection_artifact(),
            artifact("research_seed_extraction", serde_json::json!({"themes": ["options income"]})),
        ]);
        let meta = extract_article_keyword_meta(&task);
        let ctx = ContentBriefContext {
            themes: extract_research_themes(&task),
            articles: vec![make_article("cc-basics", "Covered Call Basics", Some("covered calls"), 800)],
            valid_link_targets: ["cc-basics".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let brief = build_content_brief(
            "covered call strategy",
            None,
            None,
            meta.get("covered call strategy"),
            &ctx,
        );
        assert_eq!(brief.intent.as_deref(), Some("informational"));
        assert_eq!(brief.winnability.as_deref(), Some("target"));
        assert_eq!(
            brief.proposed_title.as_deref(),
            Some("Covered Call Strategy for Small Accounts (2026)")
        );
        assert_eq!(brief.themes, vec!["options income"]);
        assert_eq!(brief.internal_link_candidates.len(), 1);
        assert_eq!(brief.internal_link_candidates[0].slug, "cc-basics");

        let description = build_content_task_description(&brief);
        assert!(description.contains("Target keyword: covered call strategy"));
        assert!(description.contains("Intent: informational"));
        assert!(description.contains("Proposed title: Covered Call Strategy for Small Accounts (2026)"));
        assert!(description.contains("Winnability: target — No AI Overview, weak SERP incumbents"));
        assert!(description.contains("Why selected: KD 22 with 2400 monthly searches"));
    }

    #[test]
    fn brief_omits_missing_fields_gracefully() {
        let brief = build_content_brief(
            "bare keyword",
            None,
            None,
            None,
            &ContentBriefContext::default(),
        );
        let description = build_content_task_description(&brief);
        assert_eq!(description, "Target keyword: bare keyword");

        // Absent optionals must not appear in the serialized artifact either.
        let json = serde_json::to_string(&brief).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("intent").is_none());
        assert!(v.get("winnability").is_none());
        assert!(v.get("proposed_title").is_none());
        assert!(v.get("internal_link_candidates").is_none());
    }

    #[test]
    fn content_tasks_carry_content_brief_artifact() {
        let research_task = make_task(vec![final_selection_artifact()]);
        let ctx = ContentBriefContext {
            articles: vec![make_article("cc-basics", "Covered Call Basics", Some("covered calls"), 800)],
            valid_link_targets: ["cc-basics".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let tasks = build_content_tasks_from_keywords(
            vec!["covered call strategy".to_string()],
            &research_task,
            "research-1",
            "proj1",
            &ctx,
        )
        .expect("tasks");
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.task_type, "write_article");
        let description = task.description.as_deref().unwrap_or_default();
        assert!(description.contains("Intent: informational"));
        assert!(description.contains("Winnability: target"));

        let brief_artifact = task
            .artifacts
            .iter()
            .find(|a| a.key == "content_brief")
            .expect("content_brief artifact");
        let brief: ContentBrief =
            serde_json::from_str(brief_artifact.content.as_deref().unwrap()).unwrap();
        assert_eq!(brief.keyword, "covered call strategy");
        assert_eq!(brief.difficulty, Some(22));
        assert_eq!(brief.volume, Some(2400));
        assert_eq!(brief.internal_link_candidates.len(), 1);
        assert_eq!(brief.internal_link_candidates[0].slug, "cc-basics");
    }
}
