//! Social media marketing models

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ═══════════════════════════════════════════════════════════════════════════════
// Enums
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Platform {
    #[serde(rename = "tiktok")]
    TikTok,
    InstagramFeed,
    InstagramReel,
    InstagramStory,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::TikTok => "tiktok",
            Platform::InstagramFeed => "instagram_feed",
            Platform::InstagramReel => "instagram_reel",
            Platform::InstagramStory => "instagram_story",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for Platform {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for Platform {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "tiktok" => Ok(Platform::TikTok),
            "instagram_feed" => Ok(Platform::InstagramFeed),
            "instagram_reel" => Ok(Platform::InstagramReel),
            "instagram_story" => Ok(Platform::InstagramStory),
            _ => Ok(Platform::InstagramFeed), // Default fallback
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PostStatus {
    Draft,
    Review,
    Approved,
    Scheduled,
    Posted,
    Failed,
}

impl PostStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostStatus::Draft => "draft",
            PostStatus::Review => "review",
            PostStatus::Approved => "approved",
            PostStatus::Scheduled => "scheduled",
            PostStatus::Posted => "posted",
            PostStatus::Failed => "failed",
        }
    }
}

impl std::fmt::Display for PostStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for PostStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for PostStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "draft" => Ok(PostStatus::Draft),
            "review" => Ok(PostStatus::Review),
            "approved" => Ok(PostStatus::Approved),
            "scheduled" => Ok(PostStatus::Scheduled),
            "posted" => Ok(PostStatus::Posted),
            "failed" => Ok(PostStatus::Failed),
            _ => Ok(PostStatus::Draft),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceType {
    Article,
    Screenshot,
    Spec,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceType::Article => "article",
            SourceType::Screenshot => "screenshot",
            SourceType::Spec => "spec",
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for SourceType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for SourceType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "article" => Ok(SourceType::Article),
            "screenshot" => Ok(SourceType::Screenshot),
            "spec" => Ok(SourceType::Spec),
            _ => Ok(SourceType::Article),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PostFormat {
    SingleImage,
    Carousel,
    VideoHook,
}

impl PostFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostFormat::SingleImage => "single_image",
            PostFormat::Carousel => "carousel",
            PostFormat::VideoHook => "video_hook",
        }
    }
}

impl std::fmt::Display for PostFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for PostFormat {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for PostFormat {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "single_image" => Ok(PostFormat::SingleImage),
            "carousel" => Ok(PostFormat::Carousel),
            "video_hook" => Ok(PostFormat::VideoHook),
            _ => Ok(PostFormat::SingleImage),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CampaignStatus {
    Draft,
    Generating,
    Active,
    Completed,
}

impl CampaignStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CampaignStatus::Draft => "draft",
            CampaignStatus::Generating => "generating",
            CampaignStatus::Active => "active",
            CampaignStatus::Completed => "completed",
        }
    }
}

impl std::fmt::Display for CampaignStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for CampaignStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for CampaignStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "draft" => Ok(CampaignStatus::Draft),
            "generating" => Ok(CampaignStatus::Generating),
            "active" => Ok(CampaignStatus::Active),
            "completed" => Ok(CampaignStatus::Completed),
            _ => Ok(CampaignStatus::Draft),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AssetType {
    Image,
    Video,
}

impl AssetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetType::Image => "image",
            AssetType::Video => "video",
        }
    }
}

impl rusqlite::types::ToSql for AssetType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for AssetType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "image" => Ok(AssetType::Image),
            "video" => Ok(AssetType::Video),
            _ => Ok(AssetType::Image),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CanvasSize {
    TikTok,   // 9:16 (1080x1920)
    Square,   // 1:1 (1080x1080)
    Portrait, // 4:5 (1080x1350)
    Story,    // 9:16 (1080x1920)
}

impl CanvasSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            CanvasSize::TikTok => "tiktok",
            CanvasSize::Square => "square",
            CanvasSize::Portrait => "portrait",
            CanvasSize::Story => "story",
        }
    }
}

impl rusqlite::types::ToSql for CanvasSize {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for CanvasSize {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "tiktok" => Ok(CanvasSize::TikTok),
            "square" => Ok(CanvasSize::Square),
            "portrait" => Ok(CanvasSize::Portrait),
            "story" => Ok(CanvasSize::Story),
            _ => Ok(CanvasSize::Square),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TextPosition {
    Top,
    Center,
    Bottom,
}

impl TextPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            TextPosition::Top => "top",
            TextPosition::Center => "center",
            TextPosition::Bottom => "bottom",
        }
    }
}

impl rusqlite::types::ToSql for TextPosition {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for TextPosition {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "top" => Ok(TextPosition::Top),
            "center" => Ok(TextPosition::Center),
            "bottom" => Ok(TextPosition::Bottom),
            _ => Ok(TextPosition::Center),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Structs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceConfig {
    pub include_articles: bool,
    pub article_slugs: Vec<String>,
    pub include_screenshots: bool,
    pub screenshot_dirs: Vec<String>,
    pub include_specs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisualAsset {
    pub path: String,
    pub asset_type: AssetType,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PostMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shares: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clicks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OverlayConfig {
    pub canvas_size: CanvasSize,
    pub font_family: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub text_position: TextPosition,
    pub max_text_length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TemplateExample {
    pub hook: String,
    pub caption: String,
    pub visual_description: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main Entity Models
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SocialCampaign {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source_config: SourceConfig,
    pub target_platforms: Vec<Platform>,
    pub template_ids: Vec<String>,
    pub status: CampaignStatus,
    pub post_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SocialPost {
    pub id: String,
    pub campaign_id: String,
    pub project_id: String,
    pub source_type: SourceType,
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub platform: Platform,
    pub format: PostFormat,
    pub hook: String,
    pub caption: String,
    pub hashtags: Vec<String>,
    pub cta: String,
    pub visual_assets: Vec<VisualAsset>,
    /// AI-generated prompt for external image generation (Midjourney, DALL-E, etc.)
    /// This is generated by the agent during post creation and can be used
    /// to create the visual asset that will have text overlaid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_generation_prompt: Option<String>,
    pub status: PostStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_post_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_post_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<PostMetrics>,
    pub template_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_prompt_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContentTemplate {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub platform: Platform,
    pub format: PostFormat,
    pub creation_prompt: String,
    pub overlay_config: OverlayConfig,
    pub default_hashtags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_output: Option<TemplateExample>,
    pub created_at: String,
    pub updated_at: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Request/Response Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CampaignStats {
    pub total_posts: u64,
    pub by_status: std::collections::HashMap<String, u64>,
    pub by_platform: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScheduleConfig {
    pub strategy: ScheduleStrategy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_hours: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScheduleStrategy {
    Immediate,
    Staggered,
    SpecificTimes,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateCampaignRequest {
    pub project_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source_config: SourceConfig,
    pub target_platforms: Vec<Platform>,
    pub template_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateTemplateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub name: String,
    pub platform: Platform,
    pub format: PostFormat,
    pub description: String,
}
