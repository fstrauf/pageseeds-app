// ═══════════════════════════════════════════════════════════════════════════════
// Type Definitions
// ═══════════════════════════════════════════════════════════════════════════════
// 
// This file contains:
// 1. Re-exports of auto-generated types from ./bindings (generated from Rust by ts-rs)
// 2. Frontend-only composite types that don't exist in the backend
// 3. Type aliases for convenience
//
// To regenerate bindings after changing Rust structs:
//   cd src-tauri && cargo test export_bindings --lib
//   cp bindings/*.ts ../src/lib/bindings/
//
// ═══════════════════════════════════════════════════════════════════════════════

// ─── Auto-Generated Types from Rust ───────────────────────────────────────────

export type {
  // Enums
  AgentPolicy,
  FollowUpPolicy,
  Platform,
  TaskRunPolicy,
  TaskReviewSurface,
  PostFormat,
  PostStatus,
  Priority,
  ScheduleStrategy,
  TaskStatus,
  
  // Core Models
  Article,
  Project,
  ProjectCreate,
  ProjectMode,
  LiveSiteAuditPage,
  LiveSiteAuditReport,
  LiveSiteAuditSummary,
  LiveSiteGscSyncResult,
  LiveSiteImportResult,
  LiveSiteLinkProfile,
  LiveSiteLinkScanResult,
  LiveSitePage,
  Task,
  TaskArtifact,
  TaskRun,
  
  // GSC Models
  Coverage404Record,
  DriftUrl,
  GscAuthStatus,
  GscDriftReport,
  InspectionRecord,
  MoverMetrics,
  PageMetrics,
  QueryMetrics,
  RedirectRecord,
  RecoveryStats,
  ResubmitCandidate,
  
  // Clarity Models
  ClarityConnectionStatus,
  ClarityExportRowPayload,
  ClarityFindingPayload,
  ClarityPageScorePayload,
  ClaritySkippedFinding,
  ClaritySummaryPayload,
  ClarityTaskCreationResult,
  
  // Reddit Models
  MigrationResult,
  RedditOpportunity,
  RedditStats,
  SubmissionSummary,
  ValidationResult,
  
  // Social Models
  AssetType,
  CampaignStats,
  CampaignStatus,
  CanvasSize,
  ContentTemplate,
  CreateCampaignRequest,
  CreateTemplateRequest,
  OverlayConfig,
  PostMetrics,
  ScheduleConfig,
  SocialCampaign,
  SocialPost,
  SourceConfig,
  SourceType,
  TemplateExample,
  TextPosition,
  VisualAsset,
  
  // Content Models
  ReadabilityReport,
  CompetitorWordCount,
  CompetitorStructure,
  CompetitorSection,
  WordCountComparison,
  KeywordDensityReport,
  SectionPresence,
  ConsecutiveViolation,
  
  // SEO Models
  IntentClassification,
  OpportunityScore,
  ResearchShortlistEntry,
  
  // Cannibalization Models
  ApprovalStatus,
  CalculatorRecommendation,
  CannibalizationSelection,
  CannibalizationStrategy,
  Confidence,
  HubRecommendation,
  MergeRecommendation,
  RecommendationTaskStatus,
  StrategyReview,
  StrategyRisk,
  StrategyWithReviews,
  TerritoryRecommendation,
  
  // CTR Models
  CtrAgentOutput,
  CtrFix,
  CtrFixApplied,
  CtrFixChange,
  CtrFixCheckResult,
  CtrFixPatch,
  CtrFixPatchChanges,
  CtrFixPatchFaqQuestion,
  CtrFixReport,
  CtrFixSkipped,
  CtrFixType,
  CtrFixVerificationReport,
  CtrOutcome,
  CtrRecommendation,
  CtrVerificationReport,
  CtrVerifiedArticle,
  CtrVerifiedFix,
  
  // Queue Models
  EnqueueItem,
  EnqueueMode,
  QueueItem,
  QueueRun,
  QueueSnapshot,
  
  // Workflow
  ExecutionResult,
  FollowUpTask,
  StepProgress,
} from './bindings'

// ─── Frontend-Only Types (not in Rust) ────────────────────────────────────────

export interface KeywordDifficultyEntry {
  keyword: string
  difficulty: number | string | null
  volume?: number | string | null
  traffic?: number | string | null
  topVolume?: number | string | null
  shortage?: number | string | null
  has_data?: boolean
  serp_count?: number
  top_result?: string
  intent?: string | null
  intent_confidence?: number | null
  /** Winnability bucket from the research pipeline SERP enrichment. */
  winnability?: string | null
  /** Human-readable reason for the winnability verdict. */
  winnability_reason?: string | null
  /** Coverage-gap score 0-100 (higher = fills a thinner cluster). */
  gap_score?: number | null
  landing_page_type?: string | null
  opportunity_score?: number | null
  opportunity_reason?: string | null
  proposed_title?: string | null
  /** Cost per click in USD (DataForSEO) — commercial value signal for landing pages. */
  cpc?: number | null
}

