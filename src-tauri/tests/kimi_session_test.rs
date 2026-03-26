/// Test to reproduce Kimi session/context accumulation issue
/// 
/// This test calls Kimi multiple times in succession to check if
/// accumulated context causes inflated output sizes.

use std::path::Path;

#[test]
#[ignore = "Requires real Kimi API"]
fn test_kimi_multiple_calls_no_context_accumulation() {
    println!("\n========================================");
    println!("TEST: Kimi Multiple Calls - No Context Accumulation");
    println!("========================================\n");
    
    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    
    // Simple prompt that should return consistent size JSON
    let prompt = r#"Return ONLY this exact JSON object, no other text:
{"product_name": "Test Product", "mention_stance": "OPTIONAL", "trigger_topics": ["test"], "query_keywords": ["test"], "seed_subreddits": ["test"], "excluded_subreddits": []}"#;
    
    let mut output_sizes = vec![];
    
    for i in 1..=3 {
        println!("Call {}/3...", i);
        
        let output = pageseeds_lib::engine::agent::run_agent("kimi", prompt, Path::new(project_path))
            .expect(&format!("Kimi call {} should succeed", i));
        
        let size = output.len();
        output_sizes.push(size);
        
        println!("  Output size: {} bytes", size);
        println!("  Output preview: {}", &output[..output.len().min(100)]);
        
        // Validate it's valid JSON
        let json_str = pageseeds_lib::engine::exec::reddit::extract_json_object(&output)
            .expect(&format!("Should extract valid JSON from call {}", i));
        
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .expect(&format!("Should parse JSON from call {}", i));
        
        assert_eq!(
            parsed.get("product_name").and_then(|v| v.as_str()),
            Some("Test Product"),
            "Call {}: product_name should match", i
        );
        
        println!("  ✅ Valid JSON extracted and parsed\n");
    }
    
    // Check that sizes are consistent (within 50% of each other)
    let first_size = output_sizes[0] as f64;
    for (i, size) in output_sizes.iter().enumerate().skip(1) {
        let ratio = *size as f64 / first_size;
        println!("Call {} / Call 1 size ratio: {:.2}x", i + 1, ratio);
        
        // If ratio > 2.0, context is accumulating
        if ratio > 2.0 {
            panic!(
                "Output size grew significantly between calls ({} -> {} bytes). \
                This suggests context accumulation. \
                Call 1: {} bytes, Call {}: {} bytes",
                output_sizes[0], size, output_sizes[0], i + 1, size
            );
        }
    }
    
    println!("\n✅ All {} calls returned consistent-sized output", output_sizes.len());
    println!("Output sizes: {:?}", output_sizes);
}

#[test]
#[ignore = "Requires real Kimi API"]
fn test_kimi_reddit_config_parsing_multiple_times() {
    println!("\n========================================");
    println!("TEST: Kimi Reddit Config Parsing - Multiple Times");
    println!("========================================\n");
    
    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let automation_dir = std::path::Path::new(project_path).join(".github/automation");
    
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .expect("Failed to read reddit_config.md");
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();
    
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
        - query_keywords: array of strings\n\
        - seed_subreddits: array of strings (WITHOUT r/ prefix)\n\
        - excluded_subreddits: array of strings\n\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );
    
    let mut results = vec![];
    
    for i in 1..=3 {
        println!("Run {}/3...", i);
        let start = std::time::Instant::now();
        
        let output = pageseeds_lib::engine::agent::run_agent("kimi", &prompt, Path::new(project_path))
            .expect(&format!("Kimi call {} should succeed", i));
        
        let elapsed = start.elapsed();
        let size = output.len();
        
        println!("  Time: {:.1}s, Size: {} bytes", elapsed.as_secs_f64(), size);
        
        // Try to extract and parse
        match pageseeds_lib::engine::exec::reddit::extract_json_object(&output) {
            Ok(json_str) => {
                match serde_json::from_str::<serde_json::Value>(&json_str) {
                    Ok(parsed) => {
                        let topics = parsed.get("trigger_topics")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        println!("  ✅ Parsed: {} topics found", topics);
                        results.push((size, true, topics));
                    }
                    Err(e) => {
                        println!("  ❌ JSON parse error: {}", e);
                        results.push((size, false, 0));
                    }
                }
            }
            Err(e) => {
                println!("  ❌ Extraction error: {}", e);
                results.push((size, false, 0));
            }
        }
        
        println!();
    }
    
    // All should succeed
    let success_count = results.iter().filter(|(_, success, _)| *success).count();
    assert_eq!(success_count, 3, "All 3 runs should successfully parse");
    
    // Sizes should be consistent
    let sizes: Vec<_> = results.iter().map(|(s, _, _)| *s).collect();
    let avg_size = sizes.iter().sum::<usize>() as f64 / sizes.len() as f64;
    
    for (i, size) in sizes.iter().enumerate() {
        let deviation = (*size as f64 - avg_size).abs() / avg_size;
        println!("Run {}: {} bytes (deviation: {:.1}%)", i + 1, size, deviation * 100.0);
        
        // Allow up to 50% deviation (some variance is normal)
        if deviation > 0.5 {
            panic!("Run {} output size ({} bytes) deviates too much from average ({:.0} bytes)", 
                i + 1, size, avg_size);
        }
    }
    
    println!("\n✅ All {} runs succeeded with consistent output", results.len());
}
