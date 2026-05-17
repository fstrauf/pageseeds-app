import { useState, useEffect, useCallback } from 'react'
import { Plus, Trash2, RefreshCw, Play, ToggleLeft, ToggleRight } from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import {
  listSchedulerRules,
  upsertSchedulerRule,
  deleteSchedulerRule,
  setSchedulerRuleEnabled,
  runSchedulerCycle,
} from '../../lib/tauri'
import type { SchedulerCycleResult, SchedulerRule } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn, formatDate } from '../../lib/utils'

interface SchedulerConfigProps {
  projectId: string
}

const PHASES = ['collection', 'investigation', 'research', 'implementation', 'verification']
const PRIORITIES = ['high', 'medium', 'low']
const DEFAULT_TASK_TYPES = [
  'investigate_gsc',
  'research_keywords',
  'reddit_opportunity_search',
  'write_article',
  'content_cleanup',
  'ctr_audit',
  'cannibalization_audit',
]

function RuleRow({
  rule,
  onToggle,
  onDelete,
}: {
  rule: SchedulerRule
  onToggle: (enabled: boolean) => void
  onDelete: () => void
}) {
  return (
    <div className="flex items-center gap-3 px-3 py-2 text-sm border-b border-border last:border-b-0">
      <div className="flex-1 min-w-0">
        <span className="font-medium text-foreground">{rule.task_type.replace(/_/g, ' ')}</span>
        <span className="ml-2 text-xs text-muted-foreground">{rule.phase}</span>
      </div>
      <span className="text-xs text-muted-foreground shrink-0">every {rule.interval_hours}h</span>
      <Badge
        variant="outline"
        className={cn('text-xs shrink-0', rule.priority === 'high' && 'border-red-300 text-red-600')}
      >
        {rule.priority}
      </Badge>
      {rule.last_run_at && (
        <span className="text-xs text-muted-foreground shrink-0 hidden lg:block">
          Last: {formatDate(rule.last_run_at)}
        </span>
      )}
      <button
        onClick={() => onToggle(!rule.enabled)}
        title={rule.enabled ? 'Disable' : 'Enable'}
        className={cn('shrink-0 text-sm transition-colors', rule.enabled ? 'text-green-500' : 'text-muted-foreground')}
      >
        {rule.enabled ? <ToggleRight size={14} /> : <ToggleLeft size={14} />}
      </button>
      <button
        onClick={onDelete}
        className="shrink-0 text-muted-foreground hover:text-destructive transition-colors"
        title="Delete rule"
      >
        <Trash2 size={13} />
      </button>
    </div>
  )
}

function NewRuleForm({
  projectId,
  onSaved,
}: {
  projectId: string
  onSaved: () => void
}) {
  const [taskType, setTaskType] = useState(DEFAULT_TASK_TYPES[0])
  const [intervalHours, setIntervalHours] = useState(168)
  const [priority, setPriority] = useState('medium')
  const [phase, setPhase] = useState('collection')
  const [saving, setSaving] = useState(false)

  async function save() {
    setSaving(true)
    const rule: SchedulerRule = {
      rule_id: `${taskType}-${Date.now()}`,
      project_id: projectId,
      task_type: taskType,
      action: 'create_task',
      interval_hours: intervalHours,
      priority: priority as SchedulerRule['priority'],
      phase,
      enabled: true,
    }
    await upsertSchedulerRule(rule)
    setSaving(false)
    onSaved()
  }

  return (
    <div className="rounded border border-dashed border-border p-3 flex flex-col gap-3">
      <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">Add Rule</p>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Task type</label>
          <input
            list="task-type-list"
            value={taskType}
            onChange={e => setTaskType(e.target.value)}
            className="rounded border border-border bg-input text-foreground text-sm px-2 py-1"
          />
          <datalist id="task-type-list">
            {DEFAULT_TASK_TYPES.map(t => <option key={t} value={t} />)}
          </datalist>
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Interval (hours)</label>
          <input
            type="number"
            min={1}
            value={intervalHours}
            onChange={e => setIntervalHours(Number(e.target.value))}
            className="rounded border border-border bg-input text-foreground text-sm px-2 py-1"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Priority</label>
          <select
            value={priority}
            onChange={e => setPriority(e.target.value)}
            className="rounded border border-border bg-input text-foreground text-sm px-2 py-1"
          >
            {PRIORITIES.map(p => <option key={p}>{p}</option>)}
          </select>
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Phase</label>
          <select
            value={phase}
            onChange={e => setPhase(e.target.value)}
            className="rounded border border-border bg-input text-foreground text-sm px-2 py-1"
          >
            {PHASES.map(p => <option key={p}>{p}</option>)}
          </select>
        </div>
      </div>
      <Button size="sm" className="self-start" disabled={saving} onClick={save}>
        <Plus size={13} className="mr-1.5" />
        {saving ? 'Saving…' : 'Add Rule'}
      </Button>
    </div>
  )
}

