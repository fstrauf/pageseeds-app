export type TaskStatus = 'todo' | 'in_progress' | 'review' | 'done' | 'cancelled';
export type ExecutionMode = 'automatic' | 'batchable' | 'manual' | 'spec';
export type AgentPolicy = 'none' | 'required' | 'optional';
export type Priority = 'high' | 'medium' | 'low';

export interface TaskArtifact {
  key: string;
  path?: string;
  type?: string;
  source?: string;
  content?: string;
}

export interface KeywordDifficultyEntry {
  keyword: string;
  difficulty: number | string | null;
  volume?: number | string | null;
  traffic?: number | string | null;
  topVolume?: number | string | null;
  shortage?: number | string | null;
  /** True when Ahrefs returned real data; false/absent when the keyword had no index data. */
  has_data?: boolean;
  serp_count?: number;
  top_result?: string;
}

export interface KeywordResearchResult {
  themes?: string[];
  total_candidates?: number;
  new_keywords: string[];
  filtered_out?: number;
  /** Batch difficulty data — either a dict with a results array, or a list directly */
  difficulty?: {
    results?: KeywordDifficultyEntry[];
    total?: number;
    successful?: number;
  } | KeywordDifficultyEntry[] | null;
  difficulty_analyzed_count?: number;
  difficulty_skipped_keywords?: string[];
}

// ─── Keyword Coverage Analysis ────────────────────────────────────────────────

export interface KeywordCoverageCluster {
  cluster_id: string;
  cluster_name: string;
  article_ids: number[];
  primary_keywords: string[];
  article_count: number;
}

export interface KeywordCoverage {
  generated_at: string;
  article_count: number;
  clusters: KeywordCoverageCluster[];
}

export interface KeywordCoverageStatus {
  exists: boolean;
  last_analyzed: string;
  coverage?: KeywordCoverage;
}

export interface TaskRun {
  attempts: number;
  last_error?: string;
  provider?: string;
}

export interface Task {
  id: string;
  type: string;
  task_type?: string;
  phase?: string;
  status: TaskStatus;
  priority?: Priority;
  execution_mode: ExecutionMode;
  agent_policy: AgentPolicy;
  title?: string;
  description?: string;
  article_slug?: string;
  project_id: string;
  depends_on: string[];
  artifacts: TaskArtifact[];
  run: TaskRun;
  created_at: string;
  updated_at: string;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  content_dir?: string;
  site_url?: string;
  site_id?: string;
  active: boolean;
  agent_provider?: string;
}

export interface Article {
  id: number;
  title: string;
  url_slug: string;
  file: string;
  target_keyword?: string;
  keyword_difficulty?: string;
  target_volume: number;
  published_date?: string;
  word_count: number;
  status: string;
  content_gaps_addressed: string[];
  estimated_traffic_monthly?: string;
  project_id: string;
}

export interface SecretStatus {
  key: string;
  description: string;
  configured: boolean;
  source?: string;
}

export interface SecretsStatus {
  secrets: SecretStatus[];
  secrets_file_exists: boolean;
  secrets_file_path: string;
}

export interface ImportResult {
  tasks_imported: number;
  articles_imported: number;
}

// View type moved to Phase 6 section below to include workflow/batch/scheduler/history views.

// ─── Content / SEO ───────────────────────────────────────────────────────────────

export interface ContentDirResolution {
  selected?: string;
  source: string; // "configured" | "auto" | "none"
  has_markdown: boolean;
  candidates_searched: string[];
}

export interface CleaningIssue {
  file: string;
  issue_type: string;
  description: string;
  fixed: boolean;
}

export interface CleaningResult {
  files_checked: number;
  issues: CleaningIssue[];
  issues_fixed: number;
}

export interface DateIssue {
  article_id: number;
  issue_type: string;
  description: string;
  current_date: string;
}

export interface DateAnalysis {
  total_articles: number;
  published_count: number;
  future_count: number;
  duplicate_count: number;
  missing_count: number;
  issues: DateIssue[];
  duplicate_dates: [string, number[]][];
}

export interface DateFix {
  article_id: number;
  old_date: string;
  new_date: string;
}

export interface DateFixResult {
  fixes: DateFix[];
  articles_fixed: number;
  dry_run: boolean;
}

