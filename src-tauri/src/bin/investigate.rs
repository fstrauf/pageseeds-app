/// CLI investigation runner.
///
/// Usage:
///   cargo run --bin investigate -- --project-path ~/code/daystoexpiry.com \
///     --project-id "abc123" "Why am I stuck at 10K impressions?"
///
/// This is the same exec_investigate function the Tauri app uses.
/// Outputs JSON results to stdout; markdown to .github/automation/investigations/
///
/// Requires a running Kimi bridge (default) or set PAGESEEDS_AGENT_PROVIDER.

use pageseeds_lib::{db, engine};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut project_path: Option<String> = None;
    let mut project_id: Option<String> = None;
    let mut agent_provider: String =
        std::env::var("PAGESEEDS_AGENT_PROVIDER").unwrap_or_else(|_| "kimi".to_string());
    let mut question: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--project-path" | "-p" => {
                i += 1;
                project_path = Some(args[i].clone());
            }
            "--project-id" | "-i" => {
                i += 1;
                project_id = Some(args[i].clone());
            }
            "--provider" => {
                i += 1;
                agent_provider = args[i].clone();
            }
            "--help" | "-h" => {
                eprintln!(
                    "investigate — PageSeeds agentic investigation CLI\n\n\
                     Usage: cargo run --bin investigate -- [OPTIONS] <QUESTION>\n\n\
                     Options:\n  \
                       --project-path, -p PATH   Path to target project repo (required)\n  \
                       --project-id, -i ID        Project ID in PageSeeds DB (required)\n  \
                       --provider PROVIDER        Agent provider: kimi, claude, openai, ollama (default: kimi)\n\n\
                     Environment:\n  \
                       PAGESEEDS_AGENT_PROVIDER   Default agent provider\n\n\
                     Examples:\n  \
                       cargo run --bin investigate -- -p ~/code/daystoexpiry.com -i abc123 \"Why am I stuck at 10K impressions?\"\n  \
                       PAGESEEDS_AGENT_PROVIDER=claude cargo run --bin investigate -- -p . -i local \"Analyze my site structure\""
                );
                std::process::exit(0);
            }
            arg if !arg.starts_with('-') => {
                question = Some(args[i..].join(" "));
                break;
            }
            _ => {
                eprintln!("Unknown flag: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let project_path = project_path.unwrap_or_else(|| {
        eprintln!("ERROR: --project-path is required. Use --help for usage.");
        std::process::exit(1);
    });
    let project_id = project_id.unwrap_or_else(|| {
        eprintln!("ERROR: --project-id is required. Use --help for usage.");
        std::process::exit(1);
    });
    let question = question.unwrap_or_else(|| {
        eprintln!("ERROR: question is required. Use --help for usage.");
        std::process::exit(1);
    });

    // Resolve project path (expand ~ to home dir)
    let project_path = if project_path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            project_path.replacen('~', &home, 1)
        } else {
            project_path
        }
    } else {
        project_path
    };

    eprintln!("🔍 Investigating: {question}");
    eprintln!("   Project: {project_path}");
    eprintln!("   Provider: {agent_provider}");

    let db_path = db::default_db_path();
    let db_path_str = db_path.to_string_lossy().to_string();

    // Run the investigation — same executor the Tauri app uses
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let result = rt.block_on(async {
        engine::exec::investigate::exec_investigate(
            &project_id,
            &project_path,
            &db_path_str,
            &question,
            &agent_provider,
        )
        .await
    });

    match result {
        Ok(json) => {
            let answer = json["answer"].as_str().unwrap_or("No answer produced.");
            let summary = json["summary"].as_str().unwrap_or("");
            let findings = json["findings"].as_array().map(|a| a.len()).unwrap_or(0);

            eprintln!("\n✅ Investigation complete: {findings} findings found\n");
            println!("{answer}");

            if !summary.is_empty() {
                eprintln!("\n📋 Summary: {summary}");
            }

            // Save evidence to automation dir (same as the Tauri app)
            let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(&project_path);
            let id = format!("inv-{}", chrono::Utc::now().timestamp_millis());
            let inv_dir = paths.automation_dir.join("investigations").join(&id);
            if let Err(e) = std::fs::create_dir_all(&inv_dir) {
                eprintln!("⚠ Failed to create dir: {e}");
            } else {
                let evidence_path = inv_dir.join("evidence.json");
                if let Err(e) =
                    std::fs::write(&evidence_path, serde_json::to_string_pretty(&json).unwrap_or_default())
                {
                    eprintln!("⚠ Failed to write evidence: {e}");
                } else {
                    eprintln!("📁 Evidence saved to {evidence_path}", evidence_path = evidence_path.display());
                }
                let answer_path = inv_dir.join("answer.md");
                let md = format!("# Investigation: {question}\n\n**Date:** {}\n\n## Answer\n\n{answer}\n",
                    chrono::Utc::now().to_rfc3339());
                let _ = std::fs::write(&answer_path, md);
            }

            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("\n❌ Investigation failed: {e}");
            std::process::exit(1);
        }
    }
}
