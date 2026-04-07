/// End-to-end test for Reddit search flow
/// 
/// Tests the complete pipeline:
/// 1. Parse reddit_config.md with Kimi agent
/// 2. Extract search parameters (topics, subreddits)
/// 3. Search Reddit API
/// 4. Verify we get results
///
/// Run with:
///   cargo run --example test_reddit_full_flow -- <project_path>
///
/// Example:
///   cargo run --example test_reddit_full_flow -- /Users/fstrauf/01_code/call-analyzer

use std::path::Path;
use std::collections::HashSet;

#[derive(Debug)]
struct SearchParams {
    product_name: String,
    trigger_topics: Vec<String>,
    seed_subreddits: Vec<String>,
}

#[tokio::main]
async fn main() {
    let project_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/Users/fstrauf/01_code/call-analyzer".to_string());
    
    let project_path = Path::new(&project_path);
    let automation_dir = project_path.join(".github").join("automation");
    
    println!("========================================");
    println!("Reddit Full Flow Test");
    println!("Project: {}", project_path.display());
    println!("========================================\n");
    
    // Step 1: Read config files
    println!("[Step 1] Reading config files...");
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .unwrap_or_default();
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();
    
    if reddit_config.is_empty() {
        println!("❌ ERROR: reddit_config.md not found or empty");
        std::process::exit(1);
    }
    println!("✅ Config files loaded (reddit_config.md: {} chars)", reddit_config.len());
    
    // Step 2: Call Kimi to parse config
    println!("\n[Step 2] Parsing config with Kimi agent...");
    
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
        Do NOT return placeholder text.\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );
    
    let kimi_output = std::process::Command::new("kimi")
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
    
    let params = match kimi_output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            
            // Extract JSON from Kimi's structured output
            let trimmed = stdout.trim();
            let json_str = if let Some(last_brace) = trimmed.rfind('}') {
                let before_end = &trimmed[..=last_brace];
                if let Some(start) = before_end.rfind("{\n  \"product_name\"") {
                    Some(&trimmed[start..=last_brace])
                } else if let Some(start) = before_end.rfind('{') {
                    Some(&trimmed[start..=last_brace])
                } else {
                    None
                }
            } else {
                None
            };
            
            match json_str {
                Some(json) => {
                    // With --final-message-only, we get clean JSON directly
                    match serde_json::from_str::<serde_json::Value>(json) {
                        Ok(val) => {
                            let product_name = val.get("product_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            
                            // Helper to get array field with name variations
                            fn get_string_array(val: &serde_json::Value, field: &str) -> Vec<String> {
                                let variations = [
                                    field.to_string(),
                                    format!(" {}", field),
                                    format!("{} ", field),
                                ];
                                for var in variations.iter() {
                                    if let Some(arr) = val.get(var).and_then(|v| v.as_array()) {
                                        return arr.iter()
                                            .filter_map(|v| v.as_str())
                                            .map(|s| s.to_string())
                                            .collect();
                                    }
                                }
                                vec![]
                            }
                            
                            let trigger_topics = get_string_array(&val, "trigger_topics");
                            let seed_subreddits = get_string_array(&val, "seed_subreddits");
                            
                            println!("✅ Config parsed successfully");
                            println!("   Product: {}", product_name);
                            println!("   Topics: {}", trigger_topics.len());
                            println!("   Subreddits: {}", seed_subreddits.len());
                            
                            if trigger_topics.is_empty() || seed_subreddits.is_empty() {
                                println!("❌ ERROR: No topics or subreddits extracted");
                                std::process::exit(1);
                            }
                            
                            SearchParams {
                                product_name,
                                trigger_topics,
                                seed_subreddits,
                            }
                        }
                        Err(e) => {
                            println!("❌ ERROR: Failed to parse Kimi JSON: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    println!("❌ ERROR: No JSON found in Kimi output");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            println!("❌ ERROR: Failed to run Kimi: {}", e);
            std::process::exit(1);
        }
    };
    
    // Step 3: Search Reddit
    println!("\n[Step 3] Searching Reddit...");
    
    // Build search pairs (subreddit + query combinations)
    // Limit to first 3 topics and 3 subreddits to avoid rate limits in test
    let topics: Vec<_> = params.trigger_topics.iter().take(3).collect();
    let subreddits: Vec<_> = params.seed_subreddits.iter().take(3).collect();
    
    println!("   Using top {} topics x {} subreddits = {} searches", 
        topics.len(), subreddits.len(), topics.len() * subreddits.len());
    
    let mut all_posts: Vec<serde_json::Value> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut errors = 0;
    
    for subreddit in &subreddits {
        for query in &topics {
            print!("   Searching r/{} for '{}'... ", subreddit, query);
            
            match search_reddit(query, subreddit, 5).await {
                Ok(posts) => {
                    let new_posts: Vec<_> = posts.into_iter()
                        .filter(|p| {
                            let id = p.get("post_id").and_then(|v| v.as_str()).unwrap_or("");
                            if id.is_empty() || seen_ids.contains(id) {
                                false
                            } else {
                                seen_ids.insert(id.to_string());
                                true
                            }
                        })
                        .collect();
                    
                    println!("{} new posts", new_posts.len());
                    all_posts.extend(new_posts);
                }
                Err(e) => {
                    println!("ERROR: {}", e);
                    errors += 1;
                }
            }
            
            // Small delay to be nice to Reddit
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    
    // Step 4: Verify results
    println!("\n[Step 4] Results Summary");
    println!("   Total unique posts: {}", all_posts.len());
    println!("   Search errors: {}", errors);
    
    if all_posts.is_empty() {
        println!("\n⚠️  WARNING: No posts found");
        println!("   This could mean:");
        println!("   - The subreddits don't have recent posts matching your topics");
        println!("   - Reddit rate limited the requests");
        println!("   - The search queries need adjustment");
    } else {
        println!("\n✅ SUCCESS: Found {} posts!", all_posts.len());
        
        // Show sample posts
        println!("\n   Sample posts:");
        for (i, post) in all_posts.iter().take(3).enumerate() {
            let title = post.get("title").and_then(|v| v.as_str()).unwrap_or("(no title)");
            let subreddit = post.get("subreddit").and_then(|v| v.as_str()).unwrap_or("(unknown)");
            println!("   {}. [{}] {}", i + 1, subreddit, &title[..title.len().min(60)]);
        }
    }
    
    // Step 5: Final status
    println!("\n========================================");
    if errors == 0 && !all_posts.is_empty() {
        println!("✅ FULL FLOW TEST PASSED");
        println!("   Config parsing: OK");
        println!("   Reddit search: OK");
        println!("   Results: {} posts", all_posts.len());
    } else if !all_posts.is_empty() {
        println!("⚠️  FULL FLOW TEST PASSED WITH WARNINGS");
        println!("   Config parsing: OK");
        println!("   Reddit search: {} errors", errors);
        println!("   Results: {} posts", all_posts.len());
    } else {
        println!("❌ FULL FLOW TEST FAILED");
        println!("   No posts found");
        std::process::exit(1);
    }
    println!("========================================");
}

async fn search_reddit(query: &str, subreddit: &str, limit: i32) -> Result<Vec<serde_json::Value>, String> {
    // For the test, we'll use a simple HTTP request directly
    // In production, this would use the app's Reddit client
    
    let base = format!("https://www.reddit.com/r/{}/search.json", subreddit);
    
    let client = reqwest::Client::builder()
        .user_agent("PageSeeds/1.0 (test; contact pageseeds.com)")
        .build()
        .map_err(|e| e.to_string())?;
    
    let resp = client
        .get(&base)
        .query(&[
            ("q", query),
            ("limit", &limit.to_string()),
            ("sort", "relevance"),
            ("t", "week"),
            ("type", "link"),
            ("restrict_sr", "1"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    
    // Parse Reddit response
    let posts = json
        .get("data")
        .and_then(|d| d.get("children"))
        .and_then(|c| c.as_array())
        .map(|children| {
            children.iter()
                .filter_map(|child| {
                    let data = child.get("data")?;
                    Some(serde_json::json!({
                        "post_id": data.get("id")?.as_str()?,
                        "title": data.get("title")?.as_str()?,
                        "subreddit": data.get("subreddit")?.as_str()?,
                        "author": data.get("author")?.as_str()?,
                        "upvotes": data.get("score")?.as_i64()?,
                        "comment_count": data.get("num_comments")?.as_i64()?,
                        "url": data.get("permalink")?.as_str()?,
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    
    Ok(posts)
}
