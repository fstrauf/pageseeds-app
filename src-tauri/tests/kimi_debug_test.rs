/// Isolated test to debug Kimi JSON extraction issues
/// Run with: cargo test --test kimi_debug_test -- --nocapture

use std::path::Path;

fn extract_json_object(output: &str) -> String {
    let trimmed = output.trim();
    
    // First, try to find JSON within markdown code blocks
    // Look for ```json ... ``` or ``` ... ```
    let patterns = ["```json\n", "```json\r\n", "```JSON\n", "```\n", "```"];
    for pat in &patterns {
        if let Some(start) = trimmed.find(pat) {
            let after_open = start + pat.len();
            let rest = &trimmed[after_open..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                // Check if it looks like JSON
                if candidate.starts_with('{') || candidate.starts_with('[') {
                    return candidate.to_string();
                }
            }
        }
    }
    
    // Fallback: look for outermost JSON object/array
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }
    
    // Last resort: return trimmed output
    trimmed.to_string()
}

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_kimi_json_extraction() {
    // This simulates what happens when we call Kimi
    // We'll test with the real config files
    
    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let automation_dir = Path::new(project_path).join(".github/automation");
    
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .expect("Failed to read reddit_config.md");
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();
    
    println!("reddit_config.md: {} chars", reddit_config.len());
    println!("project_summary.md: {} chars", project_summary.len());
    println!("brandvoice.md: {} chars", brandvoice.len());
    
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
        {{\"product_name\": \"Days to Expiry\", \"mention_stance\": \"RECOMMENDED\", \"trigger_topics\": [\"topic1\"], \"query_keywords\": [\"topic1\"], \"seed_subreddits\": [\"subreddit1\"], \"excluded_subreddits\": []}}\n\n\
        Do NOT return placeholder text like \"<actual product name>\".\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );
    
    println!("\n=== Calling Kimi ===");
    println!("Prompt length: {} chars", prompt.len());
    
    // Call Kimi using the same method as the main code
    let output = pageseeds_lib::engine::agent::run_agent("kimi", &prompt, Path::new(project_path));
    
    match output {
        Ok(output) => {
            println!("\n=== Raw Output ({} chars) ===", output.len());
            println!("{}", output);
            
            let json_str = extract_json_object(&output);
            println!("\n=== Extracted JSON ({} chars) ===", json_str.len());
            println!("{}", json_str);
            
            // Try to parse as JSON
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(val) => {
                    println!("\n=== ✅ Valid JSON ===");
                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                }
                Err(e) => {
                    println!("\n=== ❌ Invalid JSON ===");
                    println!("Error: {}", e);
                    println!("\nFirst 500 chars of extracted text:");
                    println!("{}", &json_str[..json_str.len().min(500)]);
                }
            }
        }
        Err(e) => {
            panic!("Agent failed: {}", e);
        }
    }
}

#[test]
fn test_extraction_patterns() {
    // Test the extraction function with known inputs
    
    let test_cases = vec![
        ("Raw JSON", r#"{"key": "value"}"#),
        ("JSON in markdown", "```json\n{\"key\": \"value\"}\n```"),
        ("JSON with extra text", "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text"),
    ];
    
    for (name, input) in test_cases {
        let result = extract_json_object(input);
        println!("{}: input='{}' -> output='{}'", name, input.replace('\n', "\\n"), result);
        assert!(result.starts_with('{'), "{}: Should start with {{", name);
    }
}