export function SchedulerConfig({ projectId }: SchedulerConfigProps) {
  const { showError } = useErrorHandler()
  const [rules, setRules] = useState<SchedulerRule[]>([])
  const [cycleResult, setCycleResult] = useState<SchedulerCycleResult | null>(null)
  const [running, setRunning] = useState(false)
  const [showAdd, setShowAdd] = useState(false)

  const load = useCallback(async () => {
    try {
      const data = await listSchedulerRules(projectId)
      setRules(data)
    } catch (e: unknown) {
      showError(String(e))
    }
  }, [projectId, showError])

  useEffect(() => {
    if (projectId) load()
  }, [projectId, load])

  async function toggle(rule: SchedulerRule, enabled: boolean) {
    await setSchedulerRuleEnabled(rule.rule_id, enabled)
    await load()
  }

  async function remove(rule: SchedulerRule) {
    await deleteSchedulerRule(rule.rule_id)
    await load()
  }

  async function triggerCycle() {
    setRunning(true)
    setCycleResult(null)
    try {
      const r = await runSchedulerCycle(projectId)
      setCycleResult(r)
      await load()
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setRunning(false)
    }
  }

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-foreground mb-1">Scheduler</h2>
            <p className="text-xs text-muted-foreground">
              Rules evaluated on a background timer. Tasks are created when a rule is due.
            </p>
          </div>
          <div className="flex gap-2 shrink-0">
            <button
              onClick={load}
              className="text-muted-foreground hover:text-foreground transition-colors"
              title="Refresh"
            >
              <RefreshCw size={14} />
            </button>
            <Button size="sm" variant="outline" disabled={running} onClick={triggerCycle}>
              <Play size={13} className="mr-1.5" />
              {running ? 'Running…' : 'Run Cycle'}
            </Button>
            <Button size="sm" onClick={() => setShowAdd(v => !v)}>
              <Plus size={13} className="mr-1.5" />
              Add Rule
            </Button>
          </div>
        </div>

        {showAdd && (
          <NewRuleForm
            projectId={projectId}
            onSaved={() => {
              setShowAdd(false)
              load()
            }}
          />
        )}

        {/* Rules table */}
        <div className="rounded border border-border overflow-hidden">
          {rules.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">No rules configured.</p>
          ) : (
            rules.map(rule => (
              <RuleRow
                key={rule.rule_id}
                rule={rule}
                onToggle={enabled => toggle(rule, enabled)}
                onDelete={() => remove(rule)}
              />
            ))
          )}
        </div>

        {/* Cycle result */}
        {cycleResult && (
          <div className="rounded border border-border bg-card p-4 flex flex-col gap-2">
            <p className="text-sm font-medium text-foreground">Last Cycle Result</p>
            <div className="grid grid-cols-3 gap-3 text-center">
              {[
                { label: 'Rules', value: cycleResult.rules_evaluated },
                { label: 'Created', value: cycleResult.tasks_created },
                { label: 'Errors', value: cycleResult.errors.length },
              ].map(({ label, value }) => (
                <div key={label} className="rounded border border-border p-2">
                  <div className="text-lg font-bold text-foreground">{value}</div>
                  <div className="text-xs text-muted-foreground">{label}</div>
                </div>
              ))}
            </div>
            {cycleResult.due_rules.filter(r => r.is_due).length > 0 && (
              <div>
                <p className="text-xs font-medium text-muted-foreground mt-2 mb-1">Due rules:</p>
                {cycleResult.due_rules.filter(r => r.is_due).map(r => (
                  <p key={r.rule_id} className="text-xs text-foreground">{r.rule_id}</p>
                ))}
              </div>
            )}
            {cycleResult.errors.length > 0 && (
              <div>
                <p className="text-xs font-medium text-destructive mt-2 mb-1">Errors:</p>
                {cycleResult.errors.map((e, i) => (
                  <p key={i} className="text-xs text-muted-foreground">{e}</p>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
