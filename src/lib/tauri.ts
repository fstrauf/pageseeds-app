import { invoke } from '@tauri-apps/api/core'
import { open as dialogOpen } from '@tauri-apps/plugin-dialog'
import type {
  Article,
  ContentHealthResult,
  EmbeddingStatus,
  FormatFixResult,
  FormatValidationResult,
  ImportResult,
  IngestOrphanResult,
  KeywordCoverageStatus,
  MigrationResult,
  LiveSiteAuditReport,
  LiveSiteGscSyncResult,
  LiveSiteImportResult,
  LiveSiteLinkScanResult,
  LiveSitePage,
  Project,
  ProjectMode,
  ProjectConfigFileStatus,
  ProjectSetup,
  QueueSnapshot,
  EnqueueItem,
  EnqueueMode,
  RedditOpportunity,
  RedditStats,
  RecoveryStats,
  ScoredSkill,
  SecretsStatus,
  SubmissionSummary,
  Task,
  ValidationResult,
} from './types'

// ─── Projects ─────────────────────────────────────────────────────────────────

export const listProjects = (): Promise<Project[]> =>
  invoke('list_projects')

export const createProject = (args: {
  name: string
  path?: string
  content_dir?: string
  site_url?: string
  site_id?: string
  sitemap_url?: string
  project_mode?: ProjectMode
}): Promise<Project> =>
  invoke('create_project', {
    name: args.name,
    path: args.path,
    contentDir: args.content_dir,
    siteUrl: args.site_url,
    siteId: args.site_id,
    sitemapUrl: args.sitemap_url,
    projectMode: args.project_mode,
  })

export const updateProject = (project: Project): Promise<Project> =>
  invoke('update_project', { project })

export const deleteProject = (id: string): Promise<void> =>
  invoke('delete_project', { id })

export const openFolderDialog = (title?: string): Promise<string | null> =>
  dialogOpen({ directory: true, multiple: false, title: title ?? 'Select folder' }).then(
    result => (typeof result === 'string' ? result : null),
  )

// ─── Tasks ────────────────────────────────────────────────────────────────────

export const listTasks = (
  projectId: string,
  status?: string,
  phase?: string,
): Promise<Task[]> =>
  invoke('list_tasks', { projectId, status, phase })

export const getTask = (id: string): Promise<Task> =>
  invoke('get_task', { id })

export const createTask = (
  projectId: string,
  taskType: string,
  title: string | undefined,
  description: string | undefined,
  priority: string,
  autoEnqueue?: boolean,
): Promise<Task> =>
  invoke('create_task', { projectId, taskType, title, description, priority, autoEnqueue })

export const updateTaskStatus = (id: string, status: string): Promise<Task> =>
  invoke('update_task_status', { id, status })

export const updateTask = (
  id: string,
  title: string | undefined,
  description: string | undefined,
  priority: string,
): Promise<Task> =>
  invoke('update_task', { id, title, description, priority })

export const deleteTask = (id: string): Promise<void> =>
  invoke('delete_task', { id })

export const cancelTask = (id: string): Promise<Task> =>
  invoke('cancel_task', { id })

export const createArticleTasksFromKeywords = (
  projectId: string,
  researchTaskId: string,
  keywords: string[],
): Promise<Task[]> =>
  invoke('create_article_tasks_from_keywords', { projectId, researchTaskId, keywords })

export const createGscIndexingRecoveryTask = (projectId: string): Promise<Task> =>
  invoke('create_gsc_indexing_recovery_task', { projectId })

export const getGscRecoveryStats = (
  projectId: string,
): Promise<RecoveryStats> =>
  invoke('get_gsc_recovery_stats', { projectId })

// ─── Articles ─────────────────────────────────────────────────────────────────

export const listArticles = (projectId: string): Promise<Article[]> =>
  invoke('list_articles', { projectId })

export const listLiveSitePages = (projectId: string): Promise<LiveSitePage[]> =>
  invoke('list_live_site_pages', { projectId })

