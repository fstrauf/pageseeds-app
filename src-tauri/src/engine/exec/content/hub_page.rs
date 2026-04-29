/// Hub page creation execution module.
///
/// Covers the 6-step create_hub_page pipeline:
///   1. hub_load_recommendation  — read approved hub recommendation
///   2. hub_build_brief          — gather spoke metadata, excerpts, GSC metrics
///   3. hub_write                — agentic: generate full MDX via hub-write skill
///   4. hub_apply_draft          — write MDX file, register in SQLite + articles.json
///   5. hub_apply_links          — add hub↔spoke Related Articles links
///   6. hub_validate             — validate frontmatter, H1, word count, spoke links
use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::models::cannibalization::HubRecommendation;
use crate::models::task::Task;
use rusqlite::Connection;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Load Recommendation
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_load_recommendation(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let hub_topic = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Create hub:"))
        .unwrap_or("")
        .trim();

    if hub_topic.is_empty() {
        return StepResult {
            success: false,
            message: "Cannot determine hub topic from task title".to_string(),
            output: None,
        };
    }

    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() {
        return StepResult {
            success: false,
            message: "No cannibalization_strategy artifact found".to_string(),
            output: None,
        };
    }

    let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid strategy JSON: {}", e),
                output: None,
            };
        }
    };

    let empty: Vec<serde_json::Value> = vec![];
    let recommendations = strategy["hub_recommendations"].as_array().unwrap_or(&empty);
    let rec = recommendations.iter().find(|r| {
        r["suggested_title"]
            .as_str()
            .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
            .unwrap_or(false)
            || r["topic"]
                .as_str()
                .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                .unwrap_or(false)
    });

    let rec = match rec {
        Some(r) => r.clone(),
        None => {
            let fallback = recommendations.iter().find(|r| {
                r["topic"]
                    .as_str()
                    .map(|t| {
                        hub_topic.to_lowercase().contains(&t.to_lowercase())
                            || t.to_lowercase().contains(&hub_topic.to_lowercase())
                    })
                    .unwrap_or(false)
            });
            match fallback {
                Some(r) => r.clone(),
                None => {
                    return StepResult {
                        success: false,
                        message: format!("No hub recommendation found matching '{}'", hub_topic),
                        output: None,
                    };
                }
            }
        }
    };

    let out_path = paths
        .automation_dir
        .join(format!("hub_recommendation_{}.json", task.id));
    if let Err(e) = std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&rec).unwrap_or_default(),
    ) {
        log::warn!(
            "[hub_load_recommendation] failed to write {}: {}",
            out_path.display(),
            e
        );
    }

    let json = serde_json::to_string_pretty(&rec).unwrap_or_default();
    StepResult {
        success: true,
        message: format!("Loaded hub recommendation for topic: {}", hub_topic),
        output: Some(json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Build Brief
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_build_brief(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let rec_path = paths
        .automation_dir
        .join(format!("hub_recommendation_{}.json", task.id));
    let rec_json = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Cannot read hub recommendation: {}", e),
                output: None,
            };
        }
    };

    let rec: HubRecommendation = match serde_json::from_str(&rec_json) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid hub recommendation JSON: {}", e),
                output: None,
            };
        }
    };

    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB: {}", e),
                output: None,
            };
        }
    };

    let spokes = gather_spoke_briefs(&conn, &task.project_id, project_path, &rec.spoke_pages);

    let brief = HubBrief {
        topic: rec.topic.clone(),
        suggested_url: rec.suggested_url.clone(),
        suggested_title: rec.suggested_title.clone(),
        intent: rec.intent.clone(),
        target_keyword: rec.suggested_url.replace('-', " "),
        spokes,
    };

    let out_path = paths
        .automation_dir
        .join(format!("hub_brief_{}.json", task.id));
    let brief_json = match serde_json::to_string_pretty(&brief) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize hub brief: {}", e),
                output: None,
            };
        }
    };
    if let Err(e) = std::fs::write(&out_path, &brief_json) {
        log::warn!(
            "[hub_build_brief] failed to write {}: {}",
            out_path.display(),
            e
        );
    }

    StepResult {
        success: true,
        message: format!("Built hub brief with {} spokes", brief.spokes.len()),
        output: Some(brief_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Agentic Outline
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_outline(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "hub-outline") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'hub-outline' not found".to_string(),
                output: None,
            };
        }
    };

    let prompt = skill.content
        + "\n\n---\n\n## Hub Brief\n\n"
        + context_json
        + "\n\nPlease generate a structured HubOutline JSON following the skill instructions."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the HubOutline structure."
        + " Do not include markdown prose, summaries, or explanations outside the JSON.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            let outline_json = crate::engine::text::extract_json(&output)
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(output);

            StepResult {
                success: true,
                message: format!("Hub outline complete: {} chars", outline_json.len()),
                output: Some(outline_json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Agentic Write
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_write(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);
    let paths = ProjectPaths::from_path(project_path);

    let skill = match skills::load_skill(repo_root, "hub-write") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'hub-write' not found".to_string(),
                output: None,
            };
        }
    };

    // Load brief from disk for spoke details (context_json is the outline)
    let brief_path = paths
        .automation_dir
        .join(format!("hub_brief_{}.json", task.id));
    let brief_json = std::fs::read_to_string(&brief_path).unwrap_or_default();

    let prompt = skill.content
        + "\n\n---\n\n## Hub Outline\n\n"
        + context_json
        + "\n\n---\n\n## Hub Brief (spoke details)\n\n"
        + &brief_json
        + "\n\nPlease generate the complete MDX hub page content following the skill instructions."
        + "\n\nCRITICAL: Return ONLY the MDX content. Do not include markdown prose, summaries, or explanations outside the MDX.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            let mdx = if output.trim().starts_with("```") {
                extract_code_block(&output, "mdx")
                    .or_else(|| extract_code_block(&output, ""))
                    .unwrap_or(output)
            } else {
                output
            };

            StepResult {
                success: true,
                message: format!("Hub write complete: {} chars", mdx.len()),
                output: Some(mdx),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Apply Draft
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_apply_draft(
    task: &Task,
    project_path: &str,
    mdx_content: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let brief_path = paths
        .automation_dir
        .join(format!("hub_brief_{}.json", task.id));
    let brief: HubBrief = match std::fs::read_to_string(&brief_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(b) => b,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Invalid hub brief JSON: {}", e),
                    output: None,
                };
            }
        },
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Cannot read hub brief: {}", e),
                output: None,
            };
        }
    };

    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            };
        }
    };

    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB: {}", e),
                output: None,
            };
        }
    };

    let max_existing_id: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM articles WHERE project_id = ?1",
            [&task.project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let meta_next_id: i64 = conn
        .query_row(
            "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
            [&task.project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let article_id = std::cmp::max(max_existing_id + 1, meta_next_id.max(1));

    let slug = brief
        .suggested_url
        .trim_start_matches('/')
        .replace("blog/", "");
    // Sanitize slug for use as a filename — URL paths like "hub/coffee" must not
    // create spurious subdirectories (e.g. "001_hub/coffee_hub.mdx").
    let safe_slug = slug.replace(['/', '\\'], "_").replace('-', "_");
    let filename = format!("{:03}_{}_hub.mdx", article_id, safe_slug);
    let file_path = content_dir.join(&filename);

    if let Err(e) = std::fs::create_dir_all(&content_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create content dir: {}", e),
            output: None,
        };
    }

    // Snapshot existing file if refreshing
    let is_refresh = task.task_type == "refresh_hub_page";
    if is_refresh && file_path.exists() {
        let snapshot_path = paths
            .automation_dir
            .join(format!("hub_snapshot_{}.mdx", task.id));
        if let Err(e) = std::fs::copy(&file_path, &snapshot_path) {
            log::warn!("[hub_apply_draft] failed to snapshot existing hub: {}", e);
        } else {
            log::info!(
                "[hub_apply_draft] snapshotted existing hub to {}",
                snapshot_path.display()
            );
        }
    }

    if let Err(e) = std::fs::write(&file_path, mdx_content) {
        return StepResult {
            success: false,
            message: format!("Failed to write hub file: {}", e),
            output: None,
        };
    }

    let content_rel = content_dir
        .strip_prefix(&paths.repo_root)
        .unwrap_or(Path::new("content"))
        .to_string_lossy()
        .replace('\\', "/");
    let file_ref = format!("./{}/{}", content_rel, filename);

    let word_count = count_words(mdx_content) as i64;
    let title = brief.suggested_title.clone();
    let published_date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    if let Err(e) = conn.execute(
        "INSERT INTO articles (
            id, title, url_slug, file, target_keyword, keyword_difficulty,
            target_volume, published_date, word_count, status,
            content_gaps_addressed, estimated_traffic_monthly, project_id
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        rusqlite::params![
            article_id,
            title,
            slug,
            file_ref,
            Some(brief.target_keyword.clone()),
            Option::<String>::None,
            0i64,
            Some(published_date.clone()),
            word_count,
            "published",
            "[]",
            Option::<String>::None,
            &task.project_id,
        ],
    ) {
        return StepResult {
            success: false,
            message: format!("Failed to insert article: {}", e),
            output: None,
        };
    }

    let _ = conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET next_article_id = excluded.next_article_id",
        rusqlite::params![&task.project_id, article_id + 1],
    );

    if let Err(e) =
        crate::db::export::write_articles_to_repo(&conn, &task.project_id, &paths.repo_root)
    {
        log::warn!("[hub_apply_draft] failed to export articles.json: {}", e);
    }

    StepResult {
        success: true,
        message: format!(
            "Hub draft applied: {} (id={}, {} words)",
            filename, article_id, word_count
        ),
        output: Some(
            serde_json::json!({
                "article_id": article_id,
                "file": file_ref,
                "slug": slug,
                "word_count": word_count,
            })
            .to_string(),
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 6: Apply Links
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_apply_links(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let brief_path = paths
        .automation_dir
        .join(format!("hub_brief_{}.json", task.id));
    let brief: HubBrief = match std::fs::read_to_string(&brief_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(b) => b,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Invalid hub brief JSON: {}", e),
                    output: None,
                };
            }
        },
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Cannot read hub brief: {}", e),
                output: None,
            };
        }
    };

    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            };
        }
    };

    let hub_file = find_hub_file(&content_dir, &brief.suggested_url);
    let hub_path = match hub_file {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!("Hub file not found for slug: {}", brief.suggested_url),
                output: None,
            };
        }
    };

    let hub_content = match std::fs::read_to_string(&hub_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read hub file: {}", e),
                output: None,
            };
        }
    };

    let hub_links: Vec<(String, String)> = brief
        .spokes
        .iter()
        .map(|s| (s.title.clone(), s.url_slug.clone()))
        .collect();

    let updated_hub = append_related_section(&hub_content, &hub_links);

    if updated_hub != hub_content {
        if let Err(e) = std::fs::write(&hub_path, &updated_hub) {
            return StepResult {
                success: false,
                message: format!("Failed to write updated hub file: {}", e),
                output: None,
            };
        }
    }

    let hub_title = brief.suggested_title.clone();
    let hub_slug = brief
        .suggested_url
        .trim_start_matches('/')
        .replace("blog/", "");
    let mut spoke_links_added = 0;

    for spoke in &brief.spokes {
        let spoke_path = content_dir.join(&spoke.file);
        if !spoke_path.exists() {
            let pattern = spoke.url_slug.replace('-', "_");
            let found = std::fs::read_dir(&content_dir)
                .ok()
                .and_then(|mut entries| {
                    entries.find_map(|entry| {
                        let entry = entry.ok()?;
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.contains(&pattern) && name.ends_with(".mdx") {
                            Some(entry.path())
                        } else {
                            None
                        }
                    })
                });
            if let Some(p) = found {
                let spoke_content = match std::fs::read_to_string(&p) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let updated_spoke = add_hub_link_to_spoke(&spoke_content, &hub_title, &hub_slug);
                if updated_spoke != spoke_content {
                    if std::fs::write(&p, &updated_spoke).is_ok() {
                        spoke_links_added += 1;
                    }
                }
            }
            continue;
        }

        let spoke_content = match std::fs::read_to_string(&spoke_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let updated_spoke = add_hub_link_to_spoke(&spoke_content, &hub_title, &hub_slug);
        if updated_spoke != spoke_content {
            if std::fs::write(&spoke_path, &updated_spoke).is_ok() {
                spoke_links_added += 1;
            }
        }
    }

    // Write link plan to automation dir for traceability
    let link_plan = HubLinkPlan {
        hub_slug: hub_slug.clone(),
        hub_file: hub_path.to_string_lossy().to_string(),
        links: {
            let mut entries = Vec::new();
            for (title, slug) in &hub_links {
                entries.push(HubLinkEntry {
                    source_file: hub_path.to_string_lossy().to_string(),
                    target_url: format!("/blog/{}", slug),
                    anchor_text: title.clone(),
                    direction: "hub_to_spoke".to_string(),
                });
            }
            entries
        },
    };
    let plan_path = paths
        .automation_dir
        .join(format!("hub_link_plan_{}.json", task.id));
    if let Ok(plan_json) = serde_json::to_string_pretty(&link_plan) {
        if let Err(e) = std::fs::write(&plan_path, plan_json) {
            log::warn!("[hub_apply_links] failed to write link plan: {}", e);
        }
    }

    StepResult {
        success: true,
        message: format!(
            "Links applied: hub→{} spokes, {} spokes→hub",
            hub_links.len(),
            spoke_links_added
        ),
        output: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 7: Validate
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub(crate) fn exec_hub_validate(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let brief_path = paths
        .automation_dir
        .join(format!("hub_brief_{}.json", task.id));
    let brief: HubBrief = match std::fs::read_to_string(&brief_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(b) => b,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Invalid hub brief JSON: {}", e),
                    output: None,
                };
            }
        },
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Cannot read hub brief: {}", e),
                output: None,
            };
        }
    };

    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            };
        }
    };

    let hub_file = find_hub_file(&content_dir, &brief.suggested_url);
    let hub_path = match hub_file {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!("Hub file not found for slug: {}", brief.suggested_url),
                output: None,
            };
        }
    };

    let content = match std::fs::read_to_string(&hub_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read hub file: {}", e),
                output: None,
            };
        }
    };

    let mut checks = Vec::new();
    let mut all_pass = true;

    let has_frontmatter = content.starts_with("---\n");
    checks.push(HubValidationCheck {
        name: "frontmatter_present".to_string(),
        pass: has_frontmatter,
        message: if has_frontmatter {
            "YAML frontmatter found".to_string()
        } else {
            "Missing YAML frontmatter".to_string()
        },
    });
    if !has_frontmatter {
        all_pass = false;
    }

    let mut has_hub_type = false;
    let mut has_hub_topic = false;
    if let Some((fm, _)) = crate::content::frontmatter::split_mdx(&content) {
        has_hub_type = fm.contains("type:") && fm.contains("hub");
        has_hub_topic = fm.contains("hub_topic:");
    }
    checks.push(HubValidationCheck {
        name: "frontmatter_hub_type".to_string(),
        pass: has_hub_type,
        message: if has_hub_type {
            "Frontmatter has type: hub".to_string()
        } else {
            "Frontmatter missing type: hub".to_string()
        },
    });
    checks.push(HubValidationCheck {
        name: "frontmatter_hub_topic".to_string(),
        pass: has_hub_topic,
        message: if has_hub_topic {
            "Frontmatter has hub_topic".to_string()
        } else {
            "Frontmatter missing hub_topic".to_string()
        },
    });
    if !has_hub_type || !has_hub_topic {
        all_pass = false;
    }

    let has_h1 = content.lines().any(|l| l.trim().starts_with("# "));
    checks.push(HubValidationCheck {
        name: "h1_present".to_string(),
        pass: has_h1,
        message: if has_h1 {
            "H1 heading found".to_string()
        } else {
            "Missing H1 heading".to_string()
        },
    });
    if !has_h1 {
        all_pass = false;
    }

    let word_count = count_words(&content);
    let word_count_ok = word_count >= 1500;
    checks.push(HubValidationCheck {
        name: "word_count".to_string(),
        pass: word_count_ok,
        message: format!("Word count: {} (required ≥ 1500)", word_count),
    });
    if !word_count_ok {
        all_pass = false;
    }

    let mut missing_spokes = Vec::new();
    for spoke in &brief.spokes {
        let expected_link = format!("/blog/{}", spoke.url_slug);
        if !content.contains(&expected_link) {
            missing_spokes.push(spoke.title.clone());
        }
    }
    let all_spokes_linked = missing_spokes.is_empty();
    checks.push(HubValidationCheck {
        name: "spoke_links".to_string(),
        pass: all_spokes_linked,
        message: if all_spokes_linked {
            "All spokes linked from hub".to_string()
        } else {
            format!("Missing links to: {}", missing_spokes.join(", "))
        },
    });
    if !all_spokes_linked {
        all_pass = false;
    }

    // Quality gate: hub URL/title must be broader than spokes (not an exact match)
    let hub_slug_lower = brief
        .suggested_url
        .trim_start_matches('/')
        .replace("blog/", "")
        .to_lowercase();
    let hub_title_lower = brief.suggested_title.to_lowercase();
    let mut colliding_spoke = None;
    for spoke in &brief.spokes {
        if spoke.url_slug.to_lowercase() == hub_slug_lower
            || spoke.title.to_lowercase() == hub_title_lower
        {
            colliding_spoke = Some(spoke.title.clone());
            break;
        }
    }
    let hub_is_broader = colliding_spoke.is_none();
    checks.push(HubValidationCheck {
        name: "hub_broader_than_spokes".to_string(),
        pass: hub_is_broader,
        message: if hub_is_broader {
            "Hub URL/title is broader than spokes".to_string()
        } else {
            format!(
                "Hub URL/title collides with spoke: {}",
                colliding_spoke.unwrap_or_default()
            )
        },
    });
    if !hub_is_broader {
        all_pass = false;
    }

    // Quality gate: route collision (simple heuristic — flag if slug looks like a spoke route)
    let route_collision =
        brief.suggested_url.contains("/hub/") || brief.suggested_url.contains("/guide/");
    checks.push(HubValidationCheck {
        name: "route_collision".to_string(),
        pass: !route_collision,
        message: if !route_collision {
            "No route collision detected".to_string()
        } else {
            "Hub URL may collide with project route conventions".to_string()
        },
    });
    if route_collision {
        all_pass = false;
    }

    // Quality gate: sub-intent preservation (if outline exists, check excluded intents)
    let outline_path = paths
        .automation_dir
        .join(format!("hub_outline_{}.json", task.id));
    let mut sub_intents_preserved = true;
    let mut merged_sub_intents = Vec::new();
    if let Ok(outline_json) = std::fs::read_to_string(&outline_path) {
        if let Ok(outline) = serde_json::from_str::<HubOutline>(&outline_json) {
            for excluded in &outline.excluded_sub_intents {
                let excluded_lower = excluded.to_lowercase();
                if content.to_lowercase().contains(&excluded_lower) {
                    merged_sub_intents.push(excluded.clone());
                    sub_intents_preserved = false;
                }
            }
        }
    }
    checks.push(HubValidationCheck {
        name: "sub_intents_preserved".to_string(),
        pass: sub_intents_preserved,
        message: if sub_intents_preserved {
            "Excluded sub-intents are not merged into hub".to_string()
        } else {
            format!(
                "Hub content may merge excluded sub-intents: {}",
                merged_sub_intents.join(", ")
            )
        },
    });
    if !sub_intents_preserved {
        all_pass = false;
    }

    let report = HubValidationReport {
        hub_file: hub_path.to_string_lossy().to_string(),
        word_count,
        checks,
        all_pass,
    };

    let json = match serde_json::to_string_pretty(&report) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize validation report: {}", e),
                output: None,
            };
        }
    };

    StepResult {
        success: all_pass,
        message: if all_pass {
            "Hub validation passed".to_string()
        } else {
            "Hub validation failed — see checks".to_string()
        },
        output: Some(json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Simplified hub application (single-step flow)
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect a hub file written directly by the agent (rig backend with tool use).
fn find_agent_hub_file(content_dir: &Path) -> Option<PathBuf> {
    let now = std::time::SystemTime::now();
    let five_minutes_ago = now - std::time::Duration::from_secs(300);

    let mut candidates = Vec::new();

    if let Ok(entries) = std::fs::read_dir(content_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = match meta.modified() {
                Ok(m) => m,
                Err(_) => continue,
            };

            if modified < five_minutes_ago {
                continue;
            }

            // Must start with YAML frontmatter (not prose summary)
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.starts_with("---\n")
                    && content.contains("type: hub")
                    && content.contains("hub_topic:")
                {
                    candidates.push((path, modified, content.len()));
                }
            }
        }
    }

    // Prefer the largest file (real content vs. short summary)
    candidates.sort_by(|a, b| b.2.cmp(&a.2));
    candidates.into_iter().next().map(|(p, _, _)| p)
}

/// Write hub MDX to disk, add spoke links, register in DB.
/// Called from post_actions::after_step when a hub task completes its agentic step.
pub(crate) fn apply_hub_output(
    task: &Task,
    project_path: &str,
    agent_output: &str,
    conn: &Connection,
) -> Result<String, String> {
    let paths = ProjectPaths::from_path(project_path);

    // Derive hub topic from task title
    let hub_topic = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Create hub:").or_else(|| t.strip_prefix("Refresh hub:")))
        .unwrap_or("")
        .trim();

    if hub_topic.is_empty() {
        return Err("Cannot determine hub topic from task title".to_string());
    }

    // Load recommendation from task artifacts
    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    if strategy_json.is_empty() {
        return Err("No cannibalization_strategy artifact found".to_string());
    }

    let strategy: serde_json::Value =
        serde_json::from_str(&strategy_json).map_err(|e| format!("Invalid strategy JSON: {}", e))?;

    let empty_recs: Vec<serde_json::Value> = Vec::new();
    let recommendations = strategy
        .get("hub_recommendations")
        .and_then(|r| r.as_array())
        .unwrap_or(&empty_recs);

    let rec = recommendations.iter().find(|r| {
        r.get("suggested_title")
            .and_then(|t| t.as_str())
            .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
            .unwrap_or(false)
            || r.get("topic")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                .unwrap_or(false)
    });

    let rec = match rec {
        Some(r) => r,
        None => return Err(format!("No hub recommendation found matching '{}'", hub_topic)),
    };

    let suggested_url = rec.get("suggested_url").and_then(|u| u.as_str()).unwrap_or("");
    let suggested_title = rec.get("suggested_title").and_then(|t| t.as_str()).unwrap_or(hub_topic);

    let spoke_pages = rec
        .get("spoke_pages")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
        .unwrap_or_default();

    let spokes = if spoke_pages.is_empty() {
        Vec::new()
    } else {
        gather_spoke_briefs(conn, &task.project_id, project_path, &spoke_pages)
    };

    // Resolve content directory
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = resolution
        .selected
        .ok_or("Could not locate content directory")?;

    std::fs::create_dir_all(&content_dir).map_err(|e| e.to_string())?;

    let is_refresh = task.task_type == "refresh_hub_page";

    // Try to find a file the agent already wrote (rig backend with tool use)
    let hub_file = find_agent_hub_file(&content_dir)
        .or_else(|| {
            // Agent did not write a file — fall back to writing from its output
            let mdx = extract_code_block(agent_output, "mdx")
                .or_else(|| extract_code_block(agent_output, ""))
                .unwrap_or_else(|| agent_output.to_string());

            let max_existing_id: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(id), 0) FROM articles WHERE project_id = ?1",
                    [&task.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let meta_next_id: i64 = conn
                .query_row(
                    "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
                    [&task.project_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let article_id = std::cmp::max(max_existing_id + 1, meta_next_id.max(1));

            let slug = suggested_url.trim_start_matches('/').replace("blog/", "");
            let safe_slug = slug.replace(['/', '\\'], "_").replace('-', "_");
            let filename = format!("{:03}_{}_hub.mdx", article_id, safe_slug);
            let file_path = content_dir.join(&filename);

            if is_refresh && file_path.exists() {
                let snapshot_path = paths.automation_dir.join(format!("hub_snapshot_{}.mdx", task.id));
                let _ = std::fs::copy(&file_path, &snapshot_path);
            }

            let hub_links: Vec<(String, String)> = spokes
                .iter()
                .map(|s| (s.title.clone(), s.url_slug.clone()))
                .collect();
            let mdx_with_links = append_related_section(&mdx, &hub_links);

            std::fs::write(&file_path, &mdx_with_links).ok()?;
            Some(file_path)
        })
        .ok_or("Failed to create or find hub file")?;

    let filename = hub_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("hub.mdx")
        .to_string();

    // Ensure spoke links are present in the hub file
    let hub_content = std::fs::read_to_string(&hub_file).unwrap_or_default();
    let hub_links: Vec<(String, String)> = spokes
        .iter()
        .map(|s| (s.title.clone(), s.url_slug.clone()))
        .collect();
    let updated_hub = append_related_section(&hub_content, &hub_links);
    if updated_hub != hub_content {
        let _ = std::fs::write(&hub_file, &updated_hub);
    }

    // Add hub links back to spoke articles
    let hub_title = suggested_title.to_string();
    let hub_slug_clean = suggested_url.trim_start_matches('/').replace("blog/", "");
    for spoke in &spokes {
        let spoke_path = content_dir.join(&spoke.file);
        if !spoke_path.exists() {
            let pattern = spoke.url_slug.replace('-', "_");
            let found = std::fs::read_dir(&content_dir)
                .ok()
                .and_then(|mut entries| {
                    entries.find_map(|entry| {
                        let entry = entry.ok()?;
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.contains(&pattern) && name.ends_with(".mdx") {
                            Some(entry.path())
                        } else {
                            None
                        }
                    })
                });
            if let Some(p) = found {
                if let Ok(spoke_content) = std::fs::read_to_string(&p) {
                    let updated = add_hub_link_to_spoke(&spoke_content, &hub_title, &hub_slug_clean);
                    if updated != spoke_content {
                        let _ = std::fs::write(&p, &updated);
                    }
                }
            }
            continue;
        }
        if let Ok(spoke_content) = std::fs::read_to_string(&spoke_path) {
            let updated = add_hub_link_to_spoke(&spoke_content, &hub_title, &hub_slug_clean);
            if updated != spoke_content {
                let _ = std::fs::write(&spoke_path, &updated);
            }
        }
    }

    // Register in DB
    let content_rel = content_dir
        .strip_prefix(&paths.repo_root)
        .unwrap_or(Path::new("content"))
        .to_string_lossy()
        .replace('\\', "/");
    let file_ref = format!("./{}/{}", content_rel, filename);

    // Read actual metadata from the file so DB matches frontmatter exactly
    let meta = crate::content::ops::read_file_metadata(&hub_file).ok();
    let file_title = meta.as_ref().and_then(|m| m.title.clone()).unwrap_or(hub_title);
    let published_date = meta
        .as_ref()
        .and_then(|m| m.published_date.clone())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
    let word_count = meta.map(|m| m.word_count as i64).unwrap_or_else(|| {
        let hub_content = std::fs::read_to_string(&hub_file).unwrap_or_default();
        count_words(&hub_content) as i64
    });
    let target_keyword = hub_slug_clean.replace('-', " ");

    if is_refresh {
        let _ = conn.execute(
            "DELETE FROM articles WHERE project_id = ?1 AND file = ?2",
            rusqlite::params![&task.project_id, &file_ref],
        );
    }

    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM articles WHERE project_id = ?1 AND file = ?2 LIMIT 1",
            rusqlite::params![&task.project_id, &file_ref],
            |_row| Ok(true),
        )
        .unwrap_or(false);

    if !existing {
        let max_existing_id: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM articles WHERE project_id = ?1",
                [&task.project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let meta_next_id: i64 = conn
            .query_row(
                "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
                [&task.project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let article_id = std::cmp::max(max_existing_id + 1, meta_next_id.max(1));

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                article_id,
                file_title,
                hub_slug_clean,
                file_ref,
                Some(target_keyword),
                Option::<String>::None,
                0i64,
                Some(published_date),
                word_count,
                "published",
                "[]",
                Option::<String>::None,
                &task.project_id,
            ],
        )
        .map_err(|e| format!("Failed to insert article: {}", e))?;

        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id)
             VALUES (?1, ?2)
             ON CONFLICT(project_id) DO UPDATE SET next_article_id = excluded.next_article_id",
            rusqlite::params![&task.project_id, article_id + 1],
        )
        .map_err(|e| e.to_string())?;
    } else {
        let _ = conn.execute(
            "UPDATE articles SET title = ?1, url_slug = ?2, word_count = ?3,
                 published_date = ?4, status = 'published'
             WHERE project_id = ?5 AND file = ?6",
            rusqlite::params![
                file_title,
                hub_slug_clean,
                word_count,
                published_date,
                &task.project_id,
                &file_ref,
            ],
        );
    }

    if let Err(e) = crate::db::export::write_articles_to_repo(conn, &task.project_id, &paths.repo_root)
    {
        log::warn!("[apply_hub_output] failed to export articles.json: {}", e);
    }

    Ok(filename)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data Structures
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubBrief {
    pub topic: String,
    pub suggested_url: String,
    pub suggested_title: String,
    pub intent: String,
    pub target_keyword: String,
    pub spokes: Vec<HubSpokeBrief>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubSpokeBrief {
    pub article_id: i64,
    pub title: String,
    pub url_slug: String,
    pub file: String,
    pub impressions: f64,
    pub excerpt: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubOutline {
    pub title: String,
    pub slug: String,
    pub sections: Vec<HubOutlineSection>,
    pub link_strategy: String,
    pub excluded_sub_intents: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubOutlineSection {
    pub heading: String,
    pub intent: String,
    #[serde(default)]
    pub spoke_ids: Vec<i64>,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubLinkPlan {
    pub hub_slug: String,
    pub hub_file: String,
    pub links: Vec<HubLinkEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubLinkEntry {
    pub source_file: String,
    pub target_url: String,
    pub anchor_text: String,
    pub direction: String, // "hub_to_spoke" or "spoke_to_hub"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubValidationReport {
    pub hub_file: String,
    pub word_count: usize,
    pub checks: Vec<HubValidationCheck>,
    pub all_pass: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubValidationCheck {
    pub name: String,
    pub pass: bool,
    pub message: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn gather_spoke_briefs(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    spoke_ids: &[i64],
) -> Vec<HubSpokeBrief> {
    let paths = ProjectPaths::from_path(project_path);
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = resolution.selected;

    let mut spokes = Vec::new();
    for id in spoke_ids {
        let article: Option<(String, String, String)> = conn
            .query_row(
                "SELECT title, url_slug, file FROM articles WHERE id = ?1 AND project_id = ?2",
                rusqlite::params![id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let (title, url_slug, file) = match article {
            Some((t, s, f)) => (t, s, f),
            None => {
                log::warn!("[hub_build_brief] spoke article {} not found", id);
                continue;
            }
        };

        let excerpt = {
            let repo_path = paths.repo_root.join(&file);
            let path_to_read = if repo_path.exists() {
                repo_path
            } else if let Some(ref dir) = content_dir {
                let basename = std::path::Path::new(&file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file);
                dir.join(basename)
            } else {
                PathBuf::new()
            };
            if path_to_read.exists() {
                read_excerpt(&path_to_read, 300)
            } else {
                String::new()
            }
        };

        let impressions: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(impressions), 0) FROM ctr_query_metrics WHERE project_id = ?1 AND article_id = ?2",
                rusqlite::params![project_id, id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        spokes.push(HubSpokeBrief {
            article_id: *id,
            title,
            url_slug,
            file,
            impressions,
            excerpt,
        });
    }

    spokes
}

pub(crate) fn read_excerpt(file_path: &Path, max_chars: usize) -> String {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let body = crate::content::frontmatter::split_mdx(&content)
        .map(|(_, b)| b)
        .unwrap_or(&content);

    let text = body
        .lines()
        .filter(|l| !l.trim().starts_with("```") && !l.trim().starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ");

    let cleaned = text.replace('*', "").replace('_', "").replace('#', "");

    if cleaned.chars().count() > max_chars {
        let mut excerpt = String::new();
        let mut count = 0;
        for ch in cleaned.chars() {
            if count >= max_chars {
                excerpt.push('…');
                break;
            }
            excerpt.push(ch);
            count += 1;
        }
        excerpt
    } else {
        cleaned
    }
}