export interface KeywordResearchResult {
  themes?: string[]
  total_candidates?: number
  new_keywords: string[]
  filtered_out?: number
  difficulty?: {
    results?: KeywordDifficultyEntry[]
    total?: number
    successful?: number
  } | KeywordDifficultyEntry[] | null
  difficulty_analyzed_count?: number
  difficulty_skipped_keywords?: string[]
}

// ─── SEO Provider Types ───────────────────────────────────────────────────────

export type SeoProvider = 'ahrefs' | 'dataforseo'

export interface SeoProviderConfig {
  provider: SeoProvider
  name: string
  description: string
  requiresCapSolver: boolean
  requiresDataForSeo: boolean
}

// ─── SEO / Ahrefs Types ───────────────────────────────────────────────────────

export interface KeywordIdea {
  keyword: string
  idea_type: 'regular' | 'question'
  difficulty?: string
  volume?: string           // categorical (Ahrefs)
  volume_exact?: number     // precise number (DataForSEO)
  cpc?: number              // cost per click (DataForSEO)
  competition?: number      // competition score 0-1 (DataForSEO)
  country?: string
}

export interface KeywordIdeasResult {
  keyword: string
  country: string
  search_engine: string
  ideas: KeywordIdea[]
  question_ideas: KeywordIdea[]
}

export interface SerpEntry {
  title: string
  url: string
  domain: string
  position: number
}

export interface KeywordDifficultyResult {
  keyword: string
  difficulty: number
  shortage: number
  last_update: string
  serp: SerpEntry[]
}

export interface BacklinkItem {
  anchor: string
  domain_rating: number
  title: string
  url_from: string
  url_to: string
  edu: boolean
  gov: boolean
}

export interface DomainOverview {
  domain_rating?: number
  traffic?: number
  referring_domains?: number
  backlinks?: number
}

export interface BacklinksResult {
  domain: string
  overview?: DomainOverview
  backlinks: BacklinkItem[]
}

export interface TrafficMonthly {
  traffic_monthly_avg: number
  cost_monthly_avg: number
}

export interface TrafficTopPage {
  url?: string
  traffic?: number
  keywords?: number
}

export interface TrafficTopKeyword {
  keyword?: string
  traffic?: number
  position?: number
}

export interface TrafficTopCountry {
  country?: string
  traffic?: number
  share?: number
}

export interface TrafficResult {
  domain: string
  traffic: TrafficMonthly
  traffic_history: Record<string, unknown>[]
  top_pages: TrafficTopPage[]
  top_countries: TrafficTopCountry[]
  top_keywords: TrafficTopKeyword[]
}

// ─── Content Types ────────────────────────────────────────────────────────────

export interface ContentDirResolution {
  selected?: string
  source: string
  has_markdown: boolean
  candidates_searched: string[]
}

export interface CleaningIssue {
  file: string
  issue_type: string
  description: string
  fixed: boolean
}

export interface CleaningResult {
  files_checked: number
  issues: CleaningIssue[]
  issues_fixed: number
}

export interface DateIssue {
  article_id: number
  issue_type: string
  description: string
  current_date: string
}

export interface DateAnalysis {
  total_articles: number
  published_count: number
  future_count: number
  duplicate_count: number
  missing_count: number
  issues: DateIssue[]
  duplicate_dates: [string, number[]][]
}

export interface DateFix {
  article_id: number
  old_date: string
  new_date: string
}

export interface DateFixResult {
  fixes: DateFix[]
  articles_fixed: number
  dry_run: boolean
}

export interface DatePolicyReport {
  total_checked: number
  future_count: number
  duplicate_count: number
  issues: DateIssue[]
  duplicate_dates: [string, number[]][]
}

export interface ArticleLinkProfile {
  id: number
  article_id: number
  title: string
  file: string
  outgoing_ids: number[]
  incoming_ids: number[]
  unresolved_links: string[]
}

export interface LinkScanResult {
  total_articles: number
  total_internal_links: number
  total_links: number
  articles_with_outgoing: number
  articles_with_incoming: number
  orphan_ids: number[]
  profiles: ArticleLinkProfile[]
}

// ─── Publish Articles ─────────────────────────────────────────────────────────

export interface ArticleWithIssue {
  article: import('./bindings').Article
  issue: string
}

export interface YearMismatch {
  article_id: number
  title: string
  title_year: number
  publish_year: number
}

