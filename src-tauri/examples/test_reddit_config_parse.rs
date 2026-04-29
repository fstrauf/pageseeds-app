/// Test Reddit config parsing with Kimi
///
/// Run with:
///   cargo run --example test_reddit_config_parse -- <project_path>
///
/// Example:
///   cargo run --example test_reddit_config_parse -- /Users/fstrauf/01_code/call-analyzer
use std::path::Path;

fn main() {
    let project_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/Users/fstrauf/01_code/call-analyzer".to_string());

    let project_path = Path::new(&project_path);
    let automation_dir = project_path.join(".github").join("automation");

    // Read config files
    let reddit_config =
        std::fs::read_to_string(automation_dir.join("reddit_config.md")).unwrap_or_default();
    let project_summary =
        std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
    let brandvoice =
        std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();

    println!("=== reddit_config.md (first 500 chars) ===");
    println!("{}", &reddit_config[..reddit_config.len().min(500)]);
    println!("\n=== END ===\n");

    // Build prompt - iterate on this
    let prompt = format!(
        "Extract Reddit search parameters from the config files below. Return ONLY a JSON object.\n\n\
        ## reddit_config.md\n\
        ```markdown\n\
        {reddit_config}\n\
        ```\n\n\
        ## project_summary.md\n\
        ```markdown\n\
        {project_summary}\n\
        ```\n\n\
        ## brandvoice.md\n\
        ```markdown\n\
        {brandvoice}\n\
        ```\n\n\
        ## Required JSON Output\n\
        Return a JSON object with these exact keys:\n\
        - product_name: string\n\
        - mention_stance: string (REQUIRED, RECOMMENDED, OPTIONAL, or OMIT)\n\
        - trigger_topics: array of strings\n\
        - query_keywords: array of strings (use same as trigger_topics)\n\
        - seed_subreddits: array of strings (WITHOUT r/ prefix)\n\
        - excluded_subreddits: array of strings\n\n\
        ## Example\n\
        If the config has Product Name: Days to Expiry, then return:\n\
        {{\"product_name\": \"Days to Expiry\", ...}}\n\n\
        Do NOT return placeholder text like \"<actual product name>\".\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );

    println!("=== PROMPT (first 800 chars) ===");
    println!("{}", &prompt[..prompt.len().min(800)]);
    println!("\n... (truncated)\n");

    // Call kimi directly using the correct flags
    println!("\n=== CALLING KIMI ===\n");

    let output = std::process::Command::new("kimi")
        .arg("-p")
        .arg(&prompt)
        .arg("--print")
        .arg("--no-thinking")
        .arg("--final-message-only")
        .arg("--output-format")
        .arg("text")
        .current_dir(project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            println!("=== KIMI RAW OUTPUT (first 2000 chars) ===");
            println!("{}", &stdout[..stdout.len().min(2000)]);

            if !stderr.is_empty() {
                println!("\n=== KIMI STDERR ===");
                println!("{}", stderr);
            }

            // Kimi outputs structured format with TextPart containing the JSON
            // Look for the actual JSON inside the output
            println!("\n=== JSON EXTRACTION ===");

            // Try to find JSON in the output - look for the last { } block
            let trimmed = stdout.trim();

            // Find the last occurrence of a JSON object (Kimi puts it in TextPart)
            let mut json_str = None;
            if let Some(last_brace) = trimmed.rfind('}') {
                // Search backwards from last } to find matching {
                let before_end = &trimmed[..=last_brace];
                if let Some(start) = before_end.rfind("{\n  \"product_name\"") {
                    json_str = Some(&trimmed[start..=last_brace]);
                } else if let Some(start) = before_end.rfind('{') {
                    json_str = Some(&trimmed[start..=last_brace]);
                }
            }

            if let Some(json) = json_str {
                // Kimi outputs with escaped newlines - clean them up
                let cleaned = json
                    .replace("\\n", " ")
                    .replace("\\\"", "\"")
                    .replace('\n', " ");
                println!(
                    "Cleaned JSON (first 800 chars): {}",
                    &cleaned[..cleaned.len().min(800)]
                );

                // Check if it contains real data or placeholders
                if cleaned.contains("<actual")
                    || cleaned.contains("<product")
                    || cleaned.contains("Product Name")
                {
                    println!("\n⚠️  WARNING: Contains placeholder text!");
                } else if cleaned.contains("\"product_name\": \"Days to Expiry\"") {
                    println!("\n✅ SUCCESS: Real data extracted!");
                    // Extract key fields with simple string matching
                    let product = extract_field(&cleaned, "product_name");
                    let stance = extract_field(&cleaned, "mention_stance");
                    println!("   Product: {}", product.as_deref().unwrap_or("N/A"));
                    println!("   Stance: {}", stance.as_deref().unwrap_or("N/A"));
                    // Count arrays
                    let topics = count_array_items(&cleaned, "trigger_topics");
                    let subreddits = count_array_items(&cleaned, "seed_subreddits");
                    println!("   Topics: {}", topics);
                    println!("   Subreddits: {}", subreddits);
                } else {
                    println!("\n🤔 UNEXPECTED: JSON doesn't match expected format");
                }

                // Also try to parse as JSON
                match serde_json::from_str::<serde_json::Value>(&cleaned.replace("  ", " ")) {
                    Ok(val) => {
                        println!("\n✅ JSON parsed successfully!");
                        if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                            println!(
                                "Pretty printed (first 600 chars):\n{}",
                                &pretty[..pretty.len().min(600)]
                            );
                        }
                    }
                    Err(e) => {
                        println!("\n⚠️  JSON parse error (but content looks valid): {}", e);
                    }
                }
            } else {
                println!("No JSON object found in output");
            }
        }
        Err(e) => {
            println!("Failed to run kimi: {}", e);
            println!("Make sure kimi is installed: https://github.com/MoonshotAI/kimi-cli");
        }
    }
}

fn extract_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\": \"", field);
    if let Some(start) = json.find(&pattern) {
        let after = &json[start + pattern.len()..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }
    None
}

fn count_array_items(json: &str, field: &str) -> usize {
    // Try exact match first, then with leading space (Kimi sometimes adds spaces)
    let patterns = vec![format!("\"{}\": [", field), format!("\" {}\": [", field)];

    for pattern in patterns {
        if let Some(start) = json.find(&pattern) {
            let after = &json[start + pattern.len()..];
            if let Some(end) = after.find("]") {
                let array_content = &after[..end];
                if array_content.trim().is_empty() {
                    return 0;
                }
                return array_content.matches(',').count() + 1;
            }
        }
    }
    0
}