export const getLiveSiteAudit = (projectId: string): Promise<LiveSiteAuditReport> =>
  invoke('get_live_site_audit', { projectId })

export const scanLiveSiteLinks = (projectId: string): Promise<LiveSiteLinkScanResult> =>
  invoke('scan_live_site_links', { projectId })

// ─── Import / Export ──────────────────────────────────────────────────────────

export const importFromRepo = (projectId: string): Promise<ImportResult> =>
  invoke('import_from_repo', { projectId })

export const importLiveSite = (
  projectId: string,
  limit?: number,
): Promise<LiveSiteImportResult> =>
  invoke('import_live_site', { projectId, limit })

export const syncLiveSiteGsc = (
  projectId: string,
  startDate: string,
  endDate: string,
  limit?: number,
): Promise<LiveSiteGscSyncResult> =>
  invoke('sync_live_site_gsc', { projectId, startDate, endDate, limit })

export const exportToRepo = (projectId: string): Promise<void> =>
  invoke('export_to_repo', { projectId })

// ─── Config / Secrets ─────────────────────────────────────────────────────────

export const getSecretsStatus = (projectId: string): Promise<SecretsStatus> =>
  invoke('get_secrets_status', { projectId })

export const getSecretsFilePath = (): Promise<string> =>
  invoke('get_secrets_file_path')

/** One-time migration: read a .env file and merge its vars into ~/.config/automation/secrets.env.
 *  Returns the list of keys that were imported. */
export const importEnvFile = (sourcePath: string): Promise<string[]> =>
  invoke('import_env_file', { sourcePath })

// ─── Content / SEO ──────────────────────────────────────────────────────────

import type {
  CleaningResult,
  ContentDirResolution,
  DatePolicyReport,
  DateFixResult,
  LinkScanResult,
} from './types'

export const resolveContentDir = (projectId: string): Promise<ContentDirResolution> =>
  invoke('resolve_content_dir', { projectId })

export const scanContentHealth = (projectId: string, dryRun: boolean): Promise<CleaningResult> =>
  invoke('scan_content_health', { projectId, dryRun })

export const fixContentDates = (projectId: string, dryRun: boolean): Promise<DateFixResult> =>
  invoke('fix_content_dates', { projectId, dryRun })

export const analyzeArticleDatePolicy = (
  projectId: string,
  statusFilter?: string[],
  allowedFutureDays?: number,
): Promise<DatePolicyReport> =>
  invoke('analyze_article_date_policy', {
    projectId,
    statusFilter: statusFilter ?? null,
    allowedFutureDays: allowedFutureDays ?? null,
  })

export const suggestNextArticlePublishDate = (projectId: string): Promise<string> =>
  invoke('suggest_next_article_publish_date', { projectId })

export const scanContentLinks = (projectId: string): Promise<LinkScanResult> =>
  invoke('scan_content_links', { projectId })

import type {
  PublishPreflightResult,
  PublishResult,
  YearMismatchResolution,
  CtrHealthSummary,
  RepairPathResult,
} from './types'
// Re-export to keep callers from needing to import from types directly
export type { PublishPreflightResult, PublishResult, YearMismatchResolution, CtrHealthSummary, RepairPathResult }
export type { ArticleWithIssue } from './types'
export type { YearMismatch } from './types'

export const preflightPublishArticles = (
  projectId: string,
  articleIds: number[],
): Promise<PublishPreflightResult> =>
  invoke('preflight_publish_articles', { projectId, articleIds })

export const applyPublishArticles = (
  projectId: string,
  articleIds: number[],
  dateFixes: Record<string, string>,
  yearResolutions: YearMismatchResolution[],
): Promise<PublishResult> =>
  invoke('apply_publish_articles', { projectId, articleIds, dateFixes, yearResolutions })

export const resolveYearMismatchAgent = (
  projectId: string,
  articleId: number,
  title: string,
  titleYear: number,
  publishYear: number,
): Promise<YearMismatchResolution> =>
  invoke('resolve_year_mismatch_agent', { projectId, articleId, title, titleYear, publishYear })

