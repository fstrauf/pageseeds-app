/// Isolated test for Kimi CLI - no Tauri, no database, just call Kimi and see what happens

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Call Kimi exactly as the agent.rs module does
fn call_kimi(prompt: &str, project_path: &Path) -> Result<String, String> {
    let start = Instant::now();
    
    println!("[KIMI] Calling with prompt length: {} chars", prompt.len());
    println!("[KIMI] Working directory: {:?}", project_path);
    
    let mut cmd = Command::new("kimi");
    cmd.arg("--print")
       .arg("-p").arg(prompt)
       .arg("--output-format").arg("text")
       .arg("--final-message-only")
       .arg("--work-dir").arg(project_path)
       .stdin(Stdio::null())
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());
    
    println!("[KIMI] Command: {:?}", cmd);
    
    let output = cmd.output().map_err(|e| format!("Failed to execute: {}", e))?;
    
    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    println!("[KIMI] Finished in {:.1}s", elapsed.as_secs_f64());
    println!("[KIMI] Exit status: {:?}", output.status);
    println!("[KIMI] Stdout: {} bytes", stdout.len());
    println!("[KIMI] Stderr: {} bytes", stderr.len());
    
    if !stderr.is_empty() {
        println!("[KIMI] Stderr content:\n{}", &stderr[..stderr.len().min(500)]);
    }
    
    if output.status.success() || !stdout.trim().is_empty() {
        Ok(stdout.into_owned())
    } else {
        Err(format!("Kimi failed: {}", stderr))
    }
}

/// Extract JSON using the same logic as reddit.rs
fn extract_json_object(output: &str) -> Result<String, String> {
    let trimmed = output.trim();
    
    // Strategy 1: Markdown code block ```json ... ```
    for opener in ["```json\n", "```json\r\n", "```JSON\n"] {
        if let Some(start) = trimmed.find(opener) {
            let after_open = start + opener.len();
            let rest = &trimmed[after_open..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Ok(candidate.to_string());
                }
            }
        }
    }
    
    // Strategy 2: Plain ``` ... ```
    if let Some(start) = trimmed.find("```\n") {
        let after_open = start + 4;
        let rest = &trimmed[after_open..];
        if let Some(end) = rest.find("```") {
            let candidate = rest[..end].trim();
            if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                return Ok(candidate.to_string());
            }
        }
    }
    
    // Strategy 3: Raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                let candidate = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Ok(candidate.to_string());
                }
            }
        }
    }
    
    // Strategy 4: Raw JSON array
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                let candidate = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Ok(candidate.to_string());
                }
            }
        }
    }
    
    Err(format!("No valid JSON found. Output preview (500 chars): {}", &trimmed[..trimmed.len().min(500)]))
}

#[test]
#[ignore = "Requires Kimi CLI"]
fn test_kimi_simple_json_request() {
    println!("\n========================================");
    println!("TEST 1: Simple JSON Request");
    println!("========================================\n");
    
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    
    let prompt = r#"Return ONLY this JSON object, no other text:
{"product_name": "Test", "value": 123}"#;
    
    match call_kimi(prompt, project_path) {
        Ok(output) => {
            println!("\n✅ Kimi returned {} bytes", output.len());
            println!("\nRaw output (first 500 chars):");
            println!("{}", &output[..output.len().min(500)]);
            
            match extract_json_object(&output) {
                Ok(json) => {
                    println!("\n✅ Extracted JSON ({} chars):", json.len());
                    println!("{}", json);
                }
                Err(e) => {
                    println!("\n❌ JSON extraction failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("\n❌ Kimi call failed: {}", e);
        }
    }
}

#[test]
#[ignore = "Requires Kimi CLI"]
fn test_kimi_reddit_config_parse() {
    println!("\n========================================");
    println!("TEST 2: Reddit Config Parse (Real Prompt)");
    println!("========================================\n");
    
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let automation_dir = project_path.join(".github/automation");
    
    // Read real config files
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
    
    println!("Prompt length: {} chars", prompt.len());
    
    match call_kimi(&prompt, project_path) {
        Ok(output) => {
            println!("\n✅ Kimi returned {} bytes", output.len());
            
            // Save full output for inspection
            let temp_file = std::env::temp_dir().join("kimi_output_test.txt");
            std::fs::write(&temp_file, &output).expect("Failed to write temp file");
            println!("Full output saved to: {:?}", temp_file);
            
            println!("\nFirst 1000 chars of output:");
            println!("{}", &output[..output.len().min(1000)]);
            
            if output.len() > 1000 {
                println!("\n... ({} more chars)", output.len() - 1000);
            }
            
            match extract_json_object(&output) {
                Ok(json) => {
                    println!("\n✅ Extracted JSON ({} chars):", json.len());
                    println!("{}", &json[..json.len().min(1000)]);
                    
                    // Try to parse as RedditSearchParams
                    match serde_json::from_str::<serde_json::Value>(&json) {
                        Ok(parsed) => {
                            println!("\n✅ Valid JSON structure");
                            println!("   Keys: {:?}", parsed.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                        }
                        Err(e) => {
                            println!("\n❌ JSON parse error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("\n❌ JSON extraction failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("\n❌ Kimi call failed: {}", e);
        }
    }
}

#[test]
#[ignore = "Requires Kimi CLI"]
fn test_kimi_multiple_calls() {
    println!("\n========================================");
    println!("TEST 3: Multiple Calls (Context Check)");
    println!("========================================\n");
    
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    
    let prompt = r#"Return ONLY: {"test": "value", "call": CALL_NUMBER}"#;
    
    for i in 1..=3 {
        println!("\n--- Call {}/3 ---", i);
        let call_prompt = prompt.replace("CALL_NUMBER", &i.to_string());
        
        match call_kimi(&call_prompt, project_path) {
            Ok(output) => {
                println!("Output size: {} bytes", output.len());
                println!("First 200 chars: {}", &output[..output.len().min(200)]);
            }
            Err(e) => {
                println!("Failed: {}", e);
            }
        }
        
        // Small delay between calls
        std::thread::sleep(Duration::from_millis(500));
    }
}
