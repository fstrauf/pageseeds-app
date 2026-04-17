import { useCallback, useEffect, useState } from 'react'
import { checkProjectSetup, fixDateMismatches, getContentHealth, ingestOrphanArticles, initWorkspaceConfig, initializeProjectWorkspace } from '../../lib/tauri'
import type { ContentHealthResult, ProjectSetup, SetupCheckItem, SetupSeverity } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { ActionDrawer } from '@/components/ui/action-drawer'
import { useActionRun } from '../../hooks/useActionRun'
import type { ActionResultPayload } from '../../hooks/useActionRun'

interface Props {
  projectId: string
  onViewChange?: (view: string) => void
}

const SEVERITY_STYLES: Record<SetupSeverity, string> = {
  error: 'bg-red-50 border-red-200 text-red-900 dark:bg-red-950/30 dark:border-red-800 dark:text-red-100',
  warn: 'bg-yellow-50 border-yellow-200 text-yellow-900 dark:bg-yellow-950/30 dark:border-yellow-800 dark:text-yellow-100',
  info: 'bg-blue-50 border-blue-200 text-blue-900 dark:bg-blue-950/30 dark:border-blue-800 dark:text-blue-100',
}

const SEVERITY_ICON: Record<SetupSeverity, string> = {
  error: '✕',
  warn: '⚠',
  info: 'ℹ',
}

function CheckBanner({
  item,
  setup,
  onFixed,
  onRun,
}: {
  item: SetupCheckItem
  setup: ProjectSetup
  onFixed: () => void
  onRun: (label: string, fn: () => Promise<ActionResultPayload>) => void
}) {
  const [dismissed, setDismissed] = useState(false)

  if (dismissed) return null

  // Check if this is a missing config file that can be fixed by full initialization
  const needsFullInit = item.id === 'automation_dir_missing' || 
    item.id === 'articles_json_missing' ||
    item.id === 'workspace_config_missing' ||
    item.id === 'project_md_missing' ||
    item.id === 'reddit_config_missing' ||
    item.id === 'reply_guardrails_missing'

  function handleFix() {
    onRun('Creating workspace config', async () => {
      const contentDir =
        setup.content_dir.path ??
        setup.workspace_config?.content_dir ??
        'src/blog/posts'
      const siteUrl = setup.workspace_config?.site_url ?? setup.project_id
      const path = await initWorkspaceConfig(setup.project_id, contentDir, siteUrl)
      onFixed()
      return { kind: 'message' as const, success: true, text: `Config written to ${path}` }
    })
  }

  function handleInitialize() {
    onRun('Initializing project workspace', async () => {
      const created = await initializeProjectWorkspace(setup.project_id)
      onFixed()
      if (created.length === 0) {
        return { 
          kind: 'message' as const, 
          success: true, 
          text: 'All required files already exist. No changes needed.'
        }
      }
      // Format the created list nicely
      const fileList = created.map(f => `• ${f}`).join('\n')
      return { 
        kind: 'message' as const, 
        success: true, 
        text: `Created ${created.length} file(s):\n${fileList}`
      }
    })
  }

  return (
    <div
      className={`flex items-start gap-3 rounded-md border px-3 py-2.5 text-sm ${SEVERITY_STYLES[item.severity]}`}
    >
      <span className="mt-0.5 shrink-0 font-semibold">{SEVERITY_ICON[item.severity]}</span>
      <div className="flex-1 min-w-0">
        <span className="font-medium">{item.title}</span>
        <span className="ml-2 opacity-75">{item.detail}</span>
        {item.fix_hint && (
          <div className="mt-0.5 text-xs opacity-60">{item.fix_hint}</div>
        )}
      </div>
      {/* Full initialization for structural issues */}
      {needsFullInit && (
        <Button
          size="sm"
          variant="outline"
          className="shrink-0 h-6 px-2 text-xs border-primary text-primary hover:bg-primary/10"
          onClick={handleInitialize}
        >
          Initialize Project
        </Button>
      )}
      {/* Single config creation for workspace config content issues only */}
      {item.auto_fixable && !needsFullInit && (
        <Button
          size="sm"
          variant="outline"
          className="shrink-0 h-6 px-2 text-xs"
          onClick={handleFix}
        >
          Create config
        </Button>
      )}
      {item.severity !== 'error' && (
        <button
          className="shrink-0 opacity-40 hover:opacity-70 text-xs leading-none pt-0.5"
          aria-label="Dismiss"
          onClick={() => setDismissed(true)}
        >
          ✕
        </button>
      )}
    </div>
  )
}