export const getCtrHealthSummary = (
  projectId: string,
): Promise<CtrHealthSummary> =>
  invoke('get_ctr_health_summary', { projectId })

export const repairArticlePaths = (
  projectId: string,
): Promise<RepairPathResult> =>
  invoke('repair_article_paths', { projectId })



// ─── Reddit ──────────────────────────────────────────────────────────────────

export const searchReddit = (
  query: string,
  subreddit: string,
  limit?: number,
  sort?: string,
  timeFilter?: string,
): Promise<SubmissionSummary[]> =>
  invoke('search_reddit', { query, subreddit, limit, sort, timeFilter })

export const listRedditOpportunities = (
  projectId: string,
  status?: string,
): Promise<RedditOpportunity[]> =>
  invoke('list_reddit_opportunities', { projectId, status })

export const upsertRedditOpportunity = (
  opportunity: RedditOpportunity,
): Promise<void> =>
  invoke('upsert_reddit_opportunity', { opportunity })

export const markRedditPosted = (
  postId: string,
  replyText: string,
  replyUrl: string,
  projectId?: string,
): Promise<void> =>
  invoke('mark_reddit_posted', { postId, replyText, replyUrl, projectId: projectId ?? null })

export const markRedditSkipped = (postId: string, projectId?: string): Promise<void> =>
  invoke('mark_reddit_skipped', { postId, projectId: projectId ?? null })

/** Actually post the drafted reply to Reddit via the CLI.
 *  Returns the comment URL on success, throws on failure. */
export const postToReddit = (
  projectId: string,
  postId: string,
  replyText: string,
): Promise<string> =>
  // Note: Rust expects post_id, reply_text, project_id order
  invoke('post_to_reddit', { postId, replyText, projectId })

export const getRedditStatistics = (projectId: string): Promise<RedditStats> =>
  invoke('get_reddit_statistics', { projectId })

export const validateRedditReply = (
  text: string,
  projectId?: string,
): Promise<ValidationResult> =>
  invoke('validate_reddit_reply', { text, projectId: projectId ?? null })

export const migrateRedditDb = (
  projectId: string,
  sourcePath: string,
): Promise<MigrationResult> =>
  invoke('migrate_reddit_db', { projectId, sourcePath })

export const draftRedditReply = (
  projectId: string,
  postId: string,
): Promise<string> =>
  invoke('draft_reddit_reply', { projectId, postId })

export const enrichRedditOpportunities = (
  projectId: string,
): Promise<string> =>
  invoke('enrich_reddit_opportunities', { projectId })

/** Create reply tasks from selected Reddit opportunities.
 * Called after user selects opportunities in the picker.
 */
export const createRedditReplyTasks = (
  taskId: string,
  postIds: string[],
): Promise<Task[]> =>
  invoke('create_reddit_reply_tasks', { taskId, postIds })

// ─── GSC ─────────────────────────────────────────────────────────────────────

import type {
  Coverage404Record,
  GscAuthStatus,
  GscDriftReport,
  InspectionRecord,
  MoverMetrics,
  PageMetrics,
  QueryMetrics,
  RedirectRecord,
} from './types'

export const gscGetAuthStatus = (projectId: string): Promise<GscAuthStatus> =>
  invoke('gsc_get_auth_status', { projectId })

export const gscAuthenticate = (projectId: string): Promise<void> =>
  invoke('gsc_authenticate', { projectId })

export const gscOAuthStart = (projectId: string): Promise<void> =>
  invoke('gsc_oauth_start', { projectId })

export const gscFetchAnalytics = (
  siteUrl: string,
  startDate: string,
  endDate: string,
  limit?: number,
): Promise<PageMetrics[]> =>
  invoke('gsc_fetch_analytics', { siteUrl, startDate, endDate, limit })

export const gscFetchQueriesForPage = (
  siteUrl: string,
  pageUrl: string,
  startDate: string,
  endDate: string,
  limit?: number,
): Promise<QueryMetrics[]> =>
  invoke('gsc_fetch_queries_for_page', { siteUrl, pageUrl, startDate, endDate, limit })

