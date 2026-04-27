/// Audit script: run Step 1 (research_seed_extraction) for every project in the
/// PageSeeds database and compare the extracted themes/competitors.
///
/// Run with:
///   cargo run --bin seed_extraction_audit
///
/// Optional env vars:
///   - AGENT_PROVIDER: "kimi" (default) | "copilot" | "claude"
///   - DB_PATH: override the default SQLite location
///
/// What it does:
///   1. Opens the PageSeeds SQLite DB
///   2. Lists all registered projects
///   3. For each project, loads project.md / seo_content_brief.md
///   4. Builds the exact same prompt used by the workflow's research_seed_extraction step
///   5. Calls the local agent CLI (kimi/copilot/etc.) — no Tauri runtime needed
///   6. Parses the JSON output into themes + competitors
///   7. Prints a comparison table and writes a JSON report

use std::path::{Path, PathBuf};

use pageseeds_lib::{
    db,
    engine::{project_paths::ProjectPaths, task_store},
    models::{project::Project, research::SeedExtractionOutput},
};

#[derive(Debug, Clone)]
struct ProjectResult {
    project: Project,
    themes: Vec<String>,
    competitors: Vec<String>,
    raw_output: String,
    success: bool,
    error: Option<String>,
}

fn default_db_path() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("com.pageseeds.app").join("pageseeds.db")
    } else {
        PathBuf::from("pageseeds.db")
    }
}

fn build_prompt(project_path: &str) -> (String, String) {
    let paths = ProjectPaths::from_path(project_path);

    // Try project.md first, then any *seo_content_brief*.md
    let brief_content = std::fs::read_to_string(paths.automation_dir.join("project.md"))
        .or_else(|_| {
            find_file_by_suffix(&paths.automation_dir, "seo_content_brief.md")
                .and_then(|p| std::fs::read_to_string(&p).ok())
                .ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, ""))
        })
        .unwrap_or_else(|_| "(no brief found)".to_string());

    let system = include_str!("../prompts/seed_extraction.md");
    let user = format!(
        "## Project Context\n\n{}\n\n## Task Description\n\nExtract seed themes for keyword research\n\n## Project Path\n\n{}",
        brief_content, project_path
    );

    (system.to_string(), user)
}

