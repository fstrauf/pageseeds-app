import { useState } from 'react'
import { CheckCircle2, AlertTriangle, Loader2, XCircle, Bot, Send, X } from 'lucide-react'
import {
  preflightPublishArticles,
  applyPublishArticles,
  resolveYearMismatchAgent,
} from '../../lib/tauri'
import type {
  Article,
  PublishPreflightResult,
  PublishResult,
  YearMismatch,
  YearMismatchResolution,
} from '../../lib/types'
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
  SheetFooter,
  SheetClose,
} from '@/components/ui/sheet'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'

// ─── Types ────────────────────────────────────────────────────────────────────

type PanelState =
  | { kind: 'idle' }
  | { kind: 'preflight_running' }
  | { kind: 'preflight_done'; result: PublishPreflightResult; resolutions: Map<number, YearMismatchResolution>; resolving: Set<number> }
  | { kind: 'publishing' }
  | { kind: 'done'; result: PublishResult }
  | { kind: 'error'; message: string }

function articleIdNumber(id: number | bigint): number {
  return Number(id)
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function ArticleRow({ article, badge, badgeClass, detail }: {
  article: Article
  badge: string
  badgeClass: string
  detail?: string
}) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <Badge className={`text-xs shrink-0 mt-0.5 ${badgeClass}`}>{badge}</Badge>
      <div className="flex-1 min-w-0">
        <div className="text-xs text-foreground truncate">{article.title || article.url_slug}</div>
        {detail && <div className="text-xs text-muted-foreground mt-0.5">{detail}</div>}
      </div>
    </div>
  )
}

function YearMismatchRow({
  mismatch,
  resolution,
  resolving,
  onResolve,
}: {
  mismatch: YearMismatch
  resolution?: YearMismatchResolution
  resolving: boolean
  onResolve: (m: YearMismatch) => void
}) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <Badge className="text-xs shrink-0 mt-0.5 bg-amber-100 text-amber-700 border-transparent">
        year mismatch
      </Badge>
      <div className="flex-1 min-w-0">
        <div className="text-xs text-foreground truncate">{mismatch.title}</div>
        <div className="text-xs text-muted-foreground mt-0.5">
          Title year {mismatch.title_year} vs publish year {mismatch.publish_year}
        </div>
        {resolution && (
          <div className="text-xs text-emerald-700 mt-0.5">
            {resolution.action === 'update_title'
              ? `→ New title: "${resolution.new_value}"`
              : `→ Backdate to: ${resolution.new_value}`}
          </div>
        )}
      </div>
      {!resolution && (
        <Button
          variant="outline"
          size="sm"
          onClick={() => onResolve(mismatch)}
          disabled={resolving}
          className="h-6 text-xs border-border shrink-0"
        >
          {resolving ? <Loader2 size={11} className="animate-spin" /> : <Bot size={11} />}
          {resolving ? 'Resolving…' : 'Resolve with AI'}
        </Button>
      )}
    </div>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────

interface PublishPanelProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  projectId: string
  candidates: Article[]
  onPublished: () => void
}

