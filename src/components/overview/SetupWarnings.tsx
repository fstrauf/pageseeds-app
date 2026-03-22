import React, { useCallback, useEffect, useState } from 'react'
import { checkProjectSetup, fixDateMismatches, getContentHealth, initWorkspaceConfig } from '../../lib/tauri'
import type { ContentHealthResult, ProjectSetup, SetupCheckItem, SetupSeverity } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { ActionDrawer } from '@/components/ui/action-drawer'
import { useActionRun } from '../../hooks/useActionRun'
import type { ActionResultPayload } from '../../hooks/useActionRun'

interface Props {
  projectId: string
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
      {item.auto_fixable && (
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

export function SetupWarnings({ projectId }: Props) {
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
    load()
  }, [load])

  if (!setup) return null

  // Only show error and warn items — info is noise at this level
  const visible = setup.checks.filter(
    (c) => c.severity === 'error' || c.severity === 'warn',
  )

  const showDateMismatch = health && health.date_mismatches > 0 && setup.is_valid

  if (visible.length === 0 && !showDateMismatch && actionState.status === 'idle') return null

  return (
    <>
      <div className="space-y-1.5">
        {visible.map((item) => (
          <CheckBanner key={item.id} item={item} setup={setup} onFixed={load} onRun={runAction} />
        ))}
        {showDateMismatch && health && (
          <DateMismatchBanner health={health} projectId={projectId} isRunning={actionState.status === 'running'} onFixed={load} onRun={runAction} />
        )}
      </div>
      <ActionDrawer state={actionState} onDismiss={dismissAction} />
    </>
  )
}
