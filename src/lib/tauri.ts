import { invoke } from '@tauri-apps/api/core'
import { open as dialogOpen } from '@tauri-apps/plugin-dialog'
import type {
  Article,
  ContentHealthResult,
  ImportResult,
  IngestOrphanResult,
  MigrationResult,
  Project,
  ProjectConfigFileStatus,
  ProjectSetup,
  QueueItem,
  RedditOpportunity,
  RedditStats,
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
  path: string
  content_dir?: string
  site_url?: string
  site_id?: string
}): Promise<Project> =>
  invoke('create_project', {
    name: args.name,
    path: args.path,
    contentDir: args.content_dir,
    siteUrl: args.site_url,
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
): Promise<Task> =>
  invoke('create_task', { projectId, taskType, title, description, priority })

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

// ─── Articles ─────────────────────────────────────────────────────────────────

export const listArticles = (projectId: string): Promise<Article[]> =>
  invoke('list_articles', { projectId })

// ─── Import / Export ──────────────────────────────────────────────────────────

export const importFromRepo = (projectId: string): Promise<ImportResult> =>
  invoke('import_from_repo', { projectId })

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
} from './types'
// Re-export to keep callers from needing to import from types directly
export type { PublishPreflightResult, PublishResult, YearMismatchResolution }
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
  invoke('post_to_reddit', { projectId, postId, replyText })

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

export const runRedditOpportunitySearch = (
  projectId: string,
  userContext?: string,
): Promise<import('./types').ExecutionResult> =>
  invoke('run_reddit_opportunity_search', { projectId, userContext: userContext ?? null })

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

// ─── Phase 6: Workflow Engine + Batch + Scheduler + Ledger ───────────────────

import type {
  BatchResult,
  BatchSummary,
  DueRuleResult,
  ExecutionResult,
  LedgerEvent,
  RunSummary,
  SchedulerCycleResult,
  SchedulerRule,
} from './types'

export type { DueRuleResult } // re-export so components can import from tauri.ts

// ── Executor ──────────────────────────────────────────────────────────────────

export const executeTask = (taskId: string): Promise<ExecutionResult> =>
  invoke('execute_task', { taskId })

/** Plan steps for a task without executing anything. Returns the planned step graph. */
export const dryRunTask = (taskId: string): Promise<ExecutionResult> =>
  invoke('dry_run_task', { taskId })

// ── Batch ─────────────────────────────────────────────────────────────────────

export const getBatchSummary = (projectId: string): Promise<BatchSummary> =>
  invoke('get_batch_summary', { projectId })

export const runBatch = (
  projectId: string,
  maxTasks?: number,
  pauseOnError?: boolean,
): Promise<BatchResult> =>
  invoke('run_batch', { projectId, maxTasks, pauseOnError })

/** Execute a queue of tasks across projects. Emits events for progress tracking. */
export const executeQueue = (items: QueueItem[]): Promise<void> =>
  invoke('execute_queue', { items })

export const pauseQueue = (): Promise<void> =>
  invoke('pause_queue')

export const resumeQueue = (): Promise<void> =>
  invoke('resume_queue')

export const clearCompletedQueueItems = (): Promise<void> =>
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
  NormalizedArtifact,
  ProjectOverview,
  PromptContext,
  Skill,
  TaskArtifact,
} from './types'

export const listSkills = (projectId: string): Promise<Skill[]> =>
  invoke('list_skills', { projectId })

export const getSkill = (projectId: string, skillName: string): Promise<Skill> =>
  invoke('get_skill', { projectId, skillName })

export const buildPromptPreview = (taskId: string, skillName: string): Promise<PromptContext> =>
  invoke('build_prompt_preview', { taskId, skillName })

export const normalizeOutput = (raw: string): Promise<NormalizedArtifact> =>
  invoke('normalize_output', { raw })

export const listTaskArtifacts = (taskId: string): Promise<TaskArtifact[]> =>
  invoke('list_task_artifacts', { taskId })

export const checkAgentStatus = (projectId: string): Promise<AgentStatus> =>
  invoke('check_agent_status', { projectId })

export const setAgentProvider = (projectId: string, provider: string): Promise<void> =>
  invoke('set_agent_provider', { projectId, provider })

export const getLogFilePath = (): Promise<string> =>
  invoke('get_log_file_path')

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

/** Read-only check of date consistency between articles.json and frontmatter. */
export const getContentHealth = (projectId: string): Promise<ContentHealthResult> =>
  invoke('get_content_health', { projectId })

/** Patch frontmatter dates that differ from articles.json. */
export const fixDateMismatches = (projectId: string): Promise<ContentHealthResult> =>
  invoke('fix_date_mismatches', { projectId })

/** Import MDX files that exist on disk but have no entry in articles.json. */
export const ingestOrphanArticles = (projectId: string): Promise<IngestOrphanResult> =>
  invoke('ingest_orphan_articles', { projectId })

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
