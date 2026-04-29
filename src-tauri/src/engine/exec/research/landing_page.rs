use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Deterministic step: write a structured landing page spec file from the task's
/// keyword metadata. No LLM needed — the spec is a template populated with
/// keyword, page type, intent, volume, and KD from the research output.
///
/// Output: writes `specs/landing_page_spec_{slug}.md` inside the automation dir.
pub fn exec_landing_page_spec_write(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let specs_dir = paths.automation_dir.join("specs");

    if let Err(e) = std::fs::create_dir_all(&specs_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create specs directory: {}", e),
            output: None,
        };
    }

    // Parse metadata from task description (format: "Target keyword: X\nKD: Y\nVolume: Z\n...")
    let desc = task.description.as_deref().unwrap_or("");
    let meta = parse_landing_page_meta(desc);

    let slug = slugify(&meta.keyword);
    let filename = format!("landing_page_spec_{}.md", slug);
    let spec_path = specs_dir.join(&filename);

    // Don't overwrite an existing spec — it may have been manually edited.
    if spec_path.exists() {
        return StepResult {
            success: true,
            message: format!("Spec already exists: specs/{}", filename),
            output: Some(format!("specs/{}", filename)),
        };
    }

    let spec_content = build_spec_markdown(&meta, task);

    match std::fs::write(&spec_path, &spec_content) {
        Ok(()) => {
            log::info!("[landing_page_spec] wrote spec: {}", spec_path.display());
            StepResult {
                success: true,
                message: format!("Landing page spec written: specs/{}", filename),
                output: Some(format!("specs/{}", filename)),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Failed to write spec file: {}", e),
            output: None,
        },
    }
}

struct LandingPageMeta {
    keyword: String,
    kd: Option<i64>,
    volume: Option<i64>,
    intent: Option<String>,
    landing_page_type: Option<String>,
    proposed_title: Option<String>,
    opportunity_reason: Option<String>,
}

/// Parse landing page metadata from the task description.
///
/// Expected format (lines):
///   Target keyword: <keyword>
///   KD: <number>
///   Volume: <number>
///   Intent: <string>
///   Page type: <string>
///   Proposed title: <string>
///   Opportunity: <string>
fn parse_landing_page_meta(desc: &str) -> LandingPageMeta {
    let mut meta = LandingPageMeta {
        keyword: String::new(),
        kd: None,
        volume: None,
        intent: None,
        landing_page_type: None,
        proposed_title: None,
        opportunity_reason: None,
    };

    for line in desc.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("Target keyword:") {
            meta.keyword = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("KD:") {
            meta.kd = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("Volume:") {
            meta.volume = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("Intent:") {
            meta.intent = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Page type:") {
            meta.landing_page_type = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Proposed title:") {
            meta.proposed_title = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Opportunity:") {
            meta.opportunity_reason = Some(val.trim().to_string());
        }
    }

    // Fallback: use task title as keyword if description didn't have it
    if meta.keyword.is_empty() {
        if let Some(title) = task_title_fallback(desc) {
            meta.keyword = title;
        }
    }

    meta
}

fn task_title_fallback(desc: &str) -> Option<String> {
    desc.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

fn build_spec_markdown(meta: &LandingPageMeta, task: &Task) -> String {
    let title = meta.proposed_title.as_deref().unwrap_or(&meta.keyword);
    let page_type = meta.landing_page_type.as_deref().unwrap_or("category");
    let intent = meta.intent.as_deref().unwrap_or("commercial");

    let mut out = String::with_capacity(2048);

    out.push_str(&format!("# Landing Page Spec: {}\n\n", title));

    out.push_str("## Keyword Research\n\n");
    out.push_str("| Field | Value |\n|---|---|\n");
    out.push_str(&format!("| Target keyword | {} |\n", meta.keyword));
    if let Some(kd) = meta.kd {
        out.push_str(&format!("| Keyword difficulty | {} |\n", kd));
    }
    if let Some(vol) = meta.volume {
        out.push_str(&format!("| Monthly volume | {} |\n", vol));
    }
    out.push_str(&format!("| Search intent | {} |\n", intent));
    out.push_str(&format!("| Page type | {} |\n", page_type));
    if let Some(reason) = &meta.opportunity_reason {
        out.push_str(&format!("| Opportunity | {} |\n", reason));
    }
    out.push('\n');

    out.push_str("## Page Structure\n\n");

    match page_type {
        "comparison" => {
            out.push_str("This is a **comparison** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — headline addressing the comparison query + value prop\n");
            out.push_str("2. Quick comparison table — side-by-side feature matrix\n");
            out.push_str("3. Detailed breakdown — pros/cons for each option\n");
            out.push_str("4. Use case recommendations — \"Choose X if…\" guidance\n");
            out.push_str("5. CTA — clear next step for the reader\n");
        }
        "use_case" => {
            out.push_str("This is a **use case** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — problem statement + how the product solves it\n");
            out.push_str("2. Step-by-step walkthrough — show the workflow\n");
            out.push_str("3. Benefits — concrete outcomes with evidence\n");
            out.push_str("4. Social proof — testimonials or case study snippets\n");
            out.push_str("5. CTA — get started / try it free\n");
        }
        "feature" => {
            out.push_str("This is a **feature** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — feature name + one-line benefit\n");
            out.push_str("2. Problem/solution — what pain this feature eliminates\n");
            out.push_str("3. How it works — visual walkthrough or demo\n");
            out.push_str("4. Integration/compatibility — what it connects with\n");
            out.push_str("5. CTA — try the feature\n");
        }
        _ => {
            // "category" and default
            out.push_str("This is a **category** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — category overview + primary value prop\n");
            out.push_str("2. Key capabilities — 3-5 feature highlights\n");
            out.push_str("3. Who it's for — target audience segments\n");
            out.push_str("4. Social proof — logos, testimonials, or metrics\n");
            out.push_str("5. CTA — primary conversion action\n");
        }
    }

    out.push_str("\n\n## SEO Requirements\n\n");
    out.push_str("- Primary keyword in H1 and meta title\n");
    out.push_str(&format!("- Target keyword: **{}**\n", meta.keyword));
    out.push_str("- Meta description: 150-160 chars, include keyword naturally\n");
    out.push_str("- URL slug should contain the primary keyword\n");
    out.push_str("- Include structured data (FAQ, HowTo, or Product schema as appropriate)\n");

    out.push_str("\n## Implementation Notes\n\n");
    out.push_str("- This spec defines what the landing page should contain.\n");
    out.push_str("- Implement using the repo's landing page framework/templates.\n");
    out.push_str(&format!("- Source task: `{}`\n", task.id));

    out
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