export interface DatePolicyReport {
  total_checked: number;
  future_count: number;
  duplicate_count: number;
  issues: DateIssue[];
  duplicate_dates: [string, number[]][];
}

export interface ArticleLinkProfile {
  id: number;
  article_id: number;
  title: string;
  file: string;
  outgoing_ids: number[];
  incoming_ids: number[];
  unresolved_links: string[];
}

export interface LinkScanResult {
  total_articles: number;
  total_internal_links: number;
  total_links: number;
  articles_with_outgoing: number;
  articles_with_incoming: number;
  orphan_ids: number[];
  profiles: ArticleLinkProfile[];
}

// ─── Reddit ───────────────────────────────────────────────────────────────────

export interface RedditOpportunity {
  post_id: string;
  title?: string;
  url?: string;
  subreddit?: string;
  author?: string;
  posted_date?: string;
  upvotes?: number;
  comment_count?: number;
  relevance_score?: number;
  engagement_score?: number;
  accessibility_score?: number;
  final_score?: number;
  severity?: string;
  why_relevant?: string;
  key_pain_points: string[];
  website_fit?: string;
  mention_stance?: 'REQUIRED' | 'RECOMMENDED' | 'OPTIONAL' | 'OMIT';
  reply_status: string;
  reply_text?: string;
  reply_url?: string;
  reply_upvotes?: number;
  reply_replies?: number;
  posted_at?: string;
  project_id: string;
  created_at: string;
  updated_at: string;
}

export interface SubmissionSummary {
  post_id: string;
  title?: string;
  url?: string;
  subreddit?: string;
  author?: string;
  upvotes?: number;
  comment_count?: number;
  created_at?: string;
  days_old?: number;
  selftext?: string;
}

export interface ValidationResult {
  valid: boolean;
  error?: string;
}

export interface RedditStats {
  total_opportunities: number;
  by_status: Record<string, number>;
  pending_by_severity: Record<string, number>;
  average_score: number;
  max_score: number;
}

export interface MigrationResult {
  migrated: number;
  skipped: number;
  errors: string[];
}

// ─── GSC Types ────────────────────────────────────────────────────────────────

export interface PageMetrics {
  page: string;
  clicks: number;
  impressions: number;
  ctr: number;
  position: number;
}

export interface QueryMetrics {
  query: string;
  clicks: number;
  impressions: number;
  ctr: number;
  position: number;
}

export interface MoverMetrics {
  key: string;
  current_clicks: number;
  current_impressions: number;
  current_position: number;
  previous_clicks: number;
  previous_impressions: number;
  previous_position: number;
  clicks_delta: number;
  impressions_delta: number;
  position_delta: number;
}

export interface InspectionRecord {
  url: string;
  verdict?: string;
  coverage_state?: string;
  indexing_state?: string;
  robots_txt_state?: string;
  page_fetch_state?: string;
  crawl_allowed?: boolean;
  indexing_allowed?: boolean;
  last_crawl_time?: string;
  google_canonical?: string;
  user_canonical?: string;
  sitemaps: string[];
  reason_code?: string;
  action?: string;
  priority: number;
}

export interface Coverage404Record {
  url: string;
  last_crawled?: string;
  category: string;
  reason: string;
  priority: number;
  suggested_action: string;
  path: string;
}

export interface RedirectRecord {
  url: string;
  last_crawled?: string;
  redirect_type: string;
  issue: string;
  priority: number;
  suggested_action: string;
  final_url?: string;
}

export interface GscAuthStatus {
  service_account_configured: boolean;
  oauth_configured: boolean;
  authenticated: boolean;
  method?: string;
  sa_path?: string;
  oauth_path?: string;
}

// ─── SEO / Ahrefs Types ───────────────────────────────────────────────────────

export interface KeywordIdea {
  keyword: string;
  idea_type: 'regular' | 'question';
  difficulty?: string;
  volume?: string;
  country?: string;
}

export interface KeywordIdeasResult {
  keyword: string;
  country: string;
  search_engine: string;
  ideas: KeywordIdea[];
  question_ideas: KeywordIdea[];
}

export interface SerpEntry {
  title: string;
  url: string;
  domain: string;
  position: number;
}

export interface KeywordDifficultyResult {
  keyword: string;
  difficulty: number;
  shortage: number;
  last_update: string;
  serp: SerpEntry[];
}