export const gscComputeMovers = (
  siteUrl: string,
  currStart: string,
  currEnd: string,
  prevStart: string,
  prevEnd: string,
  limit?: number,
): Promise<MoverMetrics[]> =>
  invoke('gsc_compute_movers', { siteUrl, currStart, currEnd, prevStart, prevEnd, limit })

export const gscInspectUrls = (
  siteUrl: string,
  urls: string[],
): Promise<InspectionRecord[]> =>
  invoke('gsc_inspect_urls', { siteUrl, urls })

export const gscGenerateIndexingReport = (
  projectId: string,
  siteUrl: string,
  records: InspectionRecord[],
): Promise<string> =>
  invoke('gsc_generate_indexing_report', { projectId, siteUrl, records })

export const gscParseCoverageCsv = (csvContent: string): Promise<Coverage404Record[]> =>
  invoke('gsc_parse_coverage_csv', { csvContent })

export const gscParseRedirectCsv = (csvContent: string): Promise<RedirectRecord[]> =>
  invoke('gsc_parse_redirect_csv', { csvContent })

export const gscComputeDrift = (projectId: string): Promise<GscDriftReport> =>
  invoke('gsc_compute_drift', { projectId })

// ─── SEO / Ahrefs ─────────────────────────────────────────────────────────────

import type {
  BacklinksResult,
  KeywordDifficultyResult,
  KeywordIdeasResult,
  TrafficResult,
} from './types'

export const seoGetKeywordIdeas = (
  projectId: string,
  keyword: string,
  country?: string,
  searchEngine?: string,
): Promise<KeywordIdeasResult> =>
  invoke('seo_get_keyword_ideas', { projectId, keyword, country, searchEngine })

export const seoGetKeywordDifficulty = (
  projectId: string,
  keyword: string,
  country?: string,
): Promise<KeywordDifficultyResult> =>
  invoke('seo_get_keyword_difficulty', { projectId, keyword, country })

export const seoGetBacklinks = (
  projectId: string,
  domain: string,
): Promise<BacklinksResult> =>
  invoke('seo_get_backlinks', { projectId, domain })

export const seoCheckTraffic = (
  projectId: string,
  domain: string,
  mode?: string,
  country?: string,
): Promise<TrafficResult> =>
  invoke('seo_check_traffic', { projectId, domain, mode, country })

export const seoBatchKeywordDifficulty = (
  projectId: string,
  keywords: string[],
  country?: string,
): Promise<KeywordDifficultyResult[]> =>
  invoke('seo_batch_keyword_difficulty', { projectId, keywords, country })

export const getSeoProvider = (projectId: string): Promise<string> =>
  invoke('get_seo_provider', { projectId })

export const setSeoProvider = (projectId: string, provider: string): Promise<string> =>
  invoke('set_seo_provider', { projectId, provider })

// ─── Phase 6: Workflow Engine + Batch + Scheduler + Ledger ───────────────────

import type {
  BatchResult,
  BatchSummary,
  DueRuleResult,
  LedgerEvent,
  RunSummary,
  SchedulerCycleResult,
  SchedulerRule,
} from './types'

export type { DueRuleResult } // re-export so components can import from tauri.ts

// ── Batch ─────────────────────────────────────────────────────────────────────

export const getBatchSummary = (projectId: string): Promise<BatchSummary> =>
  invoke('get_batch_summary', { projectId })

export const runBatch = (
  projectId: string,
  maxTasks?: number,
  pauseOnError?: boolean,
): Promise<BatchResult> =>
  invoke('run_batch', { projectId, maxTasks, pauseOnError })

/** Get current queue snapshot from the backend. */
export const getQueueSnapshot = (): Promise<QueueSnapshot> =>
  invoke('get_queue_snapshot')

