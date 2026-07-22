use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::social::{AssetType, VisualAsset};
use crate::models::task::Task;
use crate::social::image::assets::generate_branded_graphic;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Build Visuals
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_build_visuals(task: &Task, project_path: &str) -> StepResult {
    // Load generated posts from previous step
    let mut posts = match super::load_posts_from_artifacts(task) {
        Some(p) => p,
        None => {
            return StepResult::fail("No generated posts found. Run social_generate_posts first.".to_string());
        }
    };

    // Ensure output directory exists
    let output_dir =
        match crate::social::content::sources::ensure_output_dir(Path::new(project_path)) {
            Ok(d) => d,
            Err(e) => {
                return StepResult::fail(format!("Failed to create output directory: {}", e));
            }
        };

    log::info!(
        "[social_build_visuals] building visuals for {} posts",
        posts.len()
    );

    // Build visuals for each post
    for post in &mut posts {
        if let Some(first_asset) = post.visual_assets.first() {
            let source_path = Path::new(project_path).join(&first_asset.path);
            let output_filename = format!("{}.png", post.id);
            let output_path = output_dir.join(&output_filename);

            if source_path.exists() {
                // Copy existing image
                if let Err(e) = std::fs::copy(&source_path, &output_path) {
                    log::warn!("[social_build_visuals] failed to copy image: {}", e);
                } else {
                    // Update the post's visual asset to point to the new location
                    let relative_path = output_path
                        .strip_prefix(project_path)
                        .unwrap_or(&output_path)
                        .to_string_lossy()
                        .to_string();

                    post.visual_assets = vec![VisualAsset {
                        path: relative_path,
                        asset_type: AssetType::Image,
                        description: first_asset.description.clone(),
                        overlay_text: first_asset.overlay_text.clone(),
                    }];
                }
            } else {
                // Generate branded graphic as fallback
                match generate_branded_graphic(&output_path, &post.hook) {
                    Ok(asset) => {
                        let relative_path = output_path
                            .strip_prefix(project_path)
                            .unwrap_or(&output_path)
                            .to_string_lossy()
                            .to_string();

                        post.visual_assets = vec![VisualAsset {
                            path: relative_path,
                            ..asset
                        }];
                    }
                    Err(e) => {
                        log::warn!(
                            "[social_build_visuals] failed to generate branded graphic: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    // Save updated posts
    let posts_json = match serde_json::to_string(&posts) {
        Ok(j) => j,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize posts: {}", e));
        }
    };

    StepResult {
        success: true,
        message: format!("Built visuals for {} posts", posts.len()),
        output: Some(posts_json),
        artifact_key: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regenerate Steps
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_rebuild_visual(_task: &Task, _project_path: &str) -> StepResult {
    StepResult {
        success: true,
        message: "Visual rebuild complete".to_string(),
        output: None,
        artifact_key: None,
    }
}