function DateMismatchBanner({
  health,
  projectId,
  isRunning,
  onFixed,
  onRun,
}: {
  health: ContentHealthResult
  projectId: string
  isRunning: boolean
  onFixed: () => void
  onRun: (label: string, fn: () => Promise<ActionResultPayload>) => void
}) {
  const [dismissed, setDismissed] = useState(false)

  if (dismissed) return null

  function handleFix() {
    onRun('Fixing date mismatches', async () => {
      const r = await fixDateMismatches(projectId)
      onFixed()
      return {
        kind: 'summary' as const,
        success: true,
        items: [
          { label: 'Articles checked', value: String(r.checked) },
          { label: 'Dates fixed', value: String(r.date_mismatches) },
        ],
      }
    })
  }

  const preview = health.mismatch_details.slice(0, 3).join(', ')
  const extra = health.mismatch_details.length > 3 ? ` +${health.mismatch_details.length - 3} more` : ''

  return (
    <div className={`flex items-start gap-3 rounded-md border px-3 py-2.5 text-sm ${SEVERITY_STYLES.warn}`}>
      <span className="mt-0.5 shrink-0 font-semibold">{SEVERITY_ICON.warn}</span>
      <div className="flex-1 min-w-0">
        <span className="font-medium">
          {health.date_mismatches} article{health.date_mismatches !== 1 ? 's have' : ' has'} date mismatches
        </span>
        <span className="ml-2 opacity-90">
          Frontmatter dates differ from articles.json
        </span>
        {preview && (
          <div className="mt-0.5 text-xs opacity-80">{preview}{extra}</div>
        )}
      </div>
      <Button
        size="sm"
        variant="outline"
        className="shrink-0 h-6 px-2 text-xs"
        onClick={handleFix}
        disabled={isRunning}
      >
        {isRunning ? 'Fixing…' : 'Fix dates'}
      </Button>
      <button
        className="shrink-0 opacity-40 hover:opacity-70 text-xs leading-none pt-0.5"
        aria-label="Dismiss"
        onClick={() => setDismissed(true)}
      >
        ✕
      </button>
    </div>
  )
}

function OrphanFilesBanner({
  health,
  projectId,
  isRunning,
  onFixed,
  onRun,
  onViewChange,
}: {
  health: ContentHealthResult
  projectId: string
  isRunning: boolean
  onFixed: () => void | Promise<void>
  onRun: (label: string, fn: () => Promise<ActionResultPayload>, nextStep?: { view: string; label: string }) => void
  onViewChange?: (view: string) => void
}) {
  const [dismissed, setDismissed] = useState(false)
  const [expanded, setExpanded] = useState(false)

  if (dismissed) return null

  const count = health.orphan_files.length
  const preview = health.orphan_files.slice(0, 3)
  const extra = count > 3 ? count - 3 : 0

  function handleImport() {
    onRun('Importing untracked files', async () => {
      const r = await ingestOrphanArticles(projectId)
      await onFixed()
      return {
        kind: 'summary' as const,
        success: true,
        items: [
          { label: 'Files imported', value: String(r.ingested) },
        ],
      }
    }, onViewChange ? { view: 'articles', label: 'View imported articles' } : undefined)
  }

  return (
    <div className={`flex items-start gap-3 rounded-md border px-3 py-2.5 text-sm ${SEVERITY_STYLES.warn}`}>
      <span className="mt-0.5 shrink-0 font-semibold">{SEVERITY_ICON.warn}</span>
      <div className="flex-1 min-w-0">
        <span className="font-medium">
          {count} MDX file{count !== 1 ? 's' : ''} not tracked in articles.json
        </span>
        <span className="ml-2 opacity-90">These won't appear in the publish flow</span>
        <button
          className="ml-2 text-xs underline opacity-70 hover:opacity-100"
          onClick={() => setExpanded(e => !e)}
        >
          {expanded ? 'hide' : 'show'}
        </button>
        {expanded && (
          <div className="mt-1 space-y-0.5">
            {preview.map(f => (
              <div key={f} className="text-xs font-mono opacity-80 truncate">{f}</div>
            ))}
            {extra > 0 && <div className="text-xs opacity-60">…and {extra} more</div>}
          </div>
        )}
      </div>
      <Button
        size="sm"
        variant="outline"
        className="shrink-0 h-6 px-2 text-xs"
        onClick={handleImport}
        disabled={isRunning}
      >
        {isRunning ? 'Importing…' : `Import ${count}`}
      </Button>
      <button
        className="shrink-0 opacity-40 hover:opacity-70 text-xs leading-none pt-0.5"
        aria-label="Dismiss"
        onClick={() => setDismissed(true)}
      >
        ✕
      </button>
    </div>
  )
}