export interface BacklinkItem {
  anchor: string;
  domain_rating: number;
  title: string;
  url_from: string;
  url_to: string;
  edu: boolean;
  gov: boolean;
}

export interface DomainOverview {
  domain_rating?: number;
  traffic?: number;
  referring_domains?: number;
  backlinks?: number;
}

export interface BacklinksResult {
  domain: string;
  overview?: DomainOverview;
  backlinks: BacklinkItem[];
}

export interface TrafficMonthly {
  traffic_monthly_avg: number;
  cost_monthly_avg: number;
}

export interface TrafficTopPage {
  url?: string;
  traffic?: number;
  keywords?: number;
}

export interface TrafficTopKeyword {
  keyword?: string;
  traffic?: number;
  position?: number;
}

export interface TrafficTopCountry {
  country?: string;
  traffic?: number;
  share?: number;
}

export interface TrafficResult {
  domain: string;
  traffic: TrafficMonthly;
  traffic_history: Record<string, unknown>[];
  top_pages: TrafficTopPage[];
  top_countries: TrafficTopCountry[];
  top_keywords: TrafficTopKeyword[];
}

// ─── Publish Articles ─────────────────────────────────────────────────────────

export interface ArticleWithIssue {
  article: Article;
  issue: string;
}

export interface YearMismatch {
  article_id: number;
  title: string;
  title_year: number;
  publish_year: number;
}

export interface YearMismatchResolution {
  article_id: number;
  /** "update_title" or "backdate" */
  action: 'update_title' | 'backdate';
  /** New title string (update_title) or new date YYYY-MM-DD (backdate) */
  new_value: string;
}

export interface PublishPreflightResult {
  ready: Article[];
  needs_date_fix: ArticleWithIssue[];
  year_mismatches: YearMismatch[];
  blocked: ArticleWithIssue[];
  structural_issue_count: number;
}

export interface PublishedArticle {
  id: number;
  title: string;
  published_date: string;
}

export interface PublishResult {
  published: PublishedArticle[];
  skipped: ArticleWithIssue[];
  errors: string[];
}

// ─── Phase 6: Workflow Engine + Batch + Scheduler + Ledger ───────────────────

export type View =
  | 'overview'
  | 'tasks'
  | 'articles'
  | 'reddit'
  | 'gsc'
  | 'seo'
  | 'social'
  | 'settings'
  | 'scheduler'
  | 'history';

export interface StepProgress {
  step_name: string;
  kind: string;
  status: 'pending' | 'running' | 'ok' | 'failed' | 'skipped';
  message: string;
  output?: string;
}

export interface ExecutionResult {
  task_id: string;
  success: boolean;
  message: string;
  steps: StepProgress[];
  follow_up_tasks?: FollowUpTask[];
  started_at: string;
  finished_at: string;
}

export interface FollowUpTask {
  id: string;
  task_type: string;
  title: string;
  status: string;
  execution_mode: string;
  priority: string;
}

// ─── Global Task Queue ────────────────────────────────────────────────────────

/** Minimal descriptor used to add a task to the global queue. */
export interface QueueItem {
  taskId: string;
  projectId: string;
  title: string;
  taskType: string;
  projectName?: string;
  /** Runtime status for frontend queue tracking */
  status?: 'pending' | 'running' | 'completed' | 'failed';
  error?: string;
  /** Full execution result including follow_up_tasks */
  result?: ExecutionResult;
}

/** Runtime item tracking a task through the queue lifecycle. */
export interface RunnerItem {
  task: {
    id: string;
    title?: string;
    type?: string;
    projectId?: string;
    projectName?: string;
  };
  status: 'queued' | 'running' | 'done' | 'failed';
  result?: ExecutionResult;
  /** Live step updates streamed from the executor before the full result arrives. */
  liveSteps?: StepProgress[];
  error?: string;
}

/** Emitted by the Rust executor after each workflow step completes. */
export interface TaskStepEvent {
  task_id: string;
  step_name: string;
  status: string;
  message: string;
}

export interface BatchTaskResult {
  task_id: string;
  task_type: string;
  title: string;
  success: boolean;
  message: string;
}

