//! Database operations for social media marketing

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::social::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Campaign Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new social campaign
pub fn create_campaign(conn: &Connection, campaign: &SocialCampaign) -> Result<()> {
    let _now = Utc::now().to_rfc3339();
    
    conn.execute(
        r#"INSERT INTO social_campaigns (
            id, project_id, name, description, source_config, target_platforms,
            template_ids, status, post_count, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
        params![
            campaign.id,
            campaign.project_id,
            campaign.name,
            campaign.description,
            serde_json::to_string(&campaign.source_config)?,
            serde_json::to_string(&campaign.target_platforms)?,
            serde_json::to_string(&campaign.template_ids)?,
            campaign.status.as_str(),
            campaign.post_count,
            campaign.created_at,
            campaign.updated_at,
        ],
    )?;
    
    Ok(())
}

/// Get a campaign by ID
pub fn get_campaign(conn: &Connection, campaign_id: &str) -> Result<Option<SocialCampaign>> {
    let mut stmt = conn.prepare(
        r#"SELECT 
            id, project_id, name, description, source_config, target_platforms,
            template_ids, status, post_count, created_at, updated_at
        FROM social_campaigns WHERE id = ?1"#,
    )?;
    
    let campaign = stmt
        .query_row(params![campaign_id], row_to_campaign)
        .optional()?;
    
    Ok(campaign)
}

/// List all campaigns for a project
pub fn list_campaigns(conn: &Connection, project_id: &str) -> Result<Vec<SocialCampaign>> {
    let mut stmt = conn.prepare(
        r#"SELECT 
            id, project_id, name, description, source_config, target_platforms,
            template_ids, status, post_count, created_at, updated_at
        FROM social_campaigns 
        WHERE project_id = ?1
        ORDER BY created_at DESC"#,
    )?;
    
    let campaigns = stmt
        .query_map(params![project_id], row_to_campaign)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    
    Ok(campaigns)
}

/// Update campaign status
pub fn update_campaign_status(
    conn: &Connection,
    campaign_id: &str,
    status: CampaignStatus,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        "UPDATE social_campaigns SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), now, campaign_id],
    )?;
    
    Ok(())
}

/// Update campaign post count
pub fn update_campaign_post_count(
    conn: &Connection,
    campaign_id: &str,
    count: u32,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        "UPDATE social_campaigns SET post_count = ?1, updated_at = ?2 WHERE id = ?3",
        params![count, now, campaign_id],
    )?;
    
    Ok(())
}

/// Delete a campaign (cascades to posts via FK)
pub fn delete_campaign(conn: &Connection, campaign_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM social_campaigns WHERE id = ?1",
        params![campaign_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Post Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new social post
pub fn create_post(conn: &Connection, post: &SocialPost) -> Result<()> {
    conn.execute(
        r#"INSERT INTO social_posts (
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)"#,
        params![
            post.id,
            post.campaign_id,
            post.project_id,
            post.source_type.as_str(),
            post.source_id,
            post.source_url,
            post.platform.as_str(),
            post.format.as_str(),
            post.hook,
            post.caption,
            serde_json::to_string(&post.hashtags)?,
            post.cta,
            serde_json::to_string(&post.visual_assets)?,
            post.image_generation_prompt,
            post.status.as_str(),
            post.scheduled_at,
            post.posted_at,
            post.platform_post_id,
            post.platform_post_url,
            serde_json::to_string(&post.metrics)?,
            post.template_id,
            post.generated_by,
            post.generation_prompt_hash,
            post.created_at,
            post.updated_at,
        ],
    )?;
    
    Ok(())
}

/// Get a post by ID
pub fn get_post(conn: &Connection, post_id: &str) -> Result<Option<SocialPost>> {
    let mut stmt = conn.prepare(
        r#"SELECT 
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        FROM social_posts WHERE id = ?1"#,
    )?;
    
    let post = stmt.query_row(params![post_id], row_to_post).optional()?;
    Ok(post)
}

/// Get all posts for a campaign
pub fn get_posts_by_campaign(
    conn: &Connection,
    campaign_id: &str,
    status: Option<PostStatus>,
) -> Result<Vec<SocialPost>> {
    let sql = if let Some(ref _s) = status {
        r#"SELECT 
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        FROM social_posts 
        WHERE campaign_id = ?1 AND status = ?2
        ORDER BY created_at DESC"#
    } else {
        r#"SELECT 
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        FROM social_posts 
        WHERE campaign_id = ?1
        ORDER BY created_at DESC"#
    };
    
    let mut stmt = conn.prepare(sql)?;
    
    let posts = if let Some(s) = status {
        stmt.query_map(params![campaign_id, s.as_str()], row_to_post)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(params![campaign_id], row_to_post)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    
    Ok(posts)
}

/// Get posts by project
pub fn get_posts_by_project(
    conn: &Connection,
    project_id: &str,
    status: Option<PostStatus>,
) -> Result<Vec<SocialPost>> {
    let sql = if let Some(ref _s) = status {
        r#"SELECT 
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        FROM social_posts 
        WHERE project_id = ?1 AND status = ?2
        ORDER BY created_at DESC"#
    } else {
        r#"SELECT 
            id, campaign_id, project_id, source_type, source_id, source_url,
            platform, format, hook, caption, hashtags, cta, visual_assets,
            image_generation_prompt, status, scheduled_at, posted_at, 
            platform_post_id, platform_post_url, metrics, template_id, 
            generated_by, generation_prompt_hash, created_at, updated_at
        FROM social_posts 
        WHERE project_id = ?1
        ORDER BY created_at DESC"#
    };
    
    let mut stmt = conn.prepare(sql)?;
    
    let posts = if let Some(s) = status {
        stmt.query_map(params![project_id, s.as_str()], row_to_post)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(params![project_id], row_to_post)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    
    Ok(posts)
}

/// Update post status
pub fn update_post_status(
    conn: &Connection,
    post_id: &str,
    status: PostStatus,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        "UPDATE social_posts SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), now, post_id],
    )?;
    
    Ok(())
}

/// Schedule a post
pub fn schedule_post(
    conn: &Connection,
    post_id: &str,
    scheduled_at: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        "UPDATE social_posts SET scheduled_at = ?1, status = 'scheduled', updated_at = ?2 WHERE id = ?3",
        params![scheduled_at, now, post_id],
    )?;
    
    Ok(())
}

/// Mark a post as posted
pub fn mark_posted(
    conn: &Connection,
    post_id: &str,
    platform_url: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        r#"UPDATE social_posts SET 
            status = 'posted', 
            posted_at = ?1, 
            platform_post_url = ?2,
            updated_at = ?3 
        WHERE id = ?4"#,
        params![now, platform_url, now, post_id],
    )?;
    
    Ok(())
}

