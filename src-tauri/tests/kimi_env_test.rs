//! Test Kimi with different environment scenarios

use std::path::Path;
use std::process::Command;

fn run_with_env(env_clear: bool, env_vars: &[(&str, &str)]) -> Result<String, String> {
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let prompt = r#"Return ONLY: {"test": "value"}"#;
    
    let mut cmd = Command::new("kimi");
    cmd.arg("--no-thinking")
       .arg("--print")
       .arg("-p").arg(prompt)
       .arg("--output-format").arg("text")
       .arg("--final-message-only")
       .arg("--session").arg(format!("env-test-{}", std::time::SystemTime::now()
           .duration_since(std::time::UNIX_EPOCH)
           .unwrap_or_default()
           .as_millis()))
       .arg("--work-dir").arg(project_path);
    
    if env_clear {
        cmd.env_clear();
    }
    
    for (key, value) in env_vars {
        cmd.env(key, value);
    }
    
    let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn test_normal_env() {
    println!("=== Normal Environment ===");
    match run_with_env(false, &[]) {
        Ok(out) => println!("Output: {} bytes - {}", out.len(), 
            if out.len() < 100 { "✅ clean" } else { "⚠️ large" }),
        Err(e) => println!("❌ {}", e),
    }
}

#[test]
fn test_cleared_env() {
    println!("=== Cleared Environment ===");
    match run_with_env(true, &[
        ("PATH", "/usr/bin:/bin:/usr/local/bin"),
        ("HOME", "/Users/fstrauf"),
    ]) {
        Ok(out) => println!("Output: {} bytes - {}", out.len(),
            if out.len() < 100 { "✅ clean" } else { "⚠️ large" }),
        Err(e) => println!("❌ {}", e),
    }
}

#[test]
fn test_with_kimi_vars() {
    println!("=== With KIMI_* env vars set ===");
    match run_with_env(false, &[
        ("KIMI_SESSION", "existing-session"),
        ("KIMI_CONFIG", "/some/path"),
    ]) {
        Ok(out) => println!("Output: {} bytes - {}", out.len(),
            if out.len() < 100 { "✅ clean" } else { "⚠️ large" }),
        Err(e) => println!("❌ {}", e),
    }
}

#[test]
fn test_current_dir_set() {
    use std::path::Path;
    
    let project_path = Path::new("/Users/fstrauf/01_code/call-analyzer");
    let prompt = r#"Return ONLY: {"test": "value"}"#;
    
    println!("=== With current_dir set ===");
    
    let mut cmd = Command::new("kimi");
    cmd.arg("--no-thinking")
       .arg("--print")
       .arg("-p").arg(prompt)
       .arg("--output-format").arg("text")
       .arg("--final-message-only")
       .arg("--session").arg("dir-test-123")
       .current_dir(project_path);  // Note: using current_dir instead of --work-dir
    
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            println!("Output: {} bytes - {}", stdout.len(),
                if stdout.len() < 100 { "✅ clean" } else { "⚠️ large" });
        }
        Err(e) => println!("❌ {}", e),
    }
}