export interface YearMismatchResolution {
  article_id: number
  action: 'update_title' | 'backdate'
  new_value: string
}

export interface PublishPreflightResult {
  ready: import('./bindings').Article[]
  needs_date_fix: ArticleWithIssue[]
  year_mismatches: YearMismatch[]
  blocked: ArticleWithIssue[]
  structural_issue_count: number
  cleaned_stale_count: number
  cleaned_stale_files: string[]
}

export interface PublishedArticle {
  id: number
  title: string
  published_date: string
}

export interface PublishResult {
  published: PublishedArticle[]
  skipped: ArticleWithIssue[]
  errors: string[]
}

// ─── CTR Health Summary ───────────────────────────────────────────────────────

export interface CtrHealthArticle {
  id: number
  title: string
  url_slug: string
  file: string
  healthy: boolean
  audit_status: string
  issues: string[]
  last_audited_at: string | null
  last_audit_issues: string[]
  resolved_issues: string[]
}

export interface CtrHealthSummary {
  total_articles: number
  healthy_count: number
  unhealthy_count: number
  improved_count: number
  already_healthy_count: number
  regressed_count: number
  missing_files: number
  title_issues: number
  meta_issues: number
  snippet_issues: number
  faq_issues: number
  last_audit_at: string | null
  articles: CtrHealthArticle[]
  pending_fix_tasks: number
  completed_audits: number
  open_issues_count: number
}

export interface RepairPathResult {
  checked: number
  repaired: number
  removed: number
  not_found: string[]
}

// ─── Global Task Queue ────────────────────────────────────────────────────────

export interface RunnerItem {
  task: {
    id: string
    title?: string
    type?: string
    projectId?: string
    projectName?: string
  }
  status: 'queued' | 'running' | 'done' | 'failed'
  result?: import('./bindings').ExecutionResult
  error?: string
}

export interface BatchTaskResult {
  task_id: string
  task_type: string
  title: string
  success: boolean
  message: string
}

export interface BatchResult {
  status: 'complete' | 'error' | 'paused'
  processed: number
  errors: BatchTaskResult[]
  results: BatchTaskResult[]
  duration_ms: number
}

export interface BatchSummary {
  total_ready: number
  auto_enqueue: number
  user_enqueue: number
}

// ─── Scheduler ────────────────────────────────────────────────────────────────

export interface SchedulerRule {
  rule_id: string
  project_id: string
  task_type: string
  action: 'create_task' | 'reminder_only'
  interval_hours: number
  priority: 'high' | 'medium' | 'low'
  phase: string
  enabled: boolean
  last_run_at?: string
}

export interface DueRuleResult {
  rule_id: string
  is_due: boolean
  next_due_at: string
  reason: string
}

export interface SchedulerCycleResult {
  started_at: string
  finished_at: string
  project_id: string
  rules_evaluated: number
  tasks_created: number
  errors: string[]
  due_rules: DueRuleResult[]
}

export interface RunSummary {
  run_id: string
  project_id: string
  started_at: string
  finished_at: string
  tasks_processed: number
  tasks_succeeded: number
  tasks_failed: number
  errors: string[]
}

export interface LedgerEvent {
  timestamp: string
  event_type: string
  payload: Record<string, unknown>
}

// ─── Skills & Agent ───────────────────────────────────────────────────────────

export interface Skill {
  name: string
  skill_dir: string
  description: string
  content: string
}

export interface PromptSection {
  label: string
  content: string
}

export interface PromptContext {
  task_id: string
  skill_name: string
  project_id: string
  prompt: string
  word_count: number
  sections: PromptSection[]
}

export interface NormalizedArtifact {
  raw_output: string
  json_artifact: Record<string, unknown> | unknown[] | null
  extraction_method: 'json_block' | 'bare_json' | 'json_line' | 'none'
  success: boolean
}

export interface AgentInfo {
  name: string
  binary: string
  available: boolean
  version?: string
}

export interface AgentStatus {
  available_agents: AgentInfo[]
  configured_provider: string
  token_usage?: {
    prompt_tokens: number
    completion_tokens: number
  }
}

// ─── Skill Vector Search ─────────────────────────────────────────────────────

export interface EmbeddingStatus {
  available: boolean
  model: string
  error?: string
}

export interface ScoredSkill {
  skill: Skill
  score: number
}

// ─── Overview ─────────────────────────────────────────────────────────────────

export interface TaskStatusCounts {
  todo: number
  in_progress: number
  review: number
  done: number
  failed: number
  total: number
}

export interface RecentTask {
  id: string
  title?: string
  task_type: string
  status: string
  updated_at: string
}