/** Enqueue tasks into the backend-owned queue. */
export const enqueueTasks = (items: EnqueueItem[], mode: EnqueueMode = 'append'): Promise<QueueSnapshot> =>
  invoke('enqueue_tasks', { items, mode })

/** Remove a pending item from the queue. */
export const removeQueueItem = (taskId: string): Promise<QueueSnapshot> =>
  invoke('remove_queue_item', { taskId })

/** Dismiss/hide the active queue run. */
export const dismissQueue = (): Promise<void> =>
  invoke('dismiss_queue')

// ── Legacy queue commands (delegate to backend queue) ─────────────────────────

/** Legacy queue item returned by get_queue_state. */
export interface LegacyQueueStateItem {
  taskId: string
  projectId: string
  title: string
  taskType: string
  projectName?: string
  status?: string
  error?: string
}

/** Legacy: get queue state by status. Delegates to get_queue_snapshot. */
export const getQueueState = (): Promise<LegacyQueueStateItem[]> =>
  invoke('get_queue_state')

/** Legacy queue item shape for execute_queue compatibility. */
export interface LegacyQueueItem {
  taskId: string
  projectId: string
  title: string
  taskType: string
  projectName?: string
  status?: string
  error?: string
}

/** Legacy: execute a queue of tasks. Delegates to enqueue_tasks. */
export const executeQueue = (items: LegacyQueueItem[]): Promise<void> =>
  invoke('execute_queue', { items })

/** Legacy: no-op, queue manages statuses internally. */
export const markTasksQueued = (taskIds: string[]): Promise<void> =>
  invoke('mark_tasks_queued', { taskIds })

/** Legacy: reset queued tasks back to todo. Delegates to remove_queue_item. */
export const markTasksTodo = (taskIds: string[]): Promise<void> =>
  invoke('mark_tasks_todo', { taskIds })

export const pauseQueue = (): Promise<QueueSnapshot> =>
  invoke('pause_queue')

export const resumeQueue = (): Promise<QueueSnapshot> =>
  invoke('resume_queue')

export const clearCompletedQueueItems = (): Promise<QueueSnapshot> =>
  invoke('clear_completed_queue_items')

// ── Scheduler ─────────────────────────────────────────────────────────────────

export const listSchedulerRules = (projectId: string): Promise<SchedulerRule[]> =>
  invoke('list_scheduler_rules', { projectId })

export const upsertSchedulerRule = (rule: SchedulerRule): Promise<void> =>
  invoke('upsert_scheduler_rule', { rule })

export const deleteSchedulerRule = (ruleId: string): Promise<void> =>
  invoke('delete_scheduler_rule', { ruleId })

export const setSchedulerRuleEnabled = (ruleId: string, enabled: boolean): Promise<void> =>
  invoke('set_scheduler_rule_enabled', { ruleId, enabled })

export const runSchedulerCycle = (projectId: string): Promise<SchedulerCycleResult> =>
  invoke('run_scheduler_cycle', { projectId })

// ── Ledger ────────────────────────────────────────────────────────────────────

export const listLedgerRuns = (projectId: string): Promise<string[]> =>
  invoke('list_ledger_runs', { projectId })

export const getLedgerRunSummary = (projectId: string, runId: string): Promise<RunSummary> =>
  invoke('get_ledger_run_summary', { projectId, runId })

export const getLedgerRunEvents = (projectId: string, runId: string): Promise<LedgerEvent[]> =>
  invoke('get_ledger_run_events', { projectId, runId })

// ─── Phase 7: Skills, Prompts, Agent Interaction ──────────────────────────────

import type {
  AgentStatus,
  ProjectOverview,
  PromptContext,
  Skill,
  TaskArtifact,
} from './types'

export const listSkills = (projectId: string): Promise<Skill[]> =>
  invoke('list_skills', { projectId })

export const getSkill = (projectId: string, skillName: string): Promise<Skill> =>
  invoke('get_skill', { projectId, skillName })

export const checkEmbeddingStatus = (): Promise<EmbeddingStatus> =>
  invoke('check_embedding_status')