pub(crate) fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

pub(crate) fn extract_code_block(text: &str, lang: &str) -> Option<String> {
    let fence = if lang.is_empty() {
        "```".to_string()
    } else {
        format!("``` {}", lang)
    };
    let start = text.find(&fence)? + fence.len();
    let after_start = &text[start..];
    let end = after_start.find("```")?;
    Some(after_start[..end].trim().to_string())
}

pub(crate) fn find_hub_file(content_dir: &Path, suggested_url: &str) -> Option<PathBuf> {
    let slug = suggested_url.trim_start_matches('/').replace("blog/", "");
    let slug_underscore = slug.replace('-', "_");
    // Also check just the basename (e.g. "my-hub" from "guide/my-hub")
    let slug_basename = slug
        .split('/')
        .next_back()
        .unwrap_or(&slug)
        .replace('-', "_");

    if let Ok(entries) = std::fs::read_dir(content_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if (name.contains(&slug_underscore) || name.contains(&slug_basename))
                && name.ends_with("_hub.mdx")
            {
                return Some(entry.path());
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(content_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with("_hub.mdx") {
                if let Ok(content) = std::fs::read_to_string(&entry.path()) {
                    if content.contains(&slug) {
                        return Some(entry.path());
                    }
                }
            }
        }
    }

    None
}

pub(crate) fn append_related_section(content: &str, new_links: &[(String, String)]) -> String {
    if new_links.is_empty() {
        return content.to_string();
    }

    let has_related = content.lines().any(|l| {
        let t = l.trim();
        t.starts_with("##") && t.to_lowercase().contains("related")
    });

    if has_related {
        let mut lines: Vec<&str> = content.lines().collect();
        let mut insert_idx = lines.len();
        let mut found_related = false;
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if found_related && t.starts_with("##") && !t.to_lowercase().contains("related") {
                insert_idx = i;
                break;
            }
            if t.starts_with("##") && t.to_lowercase().contains("related") {
                found_related = true;
            }
        }

        let mut section = String::new();
        for (title, slug) in new_links {
            let link = format!("- [{}](/blog/{})\n", title, slug);
            if !content.contains(&link.trim()) {
                section.push_str(&link);
            }
        }
        if section.is_empty() {
            return content.to_string();
        }
        lines.insert(insert_idx, &section);
        lines.join("\n")
    } else {
        let mut section = String::from("\n\n## Related Articles\n\n");
        for (title, slug) in new_links {
            section.push_str(&format!("- [{}](/blog/{})\n", title, slug));
        }
        format!("{}{}", content.trim_end(), section)
    }
}

