//! Minimal test: Call Kimi CLI and parse response

use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_kimi_basic() {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");

    // Simple test prompt
    let prompt = r#"Return ONLY this JSON: {"product": "Test", "status": "ok"}"#;

    println!("=== Kimi Basic Test ===");
    println!("Project path: {:?}", project_path);
    println!("Prompt: {}", prompt);
    println!();

    // Build command exactly like agent.rs does
    let mut cmd = Command::new("kimi");
    cmd.arg("--no-thinking")
        .arg("--print")
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("text")
        .arg("--final-message-only")
        .arg("--session")
        .arg("test-basic-123")
        .arg("--work-dir")
        .arg(project_path);

    println!("Command: {:?}", cmd);
    println!();

    // Execute
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            println!("Exit code: {:?}", output.status.code());
            println!("Stdout length: {} bytes", stdout.len());
            println!("Stderr length: {} bytes", stderr.len());
            println!();

            if !stderr.is_empty() {
                println!("Stderr: {}", stderr);
                println!();
            }

            println!("=== RAW STDOUT (first 2000 chars) ===");
            println!("{}", &stdout[..stdout.len().min(2000)]);
            if stdout.len() > 2000 {
                println!("... ({} more chars)", stdout.len() - 2000);
            }
            println!();

            // Try to parse as JSON
            let trimmed = stdout.trim();
            println!("=== JSON PARSE ATTEMPT ===");
            println!("Trimmed length: {} bytes", trimmed.len());
            println!("Starts with '{{': {}", trimmed.starts_with('{'));
            println!("Ends with '}}': {}", trimmed.ends_with('}'));
            println!();

            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(json) => {
                    println!("✅ Successfully parsed as JSON!");
                    println!("Pretty printed:");
                    println!("{}", serde_json::to_string_pretty(&json).unwrap());
                }
                Err(e) => {
                    println!("❌ JSON parse failed: {}", e);
                    println!();
                    // Show first 500 chars of trimmed for debugging
                    println!("Trimmed content preview:");
                    println!("{}", &trimmed[..trimmed.len().min(500)]);
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to execute Kimi: {}", e);
        }
    }
}

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_kimi_with_config_files() {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let automation_dir = project_path.join(".github/automation");

    // Read actual config files
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .expect("Failed to read reddit_config.md");
    let project_summary =
        std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
    let brandvoice =
        std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();

    // Build the actual prompt used in production
    let prompt = format!(
        "Extract Reddit search parameters from the config files below. Return ONLY a JSON object.\n\n\
        ## reddit_config.md\n\n\
        ```markdown\n\
        {reddit_config}\n\
        ```\n\n\
        ## project_summary.md\n\n\
        ```markdown\n\
        {project_summary}\n\
        ```\n\n\
        ## brandvoice.md\n\n\
        ```markdown\n\
        {brandvoice}\n\
        ```\n\n\
        ## Required JSON Output\n\n\
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

    println!("=== Kimi Config Parse Test ===");
    println!("Prompt length: {} chars", prompt.len());
    println!();

    let mut cmd = Command::new("kimi");
    cmd.arg("--no-thinking")
        .arg("--print")
        .arg("-p")
        .arg(&prompt)
        .arg("--output-format")
        .arg("text")
        .arg("--final-message-only")
        .arg("--session")
        .arg(format!(
            "test-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ))
        .arg("--work-dir")
        .arg(project_path);

    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("Output length: {} bytes", stdout.len());
            println!();

            // Show preview
            println!("=== OUTPUT PREVIEW (first 1000 chars) ===");
            println!("{}", &stdout[..stdout.len().min(1000)]);
            if stdout.len() > 1000 {
                println!("... ({} more chars)", stdout.len() - 1000);
            }
            println!();

            // Try JSON extraction
            let trimmed = stdout.trim();
            if let Some(start) = trimmed.find('{') {
                if let Some(end) = trimmed.rfind('}') {
                    if end > start {
                        let candidate = &trimmed[start..=end];
                        match serde_json::from_str::<serde_json::Value>(candidate) {
                            Ok(json) => {
                                println!("✅ Found valid JSON ({} chars)", candidate.len());
                                println!("{}", serde_json::to_string_pretty(&json).unwrap());
                            }
                            Err(e) => {
                                println!("❌ Invalid JSON: {}", e);
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed: {}", e);
        }
    }
}