export interface BatchResult {
  status: 'complete' | 'error' | 'paused';
  processed: number;
  errors: BatchTaskResult[];
  results: BatchTaskResult[];
  duration_ms: number;
}

export interface BatchSummary {
  total_ready: number;
  automatic: number;
  batchable: number;
}

export interface SchedulerRule {
  rule_id: string;
  project_id: string;
  task_type: string;
  action: 'create_task' | 'reminder_only';
  interval_hours: number;
  priority: 'high' | 'medium' | 'low';
  phase: string;
  enabled: boolean;
  last_run_at?: string;
}

export interface DueRuleResult {
  rule_id: string;
  is_due: boolean;
  next_due_at: string;
  reason: string;
}

export interface SchedulerCycleResult {
  started_at: string;
  finished_at: string;
  project_id: string;
  rules_evaluated: number;
  tasks_created: number;
  errors: string[];
  due_rules: DueRuleResult[];
}

export interface RunSummary {
  run_id: string;
  project_id: string;
  started_at: string;
  finished_at: string;
  tasks_processed: number;
  tasks_succeeded: number;
  tasks_failed: number;
  errors: string[];
}

export interface LedgerEvent {
  timestamp: string;
  event_type: string;
  payload: Record<string, unknown>;
}

// ─── Phase 7: Skills, Prompts, Agent Interaction ──────────────────────────────

export interface Skill {
  name: string;
  skill_dir: string;
  description: string;
  /** Full raw SKILL.md content */
  content: string;
}

export interface PromptSection {
  label: string;
  content: string;
}

export interface PromptContext {
  task_id: string;
  skill_name: string;
  project_id: string;
  prompt: string;
  word_count: number;
  sections: PromptSection[];
}

export interface NormalizedArtifact {
  raw_output: string;
  json_artifact: Record<string, unknown> | unknown[] | null;
  extraction_method: 'json_block' | 'bare_json' | 'json_line' | 'none';
  success: boolean;
}

export interface AgentInfo {
  name: string;
  binary: string;
  available: boolean;
  version?: string;
}

export interface AgentStatus {
  available_agents: AgentInfo[];
  configured_provider: string;
}

// ─── Overview ─────────────────────────────────────────────────────────────────

export interface TaskStatusCounts {
  todo: number;
  in_progress: number;
  review: number;
  done: number;
  total: number;
}

export interface RecentTask {
  id: string;
  title?: string;
  task_type: string;
  status: string;
  updated_at: string;
}

export interface ArticleStatusCounts {
  total: number;
  published: number;
  draft: number;
  last_published_date?: string;
}

export interface WorkflowActivity {
  task_type: string;
  label: string;
  last_run_at?: string;
  next_due_at?: string;
  interval_hours?: number;
}

export interface ProjectOverview {
  tasks: TaskStatusCounts;
  recent_tasks: RecentTask[];
  articles: ArticleStatusCounts;
  ready_task_count: number;
  workflow_activity: WorkflowActivity[];
}

export interface QuickAction {
  task_type: string;
  label: string;
  description: string;
  themes?: string[];
}

// TaskArtifact is defined at the top of this file.

// ─── Project setup / diagnostics ─────────────────────────────────────────────

export type ContentDirSource =
  | 'workspace_config'
  | 'project_override'
  | 'auto_discovered'
  | 'not_found';

export type SetupSeverity = 'error' | 'warn' | 'info';

export interface ContentDirResult {
  source: ContentDirSource;
  /** Absolute path, or null when not found */
  path: string | null;
  /** Human-readable explanation of how the dir was found */
  how: string;
  file_count: number;
}

export interface SetupCheckItem {
  id: string;
  severity: SetupSeverity;
  title: string;
  detail: string;
  fix_hint: string | null;
  /** When true the UI can call initWorkspaceConfig to resolve it */
  auto_fixable: boolean;
}

export interface ProjectConfigFileStatus {
  id: string;
  category: string;
  label: string;
  relative_path: string;
  full_path: string;
  full_link: string;
  used_by: string;
  required: boolean;
  configured: boolean;
  detail: string;
}

export interface ProjectSetup {
  project_id: string;
  repo_root: string;
  automation_dir: string;
  workspace_config_path: string;
  workspace_config_exists: boolean;
  workspace_config: { content_dir?: string; site_url?: string } | null;
  articles_json_exists: boolean;
  content_dir: ContentDirResult;
  checks: SetupCheckItem[];
  /** false when any Error-severity check is present */
  is_valid: boolean;
  summary: string;
}

