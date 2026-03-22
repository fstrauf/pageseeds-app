import { useEffect, useState } from 'react'
import { X, ExternalLink, Sparkles, Loader2 } from 'lucide-react'
import { markRedditPosted, markRedditSkipped, validateRedditReply, draftRedditReply } from '../../lib/tauri'
import type { RedditOpportunity, ValidationResult } from '../../lib/types'

interface Props {
  opportunity: RedditOpportunity
  projectId: string
  onClose: () => void
  onStatusChange: () => void
}

export function OpportunityDetail({ opportunity: opp, projectId, onClose, onStatusChange }: Props) {
  const [replyText, setReplyText] = useState(opp.reply_text ?? '')
  const [validation, setValidation] = useState<ValidationResult | null>(null)
  const [saving, setSaving] = useState(false)
  const [drafting, setDrafting] = useState(false)
  const [actionMsg, setActionMsg] = useState<string | null>(null)

  // Reset on opportunity change
  useEffect(() => {
    setReplyText(opp.reply_text ?? '')
    setValidation(null)
    setActionMsg(null)
  }, [opp.post_id])

  async function handleValidate() {
    const result = await validateRedditReply(replyText, projectId)
    setValidation(result)
  }

  async function handleDraftWithAI() {
    setDrafting(true)
    setActionMsg(null)
    setValidation(null)
    try {
      const drafted = await draftRedditReply(projectId, opp.post_id)
      setReplyText(drafted)
      setActionMsg('Reply drafted. Review and edit before posting.')
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setActionMsg(`Draft failed: ${msg}`)
    } finally {
      setDrafting(false)
    }
  }

  async function handlePost() {
    const result = await validateRedditReply(replyText, projectId)
    setValidation(result)
    if (!result.valid) return

    setSaving(true)
    try {
      await markRedditPosted(opp.post_id, replyText, '', projectId)
      setActionMsg('Marked as posted.')
      onStatusChange()
    } catch (e) {
      setActionMsg(`Error: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  async function handleSkip() {
    setSaving(true)
    try {
      await markRedditSkipped(opp.post_id, projectId)
      setActionMsg('Skipped.')
      onStatusChange()
    } catch (e) {
      setActionMsg(`Error: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  const isPostedOrSkipped = opp.reply_status === 'posted' || opp.reply_status === 'skipped'

  return (
    <div className="flex flex-col h-full" style={{ background: 'var(--color-surface)' }}>
      {/* header */}
      <div className="flex items-start gap-2 px-4 py-3 border-b shrink-0" style={{ borderColor: 'var(--color-border)' }}>
        <div className="flex-1 min-w-0">
          <p className="text-xs font-semibold leading-snug" style={{ color: 'var(--color-text)' }}>
            {opp.title ?? 'No title'}
          </p>
          <p className="text-[10px] mt-0.5" style={{ color: 'var(--color-text-muted)' }}>
            {opp.subreddit ? `r/${opp.subreddit}` : ''} · {opp.author ?? ''} · {opp.upvotes ?? 0} pts
            {opp.created_at ? ` · ${Math.floor((Date.now() - new Date(opp.created_at).getTime()) / 86_400_000)}d old` : ''}
          </p>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          {opp.url && (
            <a href={opp.url} target="_blank" rel="noopener noreferrer" className="p-1 rounded hover:bg-white/5 transition-colors">
              <ExternalLink className="h-3.5 w-3.5" style={{ color: 'var(--color-text-muted)' }} />
            </a>
          )}
          <button onClick={onClose} className="p-1 rounded hover:bg-white/5 transition-colors">
            <X className="h-3.5 w-3.5" style={{ color: 'var(--color-text-muted)' }} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {/* scores */}
        {(opp.final_score != null || opp.severity) && (
          <div className="flex gap-3 text-xs flex-wrap">
            {opp.final_score != null && (
              <Chip label="Score" value={opp.final_score.toFixed(1)} />
            )}
            {opp.relevance_score != null && (
              <Chip label="Relevance" value={opp.relevance_score.toFixed(1)} />
            )}
            {opp.engagement_score != null && (
              <Chip label="Engagement" value={opp.engagement_score.toFixed(1)} />
            )}
            {opp.severity && (
              <Chip label="Severity" value={opp.severity} />
            )}
          </div>
        )}

        {/* mention stance badge */}
        {opp.mention_stance && (
          <StanceBadge stance={opp.mention_stance} />
        )}

        {/* why relevant */}
        {opp.why_relevant && (
          <Section title="Why relevant">
            <p className="text-xs leading-relaxed" style={{ color: 'var(--color-text)' }}>{opp.why_relevant}</p>
          </Section>
        )}

        {/* pain points */}
        {opp.key_pain_points?.length > 0 && (
          <Section title="Pain points">
            <ul className="space-y-1">
              {opp.key_pain_points.map((p, i) => (
                <li key={i} className="text-xs flex gap-2" style={{ color: 'var(--color-text)' }}>
                  <span style={{ color: 'var(--color-text-muted)' }}>·</span>
                  <span>{p}</span>
                </li>
              ))}
            </ul>
          </Section>
        )}

        {/* website fit */}
        {opp.website_fit && (
          <Section title="Website fit">
            <p className="text-xs leading-relaxed" style={{ color: 'var(--color-text)' }}>{opp.website_fit}</p>
          </Section>
        )}

        {/* reply area */}
        <Section title={isPostedOrSkipped ? 'Reply' : 'Draft reply'}>
          {/* Draft with AI button */}
          {!isPostedOrSkipped && (
            <button
              onClick={handleDraftWithAI}
              disabled={drafting || saving}
              className="flex items-center gap-1.5 mb-2 px-2.5 py-1 rounded text-[11px] font-medium transition-colors disabled:opacity-40"
              style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
            >
              {drafting
                ? <><Loader2 className="h-3 w-3 animate-spin" /> Drafting…</>
                : <><Sparkles className="h-3 w-3" /> Draft with AI</>
              }
            </button>
          )}
          <textarea
            value={replyText}
            onChange={e => { setReplyText(e.target.value); setValidation(null) }}
            disabled={isPostedOrSkipped || saving || drafting}
            rows={7}
            placeholder="Write your reply here — 3-5 sentences, no URLs, plain text only."
            className="w-full rounded px-3 py-2 text-xs resize-none outline-none focus:ring-1 focus:ring-primary/50 disabled:opacity-60"
            style={{
              background: 'var(--color-background)',
              border: '1px solid var(--color-border)',
              color: 'var(--color-text)',
            }}
          />

          {/* char + word count */}
          <p className="text-[10px] text-right mt-1" style={{ color: 'var(--color-text-muted)' }}>
            {replyText.split(/\s+/).filter(Boolean).length} words · {replyText.length} chars
          </p>

          {/* validation feedback */}
          {validation && (
            <div className={`mt-2 px-3 py-2 rounded text-xs ${validation.valid ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}`}>
              {validation.valid ? 'Reply looks good.' : validation.error}
            </div>
          )}
        </Section>

        {actionMsg && (
          <div className="px-3 py-2 rounded text-xs bg-blue-100 text-blue-700">{actionMsg}</div>
        )}
      </div>

      {/* actions */}
      {!isPostedOrSkipped && (
        <div className="flex gap-2 px-4 py-3 border-t shrink-0" style={{ borderColor: 'var(--color-border)' }}>
          <button
            onClick={handleValidate}
            disabled={saving || drafting || !replyText.trim()}
            className="px-3 py-1.5 rounded text-xs border transition-colors hover:bg-white/5 disabled:opacity-40"
            style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-muted)' }}
          >
            Validate
          </button>
          <div className="flex-1" />
          <button
            onClick={handleSkip}
            disabled={saving || drafting}
            className="px-3 py-1.5 rounded text-xs border transition-colors hover:bg-white/5 disabled:opacity-40"
            style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-muted)' }}
          >
            Skip
          </button>
          <button
            onClick={handlePost}
            disabled={saving || drafting || !replyText.trim()}
            className="px-3 py-1.5 rounded text-xs font-medium transition-colors disabled:opacity-40"
            style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
          >
            {saving ? 'Saving…' : 'Mark Posted'}
          </button>
        </div>
      )}
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="text-[10px] font-semibold uppercase tracking-wide mb-1.5" style={{ color: 'var(--color-text-muted)' }}>{title}</p>
      {children}
    </div>
  )
}

function Chip({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center gap-1 px-2 py-0.5 rounded" style={{ background: 'var(--color-background)', border: '1px solid var(--color-border)' }}>
      <span style={{ color: 'var(--color-text-muted)' }}>{label}</span>
      <span className="font-medium" style={{ color: 'var(--color-text)' }}>{value}</span>
    </div>
  )
}

function StanceBadge({ stance }: { stance: string }) {
  const config: Record<string, { label: string; color: string; hint: string }> = {
    REQUIRED:    { label: 'Mention: REQUIRED',    color: '#ef4444', hint: 'Reply must include the exact product name.' },
    RECOMMENDED: { label: 'Mention: RECOMMENDED', color: '#f97316', hint: 'Mention the product if it fits naturally.' },
    OPTIONAL:    { label: 'Mention: OPTIONAL',    color: '#6b7280', hint: 'Product mention is optional.' },
    OMIT:        { label: 'Mention: OMIT',        color: '#6b7280', hint: 'Do not mention any product.' },
  }
  const c = config[stance] ?? config.OPTIONAL
  return (
    <div
      className="flex items-center gap-1.5 px-2 py-1 rounded text-[10px]"
      style={{ background: `${c.color}18`, border: `1px solid ${c.color}55`, color: c.color }}
      title={c.hint}
    >
      <span className="font-semibold">{c.label}</span>
      <span style={{ opacity: 0.7 }}>· {c.hint}</span>
    </div>
  )
}