fn find_file_by_suffix(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let suffix_lower = suffix.to_lowercase();
    std::fs::read_dir(dir)
        .ok()?
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

#[allow(deprecated)]
fn parse_seed_extraction(raw: &str) -> SeedExtractionOutput {
    // Try direct JSON parse first
    if let Ok(parsed) = serde_json::from_str::<SeedExtractionOutput>(raw) {
        return parsed;
    }

    // Fallback: look for a JSON code block
    if let Some(start) = raw.find("```json") {
        let block = &raw[start + 7..];
        if let Some(end) = block.find("```") {
            let json_str = block[..end].trim();
            if let Ok(parsed) = serde_json::from_str::<SeedExtractionOutput>(json_str) {
                return parsed;
            }
        }
    }

    // Fallback: normalizer
    let normalized = pageseeds_lib::engine::normalizer::normalize_agent_output(raw);
    if let Some(json) = normalized.json_artifact {
        if let Ok(parsed) = serde_json::from_str::<SeedExtractionOutput>(&serde_json::to_string(&json).unwrap_or_default()) {
            return parsed;
        }
    }

    SeedExtractionOutput {
        themes: vec![],
        competitors: vec![],
    }
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let db_path = std::env::var("DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_db_path());

    let provider = std::env::var("AGENT_PROVIDER").unwrap_or_else(|_| "kimi".to_string());

    println!("═══════════════════════════════════════════════════════════════");
    println!("  Seed Extraction Audit");
    println!("  DB: {}", db_path.display());
    println!("  Provider: {}", provider);
    println!("═══════════════════════════════════════════════════════════════\n");

    if !db_path.exists() {
        eprintln!("✗ Database not found at {}. Set DB_PATH to override.", db_path.display());
        std::process::exit(1);
    }

    let conn = db::init(&db_path).expect("Failed to open SQLite DB");
    let projects = task_store::list_projects(&conn).expect("Failed to list projects");

    if projects.is_empty() {
        println!("No projects found in database.");
        return;
    }

    println!("Found {} project(s). Running seed extraction...\n", projects.len());

    let mut handles = vec![];

    let total_projects = projects.len();
    for (idx, project) in projects.into_iter().enumerate() {
        let provider_clone = provider.clone();
        let project_path = project.path.clone();
        let project_name = project.name.clone();

        let handle = tokio::spawn(async move {
            println!(
                "───────────────────────────────────────────────────────────────\n[{}/{}] {}\n      Path: {}",
                idx + 1,
                total_projects,
                project_name,
                project_path
            );

            let (system_prompt, user_prompt) = build_prompt(&project_path);
            let full_prompt = format!("{}\n\n{}", system_prompt, user_prompt);
            let project_path_clone = project_path.clone();
            let provider_clone2 = provider_clone.clone();

            let raw_output = match tokio::task::spawn_blocking(move || {
                pageseeds_lib::engine::agent::run_agent(&provider_clone2, &full_prompt, Path::new(&project_path_clone))
            }).await {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => {
                    println!("  ✗ Agent failed for {}: {}", project_name, e);
                    return ProjectResult {
                        project,
                        themes: vec![],
                        competitors: vec![],
                        raw_output: String::new(),
                        success: false,
                        error: Some(e),
                    };
                }
                Err(e) => {
                    println!("  ✗ Task panicked for {}: {}", project_name, e);
                    return ProjectResult {
                        project,
                        themes: vec![],
                        competitors: vec![],
                        raw_output: String::new(),
                        success: false,
                        error: Some(format!("Task panicked: {}", e)),
                    };
                }
            };

            let parsed = parse_seed_extraction(&raw_output);
            let success = !parsed.themes.is_empty();

            println!(
                "  {} themes: {:?}",
                if success { "✓" } else { "✗" },
                parsed.themes.len()
            );
            if !parsed.themes.is_empty() {
                for t in &parsed.themes {
                    println!("    - {}", t);
                }
            }
            if !parsed.competitors.is_empty() {
                println!("  competitors:");
                for c in &parsed.competitors {
                    println!("    - {}", c);
                }
            }

            ProjectResult {
                project,
                themes: parsed.themes,
                competitors: parsed.competitors,
                raw_output,
                success,
                error: None,
            }
        });

        handles.push(handle);
    }

    let mut results: Vec<ProjectResult> = vec![];
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => {
                eprintln!("  ✗ Join error: {}", e);
            }
        }
    }
    results.sort_by(|a, b| a.project.name.cmp(&b.project.name));

    // ── Comparison summary ─────────────────────────────────────────────
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  Comparison Summary");
    println!("═══════════════════════════════════════════════════════════════");

    let max_name_len = results
        .iter()
        .map(|r| r.project.name.len())
        .max()
        .unwrap_or(10)
        .max(10);

    println!(
        "\n  {:<name_width$} │ {:>6} │ {:>10} │ Status",
        "Project",
        "Themes",
        "Competitors",
        name_width = max_name_len
    );
    println!(
        "  {:-<name_width$}─┼--------┼------------┼--------",
        "",
        name_width = max_name_len
    );

    for r in &results {
        println!(
            "  {:<name_width$} │ {:>6} │ {:>10} │ {}",
            r.project.name,
            r.themes.len(),
            r.competitors.len(),
            if r.success { "OK" } else { "FAIL" },
            name_width = max_name_len
        );
    }

    // ── Theme overlap analysis ─────────────────────────────────────────
    println!("\n───────────────────────────────────────────────────────────────");
    println!("  Theme Overlap Analysis");
    println!("───────────────────────────────────────────────────────────────");

    let mut theme_to_projects: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in &results {
        for t in &r.themes {
            let normalized = t.to_lowercase();
            theme_to_projects
                .entry(normalized)
                .or_default()
                .push(r.project.name.clone());
        }
    }

    let mut shared_themes: Vec<_> = theme_to_projects
        .iter()
        .filter(|(_, projects)| projects.len() > 1)
        .collect();
    shared_themes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    if shared_themes.is_empty() {
        println!("\n  No overlapping themes found across projects.");
    } else {
        println!("\n  Themes shared by multiple projects:");
        for (theme, projects) in &shared_themes {
            println!("    '{}' → {} projects: {}", theme, projects.len(), projects.join(", "));
        }
    }

    // ── Unique themes per project ──────────────────────────────────────
    println!("\n  Unique themes per project:");
    for r in &results {
        let unique: Vec<_> = r
            .themes
            .iter()
            .filter(|t| theme_to_projects.get(&t.to_lowercase()).map(|p| p.len()).unwrap_or(0) == 1)
            .collect();
        println!("    {}: {} unique", r.project.name, unique.len());
        for t in &unique {
            println!("      - {}", t);
        }
    }

    // ── Save JSON report ───────────────────────────────────────────────
    let report_path = format!(
        "/tmp/seed_extraction_audit_{}.json",
        chrono::Utc::now().timestamp()
    );
    let report = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "provider": provider,
        "db_path": db_path.to_string_lossy().to_string(),
        "total_projects": results.len(),
        "successful": results.iter().filter(|r| r.success).count(),
        "failed": results.iter().filter(|r| !r.success).count(),
        "shared_themes": shared_themes.iter().map(|(t, p)| serde_json::json!({
            "theme": t,
            "projects": p
        })).collect::<Vec<_>>(),
        "results": results.iter().map(|r| serde_json::json!({
            "project_id": r.project.id,
            "project_name": r.project.name,
            "project_path": r.project.path,
            "success": r.success,
            "error": r.error,
            "themes": r.themes,
            "competitors": r.competitors,
            "raw_output_preview": r.raw_output.chars().take(500).collect::<String>(),
        })).collect::<Vec<_>>(),
    });

    if let Err(e) = std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()) {
        eprintln!("\n! Failed to write report: {}", e);
    } else {
        println!("\n✓ Report saved to: {}", report_path);
    }

    let failed = results.iter().filter(|r| !r.success).count();
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