/// Update post content
pub fn update_post(conn: &Connection, post: &SocialPost) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    
    conn.execute(
        r#"UPDATE social_posts SET 
            hook = ?1,
            caption = ?2,
            hashtags = ?3,
            cta = ?4,
            visual_assets = ?5,
            image_generation_prompt = ?6,
            updated_at = ?7
        WHERE id = ?8"#,
        params![
            post.hook,
            post.caption,
            serde_json::to_string(&post.hashtags)?,
            post.cta,
            serde_json::to_string(&post.visual_assets)?,
            post.image_generation_prompt,
            now,
            post.id,
        ],
    )?;
    
    Ok(())
}

/// Delete a post
pub fn delete_post(conn: &Connection, post_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM social_posts WHERE id = ?1",
        params![post_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Template Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new content template
pub fn create_template(conn: &Connection, template: &ContentTemplate) -> Result<()> {
    conn.execute(
        r#"INSERT INTO social_templates (
            id, project_id, name, description, platform, format,
            creation_prompt, overlay_config, default_hashtags, example_output,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
        params![
            template.id,
            template.project_id,
            template.name,
            template.description,
            template.platform.as_str(),
            template.format.as_str(),
            template.creation_prompt,
            serde_json::to_string(&template.overlay_config)?,
            serde_json::to_string(&template.default_hashtags)?,
            serde_json::to_string(&template.example_output)?,
            template.created_at,
            template.updated_at,
        ],
    )?;
    
    Ok(())
}

/// Get a template by ID
pub fn get_template(conn: &Connection, template_id: &str) -> Result<Option<ContentTemplate>> {
    let mut stmt = conn.prepare(
        r#"SELECT 
            id, project_id, name, description, platform, format,
            creation_prompt, overlay_config, default_hashtags, example_output,
            created_at, updated_at
        FROM social_templates WHERE id = ?1"#,
    )?;
    
    let template = stmt
        .query_row(params![template_id], row_to_template)
        .optional()?;
    
    Ok(template)
}

/// List templates (global + project-specific)
pub fn list_templates(
    conn: &Connection,
    project_id: Option<&str>,
) -> Result<Vec<ContentTemplate>> {
    // Get global templates (project_id IS NULL)
    let mut global_stmt = conn.prepare(
        r#"SELECT 
            id, project_id, name, description, platform, format,
            creation_prompt, overlay_config, default_hashtags, example_output,
            created_at, updated_at
        FROM social_templates WHERE project_id IS NULL
        ORDER BY created_at DESC"#,
    )?;
    
    let mut templates: Vec<ContentTemplate> = global_stmt
        .query_map([], row_to_template)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    
    // Get project-specific templates if project_id provided
    if let Some(pid) = project_id {
        let mut project_stmt = conn.prepare(
            r#"SELECT 
                id, project_id, name, description, platform, format,
                creation_prompt, overlay_config, default_hashtags, example_output,
                created_at, updated_at
            FROM social_templates WHERE project_id = ?1
            ORDER BY created_at DESC"#,
        )?;
        
        let project_templates: Vec<ContentTemplate> = project_stmt
            .query_map(params![pid], row_to_template)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        
        templates.extend(project_templates);
    }
    
    Ok(templates)
}

/// Delete a template
pub fn delete_template(conn: &Connection, template_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM social_templates WHERE id = ?1",
        params![template_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Statistics
// ═══════════════════════════════════════════════════════════════════════════════

/// Get campaign statistics
pub fn get_campaign_stats(conn: &Connection, campaign_id: &str) -> Result<CampaignStats> {
    let total: u64 = conn.query_row(
        "SELECT COUNT(*) FROM social_posts WHERE campaign_id = ?1",
        params![campaign_id],
        |row| row.get(0),
    )?;
    
    let mut by_status = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM social_posts WHERE campaign_id = ?1 GROUP BY status",
        )?;
        let rows = stmt.query_map(params![campaign_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            by_status.insert(status, count);
        }
    }
    
    let mut by_platform = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT platform, COUNT(*) FROM social_posts WHERE campaign_id = ?1 GROUP BY platform",
        )?;
        let rows = stmt.query_map(params![campaign_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;
        for row in rows {
            let (platform, count) = row?;
            by_platform.insert(platform, count);
        }
    }
    
    Ok(CampaignStats {
        total_posts: total,
        by_status,
        by_platform,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Row Mappers
// ═══════════════════════════════════════════════════════════════════════════════

fn row_to_campaign(row: &rusqlite::Row<'_>) -> rusqlite::Result<SocialCampaign> {
    let source_config_json: String = row.get("source_config")?;
    let source_config: SourceConfig = serde_json::from_str(&source_config_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let target_platforms_json: String = row.get("target_platforms")?;
    let target_platforms: Vec<Platform> = serde_json::from_str(&target_platforms_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let template_ids_json: String = row.get("template_ids")?;
    let template_ids: Vec<String> = serde_json::from_str(&template_ids_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    Ok(SocialCampaign {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        source_config,
        target_platforms,
        template_ids,
        status: row.get("status")?,
        post_count: row.get::<_, i64>("post_count")? as u32,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_post(row: &rusqlite::Row<'_>) -> rusqlite::Result<SocialPost> {
    let hashtags_json: String = row.get("hashtags")?;
    let hashtags: Vec<String> = serde_json::from_str(&hashtags_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let visual_assets_json: String = row.get("visual_assets")?;
    let visual_assets: Vec<VisualAsset> = serde_json::from_str(&visual_assets_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let metrics_json: Option<String> = row.get("metrics")?;
    let metrics: Option<PostMetrics> = metrics_json
        .and_then(|s| serde_json::from_str(&s).ok());
    
    Ok(SocialPost {
        id: row.get("id")?,
        campaign_id: row.get("campaign_id")?,
        project_id: row.get("project_id")?,
        source_type: row.get("source_type")?,
        source_id: row.get("source_id")?,
        source_url: row.get("source_url")?,
        platform: row.get("platform")?,
        format: row.get("format")?,
        hook: row.get("hook")?,
        caption: row.get("caption")?,
        hashtags,
        cta: row.get("cta")?,
        visual_assets,
        image_generation_prompt: row.get("image_generation_prompt")?,
        status: row.get("status")?,
        scheduled_at: row.get("scheduled_at")?,
        posted_at: row.get("posted_at")?,
        platform_post_id: row.get("platform_post_id")?,
        platform_post_url: row.get("platform_post_url")?,
        metrics,
        template_id: row.get("template_id")?,
        generated_by: row.get("generated_by")?,
        generation_prompt_hash: row.get("generation_prompt_hash")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContentTemplate> {
    let overlay_config_json: String = row.get("overlay_config")?;
    let overlay_config: OverlayConfig = serde_json::from_str(&overlay_config_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let default_hashtags_json: String = row.get("default_hashtags")?;
    let default_hashtags: Vec<String> = serde_json::from_str(&default_hashtags_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;
    
    let example_output_json: Option<String> = row.get("example_output")?;
    let example_output: Option<TemplateExample> = example_output_json
        .and_then(|s| serde_json::from_str(&s).ok());
    
    Ok(ContentTemplate {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        platform: row.get("platform")?,
        format: row.get("format")?,
        creation_prompt: row.get("creation_prompt")?,
        overlay_config,
        default_hashtags,
        example_output,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
