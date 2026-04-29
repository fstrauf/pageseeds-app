// Test to verify Kimi instrumentation captures output correctly
use std::path::PathBuf;

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_kimi_instrumentation() {
    // Initialize logging
    let _ = env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .try_init();

    let project_path = PathBuf::from("/Users/fstrauf/01_code/call-analyzer");
    let prompt = r#"Parse this reddit config file and extract key information:

```yaml
name: call-analyzer
reddit:
  enabled: true
  search_phrases:
    - "Call Analyzer"
    - "CallAnalyzer"
    - "Call-Analyzer"
  target_subreddits:
    - "RoastMyApp"
    - " startups"
    - "SaaS"
  trigger_topics:
    - "call analytics"
    - "sales call"
    - "call recording"
    - "conversation intelligence"
    - "gong alternative"
    - "chorus alternative"
  excluded_subreddits:
    - "politics"
    - "gaming"
    - "nsfw"
  post_tracking_limit: 100
  daily_post_limit: 5
  weekly_comment_limit: 20
  reply_persona: helpful
```

Return a JSON object with this structure:
{
  "project_name": "string",
  "reddit_config": {
    "enabled": boolean,
    "search_phrases": ["string"],
    "target_subreddits": ["string"],
    "trigger_topics": ["string"],
    "reply_persona": "string"
  },
  "validation": {
    "is_valid": boolean,
    "issues": ["string"]
  }
}"#;

    println!("Calling run_agent from instrumentation test...");

    let result = pageseeds_lib::engine::agent::run_agent("kimi", prompt, &project_path);

    println!(
        "Result: {:?}",
        result.as_ref().map(|s| s.len()).map_err(|e| e.clone())
    );

    match result {
        Ok(output) => {
            println!("Output length: {} bytes", output.len());
            println!("First 500 chars:\n{}", &output[..output.len().min(500)]);

            // Check if it's valid JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
                println!("✓ Valid JSON output!");
                println!("JSON preview: {:?}", json.get("project_name"));
            } else {
                println!("✗ NOT valid JSON");
            }
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    // Check if debug file was created
    let debug_files: Vec<_> = std::fs::read_dir("/tmp")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("kimi_output_"))
        .collect();

    println!("\nDebug files found: {}", debug_files.len());
    for f in &debug_files {
        let meta = std::fs::metadata(f.path()).unwrap();
        println!(
            "  {} ({} bytes)",
            f.file_name().to_string_lossy(),
            meta.len()
        );
    }
}