export interface ContentHealthResult {
  checked: number;
  content_files: number;
  /** Number of articles where frontmatter date ≠ articles.json date */
  date_mismatches: number;
  /** Title or id of each mismatched article */
  mismatch_details: string[];
  /** MDX files on disk with no entry in articles.json */
  orphan_files: string[];
  /** true when this result came from apply_date_fixes (fixes were written) */
  fixed: boolean;
}

export interface IngestOrphanResult {
  /** Number of files successfully ingested */
  ingested: number;
  /** Basenames of newly added files */
  files: string[];
}

// ═══════════════════════════════════════════════════════════════════════════════
// Social Media Marketing Types
// ═══════════════════════════════════════════════════════════════════════════════

export type Platform = 'tiktok' | 'instagram_feed' | 'instagram_reel' | 'instagram_story';
export type PostStatus = 'draft' | 'review' | 'approved' | 'scheduled' | 'posted' | 'failed';
export type SourceType = 'article' | 'screenshot' | 'spec';
export type PostFormat = 'single_image' | 'carousel' | 'video_hook';
export type CampaignStatus = 'draft' | 'generating' | 'active' | 'completed';
export type AssetType = 'image' | 'video';
export type CanvasSize = 'tiktok' | 'square' | 'portrait' | 'story';
export type TextPosition = 'top' | 'center' | 'bottom';
export type ScheduleStrategy = 'immediate' | 'staggered' | 'specific_times';

export interface SourceConfig {
  include_articles: boolean;
  article_slugs: string[];
  include_screenshots: boolean;
  screenshot_dirs: string[];
  include_specs: boolean;
}

export interface VisualAsset {
  path: string;
  asset_type: AssetType;
  description: string;
  overlay_text?: string;
}

export interface PostMetrics {
  views?: number;
  likes?: number;
  comments?: number;
  shares?: number;
  clicks?: number;
}

export interface OverlayConfig {
  canvas_size: CanvasSize;
  font_family: string;
  primary_color: string;
  secondary_color: string;
  text_position: TextPosition;
  max_text_length: number;
}

export interface TemplateExample {
  hook: string;
  caption: string;
  visual_description: string;
}

export interface SocialCampaign {
  id: string;
  project_id: string;
  name: string;
  description?: string;
  source_config: SourceConfig;
  target_platforms: Platform[];
  template_ids: string[];
  status: CampaignStatus;
  post_count: number;
  created_at: string;
  updated_at: string;
}

export interface SocialPost {
  id: string;
  campaign_id: string;
  project_id: string;
  source_type: SourceType;
  source_id: string;
  source_url?: string;
  platform: Platform;
  format: PostFormat;
  hook: string;
  caption: string;
  hashtags: string[];
  cta: string;
  visual_assets: VisualAsset[];
  /** AI-generated prompt for external image generation (Midjourney, DALL-E, etc.) */
  image_generation_prompt?: string;
  status: PostStatus;
  scheduled_at?: string;
  posted_at?: string;
  platform_post_id?: string;
  platform_post_url?: string;
  metrics?: PostMetrics;
  template_id: string;
  generated_by?: string;
  generation_prompt_hash?: string;
  created_at: string;
  updated_at: string;
}

export interface ContentTemplate {
  id: string;
  project_id?: string;
  name: string;
  description?: string;
  platform: Platform;
  format: PostFormat;
  creation_prompt: string;
  overlay_config: OverlayConfig;
  default_hashtags: string[];
  example_output?: TemplateExample;
  created_at: string;
  updated_at: string;
}

export interface CampaignStats {
  total_posts: number;
  by_status: Record<string, number>;
  by_platform: Record<string, number>;
}

export interface ScheduleConfig {
  strategy: ScheduleStrategy;
  start_at?: string;
  interval_hours?: number;
}

export interface CreateCampaignRequest {
  project_id: string;
  name: string;
  description?: string;
  source_config: SourceConfig;
  target_platforms: Platform[];
  template_ids: string[];
}

export interface CreateTemplateRequest {
  project_id?: string;
  name: string;
  platform: Platform;
  format: PostFormat;
  description: string;
}