export const indexSkills = (projectId: string): Promise<number> =>
  invoke('index_skills', { projectId })

export const searchSkills = (
  projectId: string,
  query: string,
  limit?: number,
): Promise<ScoredSkill[]> =>
  invoke('search_skills', { projectId, query, limit })

export const buildPromptPreview = (taskId: string, skillName: string): Promise<PromptContext> =>
  invoke('build_prompt_preview', { taskId, skillName })

export const listTaskArtifacts = (taskId: string): Promise<TaskArtifact[]> =>
  invoke('list_task_artifacts', { taskId })

export const checkAgentStatus = (): Promise<AgentStatus> =>
  invoke('check_agent_status')

export const setAgentProvider = (provider: string): Promise<string> =>
  invoke('set_agent_provider', { provider })

export const getGlobalAgentProvider = (): Promise<string> =>
  invoke('get_global_agent_provider')

export const getGlobalSettings = (): Promise<Array<{ key: string; value: string }>> =>
  invoke('get_global_settings')

export const getLogFilePath = (): Promise<string> =>
  invoke('get_log_file_path')

// ─── Logging ─────────────────────────────────────────────────────────────────

export const submitLog = (entry: {
  timestamp: string
  level: string
  component: string
  message: string
  metadata?: Record<string, unknown>
  session_id: string
}): Promise<number> => invoke('submit_log', { entry })

export const submitLogsBatch = (entries: Array<{
  timestamp: string
  level: string
  component: string
  message: string
  metadata?: Record<string, unknown>
  session_id: string
}>): Promise<number[]> => invoke('submit_logs_batch', { entries })

export const queryLogs = (
  filters: {
    level?: string
    source?: string
    component?: string
    session_id?: string
    search_query?: string
  },
  limit?: number,
  offset?: number
): Promise<Array<{
  id?: number
  timestamp: string
  level: string
  source: string
  component: string
  message: string
  metadata?: Record<string, unknown>
  session_id: string
}>> => invoke('query_logs', { filters, limit, offset })

export const getRecentLogs = (limit?: number): Promise<Array<{
  id?: number
  timestamp: string
  level: string
  source: string
  component: string
  message: string
  metadata?: Record<string, unknown>
  session_id: string
}>> => invoke('get_recent_logs', { limit })

export const getLogStats = (): Promise<{
  total_count: number
  error_count: number
  warn_count: number
  info_count: number
  debug_count: number
  frontend_count: number
  backend_count: number
  agent_count: number
} | null> => invoke('get_log_stats')

export const clearOldLogs = (daysToKeep: number): Promise<number> =>
  invoke('clear_old_logs', { daysToKeep })

export const exportLogs = (filters?: {
  level?: string
  source?: string
  component?: string
  session_id?: string
  search_query?: string
}): Promise<string | null> => invoke('export_logs', { filters })

// ─── Overview ────────────────────────────────────────────────────────────────

export const getProjectOverview = (projectId: string): Promise<ProjectOverview> =>
  invoke('get_project_overview', { projectId })

export const quickRunWorkflow = (
  projectId: string,
  taskType: string,
  title: string,
  themes?: string[],
): Promise<import('./types').ExecutionResult> =>
  invoke('quick_run_workflow', { projectId, taskType, title, themes })

// ─── Setup diagnostics ───────────────────────────────────────────────────────

/** Return a full setup diagnostic for a project. */
export const checkProjectSetup = (projectId: string): Promise<ProjectSetup> =>
  invoke('check_project_setup', { projectId })

/** Return known project config files and whether each one is configured. */
export const getProjectConfigFilesStatus = (
  projectId: string,
): Promise<ProjectConfigFileStatus[]> =>
  invoke('get_project_config_files_status', { projectId })

/**
 * Create (or overwrite) seo_workspace.json for a project.
 * Returns the path of the written file.
 */
export const initWorkspaceConfig = (
  projectId: string,
  contentDir: string,
  siteUrl: string,
): Promise<string> =>
  invoke('init_workspace_config', { projectId, contentDir, siteUrl })