pub(crate) fn add_hub_link_to_spoke(spoke_content: &str, hub_title: &str, hub_slug: &str) -> String {
    let link = format!("- [{}](/blog/{})\n", hub_title, hub_slug);
    if spoke_content.contains(&link.trim()) {
        return spoke_content.to_string();
    }

    let has_related = spoke_content.lines().any(|l| {
        let t = l.trim();
        t.starts_with("##") && t.to_lowercase().contains("related")
    });

    if has_related {
        let mut lines: Vec<&str> = spoke_content.lines().collect();
        let mut insert_idx = lines.len();
        let mut found_related = false;
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if found_related && t.starts_with("##") && !t.to_lowercase().contains("related") {
                insert_idx = i;
                break;
            }
            if t.starts_with("##") && t.to_lowercase().contains("related") {
                found_related = true;
            }
        }
        lines.insert(insert_idx, &link);
        lines.join("\n")
    } else {
        let section = format!("\n\n## Related Articles\n\n{}", link);
        format!("{}{}", spoke_content.trim_end(), section)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use uuid::Uuid;

    static DB_MUTEX: Mutex<()> = Mutex::new(());

    struct TempProjectDir {
        path: PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("pageseeds-hub-test-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join(".github").join("automation")).unwrap();
            fs::create_dir_all(path.join("content")).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn file_db(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                active INTEGER DEFAULT 1
            );
            CREATE TABLE articles (
                id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT,
                keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0,
                published_date TEXT,
                word_count INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT,
                review_started_at TEXT,
                last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
            );
            CREATE TABLE articles_meta (
                project_id TEXT PRIMARY KEY,
                next_article_id INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE ctr_query_metrics (
                project_id TEXT NOT NULL,
                article_id INTEGER NOT NULL,
                page_url TEXT NOT NULL,
                query TEXT NOT NULL,
                impressions REAL NOT NULL DEFAULT 0,
                clicks REAL NOT NULL DEFAULT 0,
                ctr REAL NOT NULL DEFAULT 0,
                avg_position REAL NOT NULL DEFAULT 0,
                period_start TEXT,
                period_end TEXT,
                intent TEXT,
                fetched_at TEXT NOT NULL,
                PRIMARY KEY (project_id, article_id, query)
            );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![id, "Test Project", path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES (?1, 10)",
            [id],
        )
        .unwrap();
    }

    fn insert_test_article(conn: &Connection, project_id: &str, id: i64, slug: &str, file: &str) {
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, content_gaps_addressed, project_id)
             VALUES (?1, ?2, ?3, ?4, 'published', '[]', ?5)",
            rusqlite::params![id, format!("Article {}", id), slug, file, project_id],
        )
        .unwrap();
    }

    #[test]
    fn test_hub_load_recommendation_from_artifact() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let strategy = serde_json::json!({
            "hub_recommendations": [
                {
                    "topic": "Cash Secured Puts",
                    "suggested_url": "/blog/cash-secured-puts-hub",
                    "suggested_title": "Cash Secured Puts Hub",
                    "intent": "informational",
                    "spoke_pages": [1, 2],
                    "outline": ["Intro", "Spokes"]
                }
            ]
        });

        let task = Task {
            id: "task-hub-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Cash Secured Puts Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("cannibalization_audit".to_string()),
                content: Some(strategy.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_load_recommendation(&task, &project_path);
        assert!(result.success, "Expected success: {}", result.message);
        assert!(result.output.is_some());
        let out: serde_json::Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["topic"], "Cash Secured Puts");
    }

    #[test]
    fn test_hub_build_brief_gather_spokes() {
        let _lock = DB_MUTEX.lock().unwrap();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();
        let db_path = dir.path().join("test.db");
        {
            let conn = file_db(&db_path);
            std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
            create_test_project(&conn, "proj1", &project_path);

            fs::write(
                dir.path().join("content").join("001_spoke_one.mdx"),
                "---\ntitle: \"Spoke One\"\ndate: \"2024-01-01\"\n---\n\nThis is the first spoke article. It has some content about options trading.\n",
            )
            .unwrap();
            fs::write(
                dir.path().join("content").join("002_spoke_two.mdx"),
                "---\ntitle: \"Spoke Two\"\ndate: \"2024-01-02\"\n---\n\nThis is the second spoke article. It discusses advanced strategies.\n",
            )
            .unwrap();

            insert_test_article(
                &conn,
                "proj1",
                1,
                "spoke-one",
                "./content/001_spoke_one.mdx",
            );
            insert_test_article(
                &conn,
                "proj1",
                2,
                "spoke-two",
                "./content/002_spoke_two.mdx",
            );

            conn.execute(
                "INSERT INTO ctr_query_metrics (project_id, article_id, page_url, query, impressions, clicks, ctr, avg_position, fetched_at)
                 VALUES ('proj1', 1, '/blog/spoke-one', 'query1', 500, 10, 0.02, 5.0, '2024-01-01')",
                [],
            )
            .unwrap();
        }

        let rec = serde_json::json!({
            "topic": "Options Trading",
            "suggested_url": "/blog/options-trading-hub",
            "suggested_title": "Options Trading Hub",
            "intent": "informational",
            "spoke_pages": [1, 2],
            "outline": ["Intro"]
        });

        let rec_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_recommendation_task-hub-2.json");
        fs::write(&rec_path, rec.to_string()).unwrap();

        let task = Task {
            id: "task-hub-2".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Options Trading Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_build_brief(&task, &project_path);
        assert!(result.success, "Expected success: {}", result.message);

        let brief: HubBrief = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(brief.spokes.len(), 2);
        assert!(brief.spokes[0].impressions > 0.0 || brief.spokes[1].impressions > 0.0);
    }

    #[test]
    fn test_hub_apply_draft_writes_file() {
        let _lock = DB_MUTEX.lock().unwrap();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();
        let db_path = dir.path().join("test.db");
        {
            let conn = file_db(&db_path);
            std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
            create_test_project(&conn, "proj1", &project_path);
        }

        fs::write(
            dir.path().join("content").join("000_dummy.mdx"),
            "---\ntitle: \"Dummy\"\n---\n\nDummy.\n",
        )
        .unwrap();

        let brief = HubBrief {
            topic: "Test Topic".to_string(),
            suggested_url: "/blog/test-topic-hub".to_string(),
            suggested_title: "Test Topic Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "test topic".to_string(),
            spokes: vec![],
        };
        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-hub-3.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let mdx = "---\ntitle: \"Test Topic Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Test Topic\"\n---\n\n# Test Topic Hub\n\nThis is a comprehensive guide.\n".to_string();

        let task = Task {
            id: "task-hub-3".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Test Topic Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_apply_draft(&task, &project_path, &mdx);
        assert!(result.success, "Expected success: {}", result.message);

        let hub_file = dir
            .path()
            .join("content")
            .join("010_test_topic_hub_hub.mdx");
        assert!(hub_file.exists(), "Hub file should exist");
    }

    #[test]
    fn test_hub_validation_checks() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let mut body = String::from("# Cash Secured Puts Hub\n\n");
        for i in 0..200 {
            body.push_str(&format!("Paragraph {}. This is content about cash secured puts and options trading strategies. ", i));
        }
        let mdx = format!(
            "---\ntitle: \"Cash Secured Puts Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Cash Secured Puts\"\n---\n\n{}\n\n## Related Articles\n\n- [Spoke One](/blog/spoke-one)\n- [Spoke Two](/blog/spoke-two)\n",
            body
        );

        fs::write(
            dir.path()
                .join("content")
                .join("010_cash_secured_puts_hub.mdx"),
            &mdx,
        )
        .unwrap();

        let brief = HubBrief {
            topic: "Cash Secured Puts".to_string(),
            suggested_url: "/blog/cash-secured-puts-hub".to_string(),
            suggested_title: "Cash Secured Puts Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "cash secured puts".to_string(),
            spokes: vec![
                HubSpokeBrief {
                    article_id: 1,
                    title: "Spoke One".to_string(),
                    url_slug: "spoke-one".to_string(),
                    file: "001_spoke_one.mdx".to_string(),
                    impressions: 0.0,
                    excerpt: "Excerpt one".to_string(),
                },
                HubSpokeBrief {
                    article_id: 2,
                    title: "Spoke Two".to_string(),
                    url_slug: "spoke-two".to_string(),
                    file: "002_spoke_two.mdx".to_string(),
                    impressions: 0.0,
                    excerpt: "Excerpt two".to_string(),
                },
            ],
        };

        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-hub-4.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let task = Task {
            id: "task-hub-4".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Cash Secured Puts Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_validate(&task, &project_path);
        assert!(
            result.success,
            "Expected validation to pass: {}",
            result.message
        );

        let report: HubValidationReport = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert!(report.all_pass);
        assert!(report.word_count >= 1500);
    }

    #[test]
    fn test_hub_validation_fails_low_word_count() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let mdx = "---\ntitle: \"Short Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Short\"\n---\n\n# Short Hub\n\nToo short.\n";
        fs::write(dir.path().join("content").join("010_short_hub.mdx"), mdx).unwrap();

        let brief = HubBrief {
            topic: "Short".to_string(),
            suggested_url: "/blog/short-hub".to_string(),
            suggested_title: "Short Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "short".to_string(),
            spokes: vec![],
        };

        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-hub-5.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let task = Task {
            id: "task-hub-5".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Short Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_validate(&task, &project_path);
        assert!(!result.success, "Expected validation to fail");

        let report: HubValidationReport = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert!(!report.all_pass);
    }

    #[test]
    fn test_hub_refresh_snapshots_existing_file() {
        let _lock = DB_MUTEX.lock().unwrap();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();
        let db_path = dir.path().join("test.db");
        {
            let conn = file_db(&db_path);
            std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
            create_test_project(&conn, "proj1", &project_path);
        }

        fs::write(
            dir.path().join("content").join("000_dummy.mdx"),
            "---\ntitle: \"Dummy\"\n---\n\nDummy.\n",
        )
        .unwrap();

        // Create an existing hub file
        let existing_hub = dir
            .path()
            .join("content")
            .join("010_test_topic_hub_hub.mdx");
        fs::write(
            &existing_hub,
            "---\ntitle: \"Old Hub\"\n---\n\nOld content.\n",
        )
        .unwrap();

        let brief = HubBrief {
            topic: "Test Topic".to_string(),
            suggested_url: "/blog/test-topic-hub".to_string(),
            suggested_title: "Test Topic Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "test topic".to_string(),
            spokes: vec![],
        };
        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-refresh-1.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let mdx = "---\ntitle: \"Test Topic Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Test Topic\"\n---\n\n# Test Topic Hub\n\nRefreshed content.\n".to_string();

        let task = Task {
            id: "task-refresh-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "refresh_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Refresh hub: Test Topic Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_apply_draft(&task, &project_path, &mdx);
        assert!(result.success, "Expected success: {}", result.message);

        // Verify snapshot exists
        let snapshot = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_snapshot_task-refresh-1.mdx");
        assert!(snapshot.exists(), "Snapshot should exist for refresh");
        let snapshot_content = fs::read_to_string(&snapshot).unwrap();
        assert!(snapshot_content.contains("Old content"));
    }

    #[test]
    fn test_hub_link_plan_written() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        // Create hub file
        let mdx = "---\ntitle: \"Options Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Options\"\n---\n\n# Options Hub\n\nContent.\n";
        fs::write(dir.path().join("content").join("010_options_hub.mdx"), mdx).unwrap();

        // Create spoke file
        fs::write(
            dir.path().join("content").join("001_spoke_one.mdx"),
            "---\ntitle: \"Spoke One\"\n---\n\nSpoke content.\n",
        )
        .unwrap();

        let brief = HubBrief {
            topic: "Options".to_string(),
            suggested_url: "/blog/options-hub".to_string(),
            suggested_title: "Options Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "options".to_string(),
            spokes: vec![HubSpokeBrief {
                article_id: 1,
                title: "Spoke One".to_string(),
                url_slug: "spoke-one".to_string(),
                file: "001_spoke_one.mdx".to_string(),
                impressions: 0.0,
                excerpt: "Excerpt".to_string(),
            }],
        };
        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-link-1.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let task = Task {
            id: "task-link-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Options Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_apply_links(&task, &project_path);
        assert!(result.success, "Expected success: {}", result.message);

        // Verify link plan exists
        let plan_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_link_plan_task-link-1.json");
        assert!(plan_path.exists(), "Link plan should be written");

        let plan: HubLinkPlan =
            serde_json::from_str(&fs::read_to_string(&plan_path).unwrap()).unwrap();
        assert_eq!(plan.hub_slug, "options-hub");
        assert!(!plan.links.is_empty());
        assert_eq!(plan.links[0].direction, "hub_to_spoke");
    }

    #[test]
    fn test_hub_validation_route_collision() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let mdx = "---\ntitle: \"Guide Hub\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Guide\"\n---\n\n# Guide Hub\n\nContent.\n";
        fs::write(dir.path().join("content").join("010_my_hub_hub.mdx"), mdx).unwrap();

        // URL with /guide/ should trigger route collision
        let brief = HubBrief {
            topic: "Guide".to_string(),
            suggested_url: "/guide/my-hub".to_string(),
            suggested_title: "Guide Hub".to_string(),
            intent: "informational".to_string(),
            target_keyword: "guide".to_string(),
            spokes: vec![],
        };
        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-route-1.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let task = Task {
            id: "task-route-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Guide Hub".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_validate(&task, &project_path);
        // Should fail due to route collision
        assert!(
            !result.success,
            "Expected validation to fail due to route collision"
        );

        let report: HubValidationReport = serde_json::from_str(&result.output.unwrap()).unwrap();
        let route_check = report
            .checks
            .iter()
            .find(|c| c.name == "route_collision")
            .unwrap();
        assert!(!route_check.pass);
    }

    #[test]
    fn test_hub_validation_hub_broader_than_spokes() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let mdx = "---\ntitle: \"Spoke One\"\ndate: \"2024-01-01\"\ntype: hub\nhub_topic: \"Spoke\"\n---\n\n# Spoke One\n\nContent.\n- [Spoke One](/blog/spoke-one)\n";
        fs::write(
            dir.path().join("content").join("010_spoke_one_hub.mdx"),
            mdx,
        )
        .unwrap();

        // Hub title collides with spoke title
        let brief = HubBrief {
            topic: "Spoke".to_string(),
            suggested_url: "/blog/spoke-one-hub".to_string(),
            suggested_title: "Spoke One".to_string(),
            intent: "informational".to_string(),
            target_keyword: "spoke".to_string(),
            spokes: vec![HubSpokeBrief {
                article_id: 1,
                title: "Spoke One".to_string(),
                url_slug: "spoke-one".to_string(),
                file: "001_spoke_one.mdx".to_string(),
                impressions: 0.0,
                excerpt: "Excerpt".to_string(),
            }],
        };
        let brief_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("hub_brief_task-broad-1.json");
        fs::write(&brief_path, serde_json::to_string(&brief).unwrap()).unwrap();

        let task = Task {
            id: "task-broad-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "create_hub_page".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Spec,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Create hub: Spoke One".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_hub_validate(&task, &project_path);
        assert!(
            !result.success,
            "Expected validation to fail: hub title matches spoke"
        );

        let report: HubValidationReport = serde_json::from_str(&result.output.unwrap()).unwrap();
        let broad_check = report
            .checks
            .iter()
            .find(|c| c.name == "hub_broader_than_spokes")
            .unwrap();
        assert!(!broad_check.pass);
    }
}
