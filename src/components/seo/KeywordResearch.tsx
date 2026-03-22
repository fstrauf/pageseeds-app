import { useState } from 'react'
import { seoGetKeywordIdeas, seoGetKeywordDifficulty } from '../../lib/tauri'
import type { KeywordIdea, KeywordIdeasResult, KeywordDifficultyResult, SerpEntry } from '../../lib/types'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'

interface Props {
  projectId: string
}

const COUNTRY_OPTIONS = ['us', 'uk', 'au', 'ca', 'de', 'fr', 'in', 'br']

function difficultyColor(d?: string): string {
  if (!d) return 'text-muted-foreground'
  const lower = d.toLowerCase()
  if (lower.includes('easy') || lower.includes('low')) return 'text-green-500'
  if (lower.includes('hard') || lower.includes('high')) return 'text-red-500'
  return 'text-yellow-500'
}

function IdeaTable({ ideas }: { ideas: KeywordIdea[] }) {
  if (ideas.length === 0) {
    return <p className="text-sm text-muted-foreground py-4">No results.</p>
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted-foreground">
            <th className="py-2 pr-4 font-medium">Keyword</th>
            <th className="py-2 pr-4 font-medium">Difficulty</th>
            <th className="py-2 pr-4 font-medium">Volume</th>
            <th className="py-2 font-medium">Type</th>
          </tr>
        </thead>
        <tbody>
          {ideas.map((idea, i) => (
            <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
              <td className="py-2 pr-4" style={{ color: 'var(--color-text)' }}>
                {idea.keyword}
              </td>
              <td className={`py-2 pr-4 font-medium ${difficultyColor(idea.difficulty)}`}>
                {idea.difficulty ?? '—'}
              </td>
              <td className="py-2 pr-4 text-muted-foreground">{idea.volume ?? '—'}</td>
              <td className="py-2">
                <span
                  className={`text-xs px-1.5 py-0.5 rounded ${
                    idea.idea_type === 'question'
                      ? 'bg-blue-100 text-blue-700'
                      : 'bg-secondary text-muted-foreground'
                  }`}
                >
                  {idea.idea_type}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function DifficultyPanel({ result }: { result: KeywordDifficultyResult }) {
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        <div className="rounded border border-border bg-card p-3">
          <div className="text-xs text-muted-foreground mb-1">Difficulty</div>
          <div className="text-2xl font-bold" style={{ color: 'var(--color-text)' }}>
            {Math.round(result.difficulty)}
          </div>
        </div>
        <div className="rounded border border-border bg-card p-3">
          <div className="text-xs text-muted-foreground mb-1">Shortage</div>
          <div className="text-2xl font-bold" style={{ color: 'var(--color-text)' }}>
            {Math.round(result.shortage)}
          </div>
        </div>
      </div>

      {result.serp.length > 0 && (
        <div>
          <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
            SERP Results
          </div>
          <div className="space-y-1.5">
            {result.serp.map((entry: SerpEntry) => (
              <div
                key={entry.position}
                className="flex items-start gap-3 rounded border border-border bg-card p-2.5"
              >
                <span className="shrink-0 text-xs text-muted-foreground w-5 text-right">
                  {entry.position}
                </span>
                <div className="min-w-0">
                  <div className="text-sm truncate" style={{ color: 'var(--color-text)' }}>
                    {entry.title || entry.url}
                  </div>
                  <div className="text-xs text-muted-foreground truncate">{entry.domain}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

export function KeywordResearch({ projectId }: Props) {
  const [keyword, setKeyword] = useState('')
  const [country, setCountry] = useState('us')

  const [ideasResult, setIdeasResult] = useState<KeywordIdeasResult | null>(null)
  const [diffResult, setDiffResult] = useState<KeywordDifficultyResult | null>(null)

  const [loadingIdeas, setLoadingIdeas] = useState(false)
  const [loadingDiff, setLoadingDiff] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function fetchIdeas() {
    if (!keyword.trim()) return
    setLoadingIdeas(true)
    setError(null)
    setIdeasResult(null)
    try {
      const result = await seoGetKeywordIdeas(projectId, keyword, country)
      setIdeasResult(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoadingIdeas(false)
    }
  }

  async function fetchDifficulty() {
    if (!keyword.trim()) return
    setLoadingDiff(true)
    setError(null)
    setDiffResult(null)
    try {
      const result = await seoGetKeywordDifficulty(projectId, keyword, country)
      setDiffResult(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoadingDiff(false)
    }
  }

  const allIdeas = ideasResult
    ? [...ideasResult.ideas, ...ideasResult.question_ideas]
    : []

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Controls */}
      <div
        className="flex gap-2 items-end flex-wrap p-3 border-b shrink-0"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <div className="flex flex-col gap-1 flex-1 min-w-40">
          <label className="text-xs text-muted-foreground">Keyword</label>
          <input
            className="h-8 px-2 rounded border border-border bg-card text-sm"
            value={keyword}
            onChange={e => setKeyword(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && fetchIdeas()}
            placeholder="e.g. content marketing"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Country</label>
          <select
            className="h-8 px-2 rounded border border-border bg-card text-sm"
            value={country}
            onChange={e => setCountry(e.target.value)}
          >
            {COUNTRY_OPTIONS.map(c => (
              <option key={c} value={c}>
                {c.toUpperCase()}
              </option>
            ))}
          </select>
        </div>
        <button
          className="h-8 px-3 rounded bg-primary text-primary-foreground text-sm font-medium disabled:opacity-50"
          onClick={fetchIdeas}
          disabled={!keyword.trim() || loadingIdeas}
        >
          {loadingIdeas ? 'Solving CAPTCHA…' : 'Get Ideas'}
        </button>
        <button
          className="h-8 px-3 rounded border border-border bg-card text-sm font-medium disabled:opacity-50"
          style={{ color: 'var(--color-text)' }}
          onClick={fetchDifficulty}
          disabled={!keyword.trim() || loadingDiff}
        >
          {loadingDiff ? 'Loading…' : 'KD Check'}
        </button>
      </div>

      {error && (
        <div className="mx-3 mt-3 rounded border border-red-200 bg-red-100 px-3 py-2 text-sm text-red-700">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-3">
        {/* Difficulty result (shown above ideas when available) */}
        {diffResult && (
          <div className="mb-4">
            <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
              Keyword Difficulty — {diffResult.keyword}
            </div>
            <DifficultyPanel result={diffResult} />
          </div>
        )}

        {/* Ideas result */}
        {ideasResult && (
          <div>
            <Tabs defaultValue="all">
              <TabsList className="bg-card border border-border mb-3">
                <TabsTrigger
                  value="all"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  All ({allIdeas.length})
                </TabsTrigger>
                <TabsTrigger
                  value="ideas"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  Suggestions ({ideasResult.ideas.length})
                </TabsTrigger>
                <TabsTrigger
                  value="questions"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  Questions ({ideasResult.question_ideas.length})
                </TabsTrigger>
              </TabsList>
              <TabsContent value="all">
                <IdeaTable ideas={allIdeas} />
              </TabsContent>
              <TabsContent value="ideas">
                <IdeaTable ideas={ideasResult.ideas} />
              </TabsContent>
              <TabsContent value="questions">
                <IdeaTable ideas={ideasResult.question_ideas} />
              </TabsContent>
            </Tabs>
          </div>
        )}

        {!ideasResult && !diffResult && !loadingIdeas && !loadingDiff && (
          <p className="text-sm text-muted-foreground">
            Enter a keyword and click <strong>Get Ideas</strong> or <strong>KD Check</strong>.
            <br />
            Requires <code className="text-xs">CAPSOLVER_API_KEY</code> in your secrets file.
          </p>
        )}
      </div>
    </div>
  )
}
