//! Test Kimi with Tauri-like execution patterns

use std::path::Path;
use std::process::Command;

fn build_prompt() -> String {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let automation_dir = project_path.join(".github/automation");
    
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .expect("Failed to read reddit_config.md");
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();
    
    format!(
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
    )
}

fn run_agent_direct(prompt: &str, project_path: &Path) -> Result<String, String> {
    let session_id = format!("test-direct-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis());
    
    let mut cmd = Command::new("kimi");
    cmd.arg("--no-thinking")
       .arg("--print")
       .arg("-p").arg(prompt)
       .arg("--output-format").arg("text")
       .arg("--final-message-only")
       .arg("--session").arg(&session_id)
       .arg("--work-dir").arg(project_path);
    
    let output = cmd.output().map_err(|e| format!("Failed to execute: {}", e))?;
    
    if output.status.success() || !output.stdout.is_empty() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("Kimi failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

fn run_agent_in_thread(prompt: &str, project_path: &Path) -> Result<String, String> {
    use std::sync::mpsc;
    use std::time::Duration;
    
    let (tx, rx) = mpsc::channel();
    let prompt = prompt.to_string();
    let project_path = project_path.to_path_buf();
    
    std::thread::spawn(move || {
        let result = run_agent_direct(&prompt, &project_path);
        let _ = tx.send(result);
    });
    
    match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(result) => result,
        Err(_) => Err("Timeout".to_string()),
    }
}

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_direct_call() {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let prompt = build_prompt();
    
    println!("=== Direct Call Test ===");
    match run_agent_direct(&prompt, project_path) {
        Ok(output) => {
            println!("Output: {} bytes", output.len());
            if output.len() < 5000 {
                println!("✅ Looks like clean JSON!");
            } else {
                println!("⚠️ Large output - may have conversation history");
            }
        }
        Err(e) => println!("❌ Error: {}", e),
    }
}

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_thread_spawn() {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let prompt = build_prompt();
    
    println!("=== Thread Spawn Test (like Tauri) ===");
    match run_agent_in_thread(&prompt, project_path) {
        Ok(output) => {
            println!("Output: {} bytes", output.len());
            if output.len() < 5000 {
                println!("✅ Looks like clean JSON!");
            } else {
                println!("⚠️ Large output - may have conversation history");
                println!("\nFirst 500 chars:");
                println!("{}", &output[..output.len().min(500)]);
            }
        }
        Err(e) => println!("❌ Error: {}", e),
    }
}

#[test]
#[ignore = "Requires local Kimi CLI and project files"]
fn test_tokio_spawn_blocking() {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let prompt = build_prompt();
    
    println!("=== Tokio spawn_blocking Test ===");
    
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        let project_path = project_path.to_path_buf();
        let prompt = prompt.clone();
        
        tokio::task::spawn_blocking(move || {
            run_agent_direct(&prompt, &project_path)
        }).await.unwrap_or_else(|e| Err(format!("Join error: {:?}", e)))
    });
    
    match result {
        Ok(output) => {
            println!("Output: {} bytes", output.len());
            if output.len() < 5000 {
                println!("✅ Looks like clean JSON!");
            } else {
                println!("⚠️ Large output - may have conversation history");
            }
        }
        Err(e) => println!("❌ Error: {}", e),
    }
}