/**
 * Initialize a complete project workspace with all required files.
 * This creates .github/automation/, seo_workspace.json, and articles.json.
 * Returns a list of files that were created.
 */
export const initializeProjectWorkspace = (projectId: string): Promise<string[]> =>
  invoke('initialize_project_workspace', { projectId })

/** Read-only check of date consistency between articles.json and frontmatter. */
export const getContentHealth = (projectId: string): Promise<ContentHealthResult> =>
  invoke('get_content_health', { projectId })

/** Patch frontmatter dates that differ from articles.json. */
export const fixDateMismatches = (projectId: string): Promise<ContentHealthResult> =>
  invoke('fix_date_mismatches', { projectId })

/** Validate frontmatter format across all MDX files in the project. */
export const validateContentFormat = (projectId: string): Promise<FormatValidationResult> =>
  invoke('validate_content_format', { projectId })

/** Apply auto-fixes for frontmatter format issues. */
export const fixContentFormat = (projectId: string): Promise<FormatFixResult> =>
  invoke('fix_content_format', { projectId })

/** Import MDX files that exist on disk but have no entry in articles.json. */
export const ingestOrphanArticles = (projectId: string): Promise<IngestOrphanResult> =>
  invoke('ingest_orphan_articles', { projectId })

/** Remove articles.json entries whose files no longer exist on disk. Returns list of removed titles. */
export const cleanStaleArticles = (projectId: string): Promise<string[]> =>
  invoke('clean_stale_articles', { projectId })

// ═══════════════════════════════════════════════════════════════════════════════
// Social Media Marketing
// ═══════════════════════════════════════════════════════════════════════════════

import type {
  CampaignStats,
  ContentTemplate,
  CreateCampaignRequest,
  PostStatus,
  SocialCampaign,
  SocialPost,
} from './types'

// ─── Campaigns ────────────────────────────────────────────────────────────────

export const listSocialCampaigns = (projectId: string): Promise<SocialCampaign[]> =>
  invoke('list_social_campaigns', { projectId })

export const getSocialCampaign = (campaignId: string): Promise<SocialCampaign | null> =>
  invoke('get_social_campaign', { campaignId })

export const createSocialCampaign = (req: CreateCampaignRequest): Promise<SocialCampaign> =>
  invoke('create_social_campaign', { req })

export const deleteSocialCampaign = (campaignId: string): Promise<void> =>
  invoke('delete_social_campaign', { campaignId })

export const getSocialCampaignStats = (campaignId: string): Promise<CampaignStats> =>
  invoke('get_social_campaign_stats', { campaignId })

// ─── Posts ────────────────────────────────────────────────────────────────────

export const getCampaignPosts = (
  campaignId: string,
  status?: PostStatus,
): Promise<SocialPost[]> =>
  invoke('get_campaign_posts', { campaignId, status })

export const getSocialPost = (postId: string): Promise<SocialPost | null> =>
  invoke('get_social_post', { postId })

export const getSocialPostsByProject = (
  projectId: string,
  status?: PostStatus,
): Promise<SocialPost[]> =>
  invoke('get_social_posts_by_project', { projectId, status })

export const updateSocialPostStatus = (postId: string, status: PostStatus): Promise<void> =>
  invoke('update_social_post_status', { postId, status })

export const updateSocialPost = (post: SocialPost): Promise<void> =>
  invoke('update_social_post', { post })

export const scheduleSocialPost = (postId: string, scheduledAt: string): Promise<void> =>
  invoke('schedule_social_post', { postId, scheduledAt })

export const markSocialPostPosted = (postId: string, platformUrl: string): Promise<void> =>
  invoke('mark_social_post_posted', { postId, platformUrl })

export const deleteSocialPost = (postId: string): Promise<void> =>
  invoke('delete_social_post', { postId })

// ─── Templates ────────────────────────────────────────────────────────────────

export const listSocialTemplates = (projectId: string): Promise<ContentTemplate[]> =>
  invoke('list_social_templates', { projectId })