export function PublishPanel({
  open,
  onOpenChange,
  projectId,
  candidates,
  onPublished,
}: PublishPanelProps) {
  const [state, setState] = useState<PanelState>({ kind: 'idle' })

  function handleOpenChange(nextOpen: boolean) {
    if (!nextOpen) {
      setState({ kind: 'idle' })
    }
    onOpenChange(nextOpen)
  }

  async function runPreflight() {
    setState({ kind: 'preflight_running' })
    try {
      const ids = candidates.map(a => articleIdNumber(a.id))
      const result = await preflightPublishArticles(projectId, ids)
      setState({
        kind: 'preflight_done',
        result,
        resolutions: new Map(),
        resolving: new Set(),
      })
    } catch (e: unknown) {
      setState({ kind: 'error', message: String(e) })
    }
  }

  async function handleResolve(mismatch: YearMismatch) {
    if (state.kind !== 'preflight_done') return
    setState({
      ...state,
      resolving: new Set([...state.resolving, mismatch.article_id]),
    })
    try {
      const resolution = await resolveYearMismatchAgent(
        projectId,
        mismatch.article_id,
        mismatch.title,
        mismatch.title_year,
        mismatch.publish_year,
      )
      setState(prev => {
        if (prev.kind !== 'preflight_done') return prev
        const newResolutions = new Map(prev.resolutions)
        newResolutions.set(mismatch.article_id, resolution)
        const newResolving = new Set(prev.resolving)
        newResolving.delete(mismatch.article_id)
        return { ...prev, resolutions: newResolutions, resolving: newResolving }
      })
    } catch {
      setState(prev => {
        if (prev.kind !== 'preflight_done') return prev
        const newResolving = new Set(prev.resolving)
        newResolving.delete(mismatch.article_id)
        return { ...prev, resolving: newResolving }
      })
    }
  }

  async function handlePublish() {
    if (state.kind !== 'preflight_done') return
    const { result, resolutions } = state

    // Collect article IDs to publish: ready + year_mismatch (if resolved) + needs_date_fix
    const publishIds: number[] = [
      ...result.ready.map(a => articleIdNumber(a.id)),
      ...result.needs_date_fix.map(wi => articleIdNumber(wi.article.id)),
      ...result.year_mismatches
        .filter(m => resolutions.has(m.article_id))
        .map(m => m.article_id),
    ]

    if (publishIds.length === 0) {
      setState({ kind: 'error', message: 'No articles are ready to publish.' })
      return
    }

    // Build date_fixes map from the date analysis (auto-fixable dates)
    // We pass an empty map — the Rust apply_publish handles date redistribution via calculate_fixes
    // for articles that already have future/duplicate dates noted in needs_date_fix
    const dateFixes: Record<string, string> = {}
    // Pass all date-fix articles in the IDs so apply_publish can auto-fix them
    const yearResolutions: YearMismatchResolution[] = Array.from(resolutions.values())

    setState({ kind: 'publishing' })
    try {
      const publishResult = await applyPublishArticles(
        projectId,
        publishIds,
        dateFixes,
        yearResolutions,
      )
      setState({ kind: 'done', result: publishResult })
      onPublished()
    } catch (e: unknown) {
      setState({ kind: 'error', message: String(e) })
    }
  }

  // ─── Derived state ──────────────────────────────────────────────────────────

  const allMismatchesResolved =
    state.kind === 'preflight_done' &&
    state.result.year_mismatches.every(m => state.resolutions.has(m.article_id))

  const publishableCount =
    state.kind === 'preflight_done'
      ? state.result.ready.length +
        state.result.needs_date_fix.length +
        state.result.year_mismatches.filter(m => state.resolutions.has(m.article_id)).length
      : 0

  // ─── Render ─────────────────────────────────────────────────────────────────

  return (
    <Sheet open={open} onOpenChange={handleOpenChange}>
      <SheetContent side="right" className="w-[min(100vw,42rem)] max-w-[100vw] p-0 overflow-hidden [&>button:last-child]:hidden">
        <div className="h-full flex flex-col overflow-hidden">
        <SheetHeader className="shrink-0 flex-row items-center gap-3 px-5 py-4 border-b border-border min-w-0">
          <div className="flex-1 min-w-0">
            <SheetTitle className="text-sm font-semibold">Publish Articles</SheetTitle>
            <SheetDescription className="text-xs text-muted-foreground mt-0.5">
              {candidates.length} article{candidates.length !== 1 ? 's' : ''} ready to review
            </SheetDescription>
          </div>
          <SheetClose asChild>
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground shrink-0">
              <X size={14} />
            </Button>
          </SheetClose>
        </SheetHeader>

        <div className="flex-1 overflow-y-auto px-5 py-4">
          <div className="space-y-4">

            {/* Idle state */}
            {state.kind === 'idle' && (
              <div className="space-y-3">
                <p className="text-xs text-muted-foreground">
                  Pre-flight checks will verify content files exist, detect date issues, and flag title/year mismatches.
                  All fixes are deterministic — the AI step is only used if year mismatches need editorial judgment.
                </p>
                <div className="space-y-1">
                  {candidates.map(a => (
                    <div key={a.id} className="flex items-center gap-2 py-1">
                      <Badge className="text-xs bg-secondary text-secondary-foreground border-transparent">
                        {a.status}
                      </Badge>
                      <span className="text-xs text-foreground truncate">{a.title || a.url_slug}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Running preflight */}
            {state.kind === 'preflight_running' && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground py-4">
                <Loader2 size={14} className="animate-spin" />
                Running pre-flight checks…
              </div>
            )}

            {/* Preflight done */}
            {state.kind === 'preflight_done' && (() => {
              const { result, resolutions, resolving } = state
              return (
                <div className="space-y-4">
                  {result.structural_issue_count > 0 && (
                    <div className="px-3 py-2 rounded-md text-xs bg-amber-50 text-amber-700 border border-amber-200">
                      {result.structural_issue_count} structural issue{result.structural_issue_count !== 1 ? 's' : ''} found
                      (duplicate headings, blank-line gaps) — will be auto-fixed on publish.
                    </div>
                  )}

                  {result.blocked.length > 0 && (
                    <div className="space-y-1">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-destructive mb-1">
                        <XCircle size={12} />
                        Blocked ({result.blocked.length})
                      </div>
                      {result.blocked.map(wi => (
                        <ArticleRow
                          key={wi.article.id}
                          article={wi.article}
                          badge="blocked"
                          badgeClass="bg-destructive/15 text-destructive border-transparent"
                          detail={wi.issue}
                        />
                      ))}
                      <Separator className="mt-2" />
                    </div>
                  )}

                  {result.year_mismatches.length > 0 && (
                    <div className="space-y-1">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-amber-700 mb-1">
                        <Bot size={12} />
                        Year mismatches — needs AI ({result.year_mismatches.length})
                      </div>
                      {result.year_mismatches.map(m => (
                        <YearMismatchRow
                          key={m.article_id}
                          mismatch={m}
                          resolution={resolutions.get(m.article_id)}
                          resolving={resolving.has(m.article_id)}
                          onResolve={handleResolve}
                        />
                      ))}
                      <Separator className="mt-2" />
                    </div>
                  )}

                  {result.needs_date_fix.length > 0 && (
                    <div className="space-y-1">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-sky-700 mb-1">
                        <AlertTriangle size={12} />
                        Date issues — auto-fixable ({result.needs_date_fix.length})
                      </div>
                      {result.needs_date_fix.map(wi => (
                        <ArticleRow
                          key={wi.article.id}
                          article={wi.article}
                          badge="date fix"
                          badgeClass="bg-sky-100 text-sky-700 border-transparent"
                          detail={wi.issue}
                        />
                      ))}
                      <Separator className="mt-2" />
                    </div>
                  )}

                  {result.ready.length > 0 && (
                    <div className="space-y-1">
                      <div className="flex items-center gap-1.5 text-xs font-medium text-emerald-700 mb-1">
                        <CheckCircle2 size={12} />
                        Ready ({result.ready.length})
                      </div>
                      {result.ready.map(a => (
                        <ArticleRow
                          key={a.id}
                          article={a}
                          badge="ready"
                          badgeClass="bg-emerald-100 text-emerald-700 border-transparent"
                          detail={a.published_date ?? undefined}
                        />
                      ))}
                    </div>
                  )}

                  {result.ready.length === 0 &&
                    result.needs_date_fix.length === 0 &&
                    result.year_mismatches.length === 0 && (
                    <div className="text-xs text-muted-foreground py-2">
                      All candidates are blocked. Resolve the issues above before publishing.
                    </div>
                  )}
                </div>
              )
            })()}

            {/* Publishing */}
            {state.kind === 'publishing' && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground py-4">
                <Loader2 size={14} className="animate-spin" />
                Publishing articles and patching content files…
              </div>
            )}

            {/* Done */}
            {state.kind === 'done' && (
              <div className="space-y-3">
                <div className="flex items-center gap-2 text-sm text-emerald-700">
                  <CheckCircle2 size={15} />
                  {state.result.published.length} article{state.result.published.length !== 1 ? 's' : ''} published
                </div>
                {state.result.published.length > 0 && (
                  <div className="space-y-1">
                    {state.result.published.map(p => (
                      <div key={p.id} className="flex items-center gap-2 text-xs">
                        <Badge className="text-xs bg-emerald-100 text-emerald-700 border-transparent">published</Badge>
                        <span className="flex-1 truncate text-foreground">{p.title}</span>
                        <span className="text-muted-foreground shrink-0">{p.published_date}</span>
                      </div>
                    ))}
                  </div>
                )}
                {state.result.skipped.length > 0 && (
                  <div className="space-y-1 mt-2">
                    <div className="text-xs font-medium text-muted-foreground">Skipped</div>
                    {state.result.skipped.map(wi => (
                      <div key={wi.article.id} className="text-xs text-muted-foreground">
                        {wi.article.title}: {wi.issue}
                      </div>
                    ))}
                  </div>
                )}
                {state.result.errors.length > 0 && (
                  <div className="space-y-1 mt-2">
                    <div className="text-xs font-medium text-destructive">Warnings</div>
                    {state.result.errors.map((e, i) => (
                      <div key={i} className="text-xs text-destructive">{e}</div>
                    ))}
                  </div>
                )}
                <p className="text-xs text-muted-foreground mt-2">
                  articles.json and MDX frontmatter have been updated. Deploy via your normal pipeline (git push / CI).
                </p>
              </div>
            )}

            {/* Error */}
            {state.kind === 'error' && (
              <div className="px-3 py-2 rounded-md text-xs bg-destructive/15 text-destructive">
                {state.message}
              </div>
            )}

          </div>
        </div>

        <SheetFooter className="shrink-0 px-5 py-4 border-t border-border flex-col gap-2">
          {state.kind === 'idle' && (
            <Button onClick={runPreflight} size="sm" className="w-full">
              Run Pre-flight Checks
            </Button>
          )}

          {state.kind === 'preflight_done' && publishableCount > 0 && (
            <div className="w-full space-y-2">
              {!allMismatchesResolved && state.result.year_mismatches.length > 0 && (
                <p className="text-xs text-amber-700 text-center">
                  Resolve all year mismatches above to include those articles.
                </p>
              )}
              <Button
                onClick={handlePublish}
                size="sm"
                className="w-full"
              >
                <Send size={13} />
                Publish {publishableCount} article{publishableCount !== 1 ? 's' : ''}
              </Button>
            </div>
          )}

          {state.kind === 'done' && (
            <SheetClose asChild>
              <Button variant="outline" size="sm" className="w-full">
                Close
              </Button>
            </SheetClose>
          )}

          {state.kind === 'error' && (
            <Button variant="outline" size="sm" onClick={() => setState({ kind: 'idle' })} className="w-full">
              Try again
            </Button>
          )}

          {(state.kind === 'preflight_done' && publishableCount === 0) && (
            <SheetClose asChild>
              <Button variant="outline" size="sm" className="w-full">
                Close
              </Button>
            </SheetClose>
          )}
        </SheetFooter>
        </div>
      </SheetContent>
    </Sheet>
  )
}