export function SetupWarnings({ projectId, onViewChange }: Props) {
  const [setup, setSetup] = useState<ProjectSetup | null>(null)
  const [health, setHealth] = useState<ContentHealthResult | null>(null)
  const { state: actionState, run: runAction, dismiss: dismissAction } = useActionRun()

  const load = useCallback(async () => {
    try {
      const [setupData, healthData] = await Promise.all([
        checkProjectSetup(projectId),
        getContentHealth(projectId).catch(() => null),
      ])
      setSetup(setupData)
      setHealth(healthData)
    } catch {
      // Silent — don't disturb the UI if diagnostics fail
    }
  }, [projectId])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    load()
  }, [load])

  if (!setup) return null

  // Only show error and warn items — info is noise at this level
  const visible = setup.checks.filter(
    (c) => c.severity === 'error' || c.severity === 'warn',
  )
  
  // Check if there are any missing config files that would benefit from initialization
  // This includes: automation_dir, articles.json, workspace_config, project.md, reddit_config, etc.
  const hasMissingConfigFiles = setup.checks.some(
    (c) => c.id === 'automation_dir_missing' || 
       c.id === 'articles_json_missing' ||
       c.id === 'workspace_config_missing' ||
       c.id === 'workspace_config_no_content_dir' ||
       c.id === 'content_dir_not_found' ||
       c.id === 'content_dir_auto_discovered' ||
       c.id === 'project_md_missing' ||
       c.id === 'reddit_config_missing' ||
       c.id === 'reply_guardrails_missing'
  )

  const showDateMismatch = health && health.date_mismatches > 0 && setup.is_valid
  const showOrphans = health && health.orphan_files.length > 0 && setup.is_valid

  if (visible.length === 0 && !showDateMismatch && !showOrphans && actionState.status === 'idle') return null

  function handleBulkInitialize() {
    runAction('Initializing project workspace', async () => {
      const created = await initializeProjectWorkspace(projectId)
      await load()
      if (created.length === 0) {
        return { 
          kind: 'message' as const, 
          success: true, 
          text: 'All required files already exist. No changes needed.'
        }
      }
      // Format the created list nicely
      const fileList = created.map(f => `• ${f}`).join('\n')
      return { 
        kind: 'message' as const, 
        success: true, 
        text: `Created ${created.length} file(s):\n${fileList}`
      }
    })
  }

  return (
    <>
      <div className="space-y-1.5">
        {/* Bulk initialization banner for missing config files */}
        {hasMissingConfigFiles && (
          <div className="flex items-center gap-3 rounded-md border px-3 py-2.5 text-sm bg-blue-50 border-blue-200 text-blue-900 dark:bg-blue-950/30 dark:border-blue-800 dark:text-blue-100">
            <span className="mt-0.5 shrink-0 font-semibold">🚀</span>
            <div className="flex-1 min-w-0">
              <span className="font-medium">Project needs initialization</span>
              <span className="ml-2 opacity-75">
                Missing required configuration files. Click to auto-create them.
              </span>
            </div>
            <Button
              size="sm"
              className="shrink-0 h-6 px-3 text-xs bg-blue-600 text-white hover:bg-blue-700"
              onClick={handleBulkInitialize}
              disabled={actionState.status === 'running'}
            >
              {actionState.status === 'running' ? 'Initializing…' : 'Initialize Project'}
            </Button>
          </div>
        )}
        
        {visible.map((item) => (
          <CheckBanner key={item.id} item={item} setup={setup} onFixed={load} onRun={runAction} />
        ))}
        {showDateMismatch && health && (
          <DateMismatchBanner health={health} projectId={projectId} isRunning={actionState.status === 'running'} onFixed={load} onRun={runAction} />
        )}
        {showOrphans && health && (
          <OrphanFilesBanner health={health} projectId={projectId} isRunning={actionState.status === 'running'} onFixed={load} onRun={runAction} onViewChange={onViewChange} />
        )}
      </div>
      <ActionDrawer state={actionState} onDismiss={dismissAction} onNavigate={onViewChange} />
    </>
  )
}