export interface ArticleStatusCounts {
  total: number
  published: number
  draft: number
  last_published_date?: string
}

export interface WorkflowActivity {
  task_type: string
  label: string
  last_run_at?: string
  next_due_at?: string
  interval_hours?: number
}

export interface LandingPageResearchPending {
  id: string
  title?: string
  context: string
  themes: string[]
  updated_at: string
}

export interface PendingFeatureSpec {
  id: string
  title?: string
  updated_at: string
}

export interface FixSummary {
  completed: number
  failed: number
  pending: number
  total_found: number
}

export interface HealthSnapshot {
  content_poor: number
  content_needs_improvement: number
  content_good: number
  indexing_not_indexed: number
  ctr_issue_count: number
  cannibalization_clusters: number
  fix_completed: number
  fix_failed: number
  fix_pending: number
  last_audit_days: number
  content_next_run_yield: number
  indexing_next_run_yield: number
  fix_on_cooldown: number
  content_poor_outstanding: number
  content_needs_work_outstanding: number
  fix_needs_review: number
}

export interface ProjectOverview {
  tasks: TaskStatusCounts
  recent_tasks: RecentTask[]
  articles: ArticleStatusCounts
  ready_task_count: number
  workflow_activity: WorkflowActivity[]
  pending_landing_page_research: LandingPageResearchPending[]
  pending_feature_specs: PendingFeatureSpec[]
  fix_summary: FixSummary
  health_snapshot: HealthSnapshot
}

export interface QuickAction {
  task_type: string
  label: string
  description: string
  themes?: string[]
}

// ─── Project Setup / Diagnostics ──────────────────────────────────────────────

export type ContentDirSource =
  | 'workspace_config'
  | 'project_override'
  | 'auto_discovered'
  | 'not_found'

export type SetupSeverity = 'error' | 'warn' | 'info'

export interface ContentDirResult {
  source: ContentDirSource
  path: string | null
  how: string
  file_count: number
}

export interface SetupCheckItem {
  id: string
  severity: SetupSeverity
  title: string
  detail: string
  fix_hint: string | null
  auto_fixable: boolean
}

export interface ProjectConfigFileStatus {
  id: string
  category: string
  label: string
  relative_path: string
  full_path: string
  full_link: string
  used_by: string
  required: boolean
  configured: boolean
  detail: string
}

export interface ProjectSetup {
  project_id: string
  repo_root: string
  automation_dir: string
  workspace_config_path: string
  workspace_config_exists: boolean
  workspace_config: { content_dir?: string; site_url?: string } | null
  articles_json_exists: boolean
  content_dir: ContentDirResult
  checks: SetupCheckItem[]
  is_valid: boolean
  summary: string
}

export interface ContentHealthResult {
  checked: number
  content_files: number
  date_mismatches: number
  fixable_mismatches: number
  mismatch_details: string[]
  orphan_files: string[]
  fixed: boolean
  dates_synced: number
}

export interface FormatIssue {
  file: string
  issue_type: string
  field: string | null
  severity: 'error' | 'warn' | 'info'
  message: string
  auto_fixable: boolean
}

export interface FormatValidationResult {
  files_checked: number
  issues: FormatIssue[]
  error_count: number
  warn_count: number
  info_count: number
  auto_fixable_count: number
}

export interface FormatFixResult {
  files_checked: number
  files_fixed: number
  issues_remaining: FormatIssue[]
}

export interface IngestOrphanResult {
  ingested: number
  files: string[]
}

// ─── Import / Export ──────────────────────────────────────────────────────────

export interface ImportResult {
  tasks_imported: number
  articles_imported: number
}

// ─── Secrets ──────────────────────────────────────────────────────────────────

export interface SecretStatus {
  key: string
  description: string
  configured: boolean
  source?: string
}

export interface SecretsStatus {
  secrets: SecretStatus[]
  secrets_file_exists: boolean
  secrets_file_path: string
}

// ─── View Type ─────────────────────────────────────────────────────────────────

export type View =
  | 'overview'
  | 'tasks'
  | 'articles'
  | 'reddit'
  | 'gsc'
  | 'clarity'
  | 'seo'
  | 'social'
  | 'cannibalization'
  | 'settings'
  | 'scheduler'
  | 'history'
  | 'health'

// ─── Investigation ──────────────────────────────────────────────────────────

export interface InvestigationResult {
  id: string
  question: string
  answer: string
  summary: string
  evidence: unknown
  findings: Array<{
    title: string
    description: string
    evidence?: string
    severity: 'critical' | 'warning' | 'info'
    fix_type: 'auto_fixable' | 'developer_actionable' | 'hybrid' | 'informational'
    auto_fix_task?: string
  }>
  created_at: string
}