export const getSocialTemplate = (templateId: string): Promise<ContentTemplate | null> =>
  invoke('get_social_template', { templateId })

export const createSocialTemplate = (template: ContentTemplate): Promise<ContentTemplate> =>
  invoke('create_social_template', { template })

export const deleteSocialTemplate = (templateId: string): Promise<void> =>
  invoke('delete_social_template', { templateId })

// ─── Campaign Generation ──────────────────────────────────────────────────────

export const runSocialCampaign = (campaignId: string): Promise<Task> =>
  invoke('run_social_campaign', { campaignId })

// ═══════════════════════════════════════════════════════════════════════════════
// Keyword Coverage Analysis
// ═══════════════════════════════════════════════════════════════════════════════

export const getKeywordCoverage = (projectId: string): Promise<KeywordCoverageStatus> =>
  invoke('get_keyword_coverage', { projectId })

// ═══════════════════════════════════════════════════════════════════════════════
// Readability Analysis
// ═══════════════════════════════════════════════════════════════════════════════

import type { 
  ReadabilityReport, 
  IntentClassification, 
  OpportunityScore, 
  WordCountComparison,
  KeywordDensityReport,
} from './bindings'
import type { KeywordIdea } from './types'

export const analyzeArticleReadability = (projectId: string, slug: string): Promise<ReadabilityReport> =>
  invoke('analyze_article_readability', { projectId, slug })

export const classifySearchIntent = (projectId: string, keywords: string[]): Promise<IntentClassification[]> =>
  invoke('classify_search_intent', { projectId, keywords })

export const scoreKeywordOpportunities = (
  projectId: string,
  keywords: KeywordIdea[],
  intents: IntentClassification[],
  existingSlugs: string[],
): Promise<OpportunityScore[]> =>
  invoke('score_keyword_opportunities', { projectId, keywords, intents, existingSlugs: existingSlugs })

export const compareCompetitorContent = (
  keyword: string,
  competitorUrls: string[],
  userUrl?: string,
): Promise<WordCountComparison> =>
  invoke('compare_competitor_content', { keyword, competitorUrls, userUrl })

export const analyzeKeywordDensity = (
  projectId: string,
  slug: string,
  targetKeyword: string,
): Promise<KeywordDensityReport> =>
  invoke('analyze_keyword_density', { projectId, slug, targetKeyword })


// ═══════════════════════════════════════════════════════════════════════════════
// Cannibalization Review & Approval
// ═══════════════════════════════════════════════════════════════════════════════

import type {
  StrategyWithReviews,
  StrategyReview,
} from './types'

export const getCannibalizationStrategy = (
  projectId: string,
): Promise<StrategyWithReviews | null> =>
  invoke('get_cannibalization_strategy', { projectId })

export const setRecommendationApproval = (args: {
  strategyId: string
  projectId: string
  recommendationType: string
  recommendationId: string
  status: 'pending' | 'approved' | 'rejected' | 'needs_review'
  notes?: string
}): Promise<StrategyReview> =>
  invoke('set_recommendation_approval', {
    strategyId: args.strategyId,
    projectId: args.projectId,
    recommendationType: args.recommendationType,
    recommendationId: args.recommendationId,
    status: args.status,
    notes: args.notes,
  })

export const getStrategyReviews = (strategyId: string): Promise<StrategyReview[]> =>
  invoke('get_strategy_reviews', { strategyId })

export const createTasksFromApprovedRecommendations = (
  strategyId: string,
  projectId: string,
): Promise<string[]> =>
  invoke('create_tasks_from_approved_recommendations', { strategyId, projectId })

export const createCannibalizationTasksFromSelection = (
  parentTaskId: string,
  selections: { recommendation_type: string; recommendation_id: string }[],
): Promise<Task[]> =>
  invoke('create_cannibalization_tasks_from_selection', { parentTaskId, selections })

export const backfillHubPages = (projectId: string): Promise<number> =>
  invoke('backfill_hub_pages', { projectId })
