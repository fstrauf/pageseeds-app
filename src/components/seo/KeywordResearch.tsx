import { useState, useEffect } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { seoGetKeywordIdeas, seoGetKeywordDifficulty, classifySearchIntent, scoreKeywordOpportunities, listArticles } from '../../lib/tauri'
import type { KeywordIdea, KeywordIdeasResult, KeywordDifficultyResult, SerpEntry, IntentClassification, OpportunityScore, Article } from '../../lib/types'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { Badge } from '../ui/badge'

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

function intentColor(intent?: string): string {
  switch (intent) {
    case 'transactional': return 'bg-purple-100 text-purple-700'
    case 'commercial': return 'bg-blue-100 text-blue-700'
    case 'informational': return 'bg-green-100 text-green-700'
    case 'navigational': return 'bg-gray-100 text-gray-700'
    default: return 'bg-secondary text-muted-foreground'
  }
}

function opportunityTierColor(tier?: string): string {
  switch (tier) {
    case 'high': return 'bg-emerald-100 text-emerald-700 border-emerald-200'
    case 'medium': return 'bg-amber-100 text-amber-700 border-amber-200'
    case 'low': return 'bg-gray-100 text-gray-700 border-gray-200'
    default: return 'bg-secondary text-muted-foreground'
  }
}

interface IdeaTableProps {
  ideas: KeywordIdea[]
  intents?: Map<string, IntentClassification>
  scores?: Map<string, OpportunityScore>
  showScores?: boolean
}

function IdeaTable({ ideas, intents, scores, showScores }: IdeaTableProps) {
  if (ideas.length === 0) {
    return <p className="text-sm text-muted-foreground py-4">No results.</p>
  }
  
  // Sort by opportunity score if available (high first)
  const sortedIdeas = showScores && scores
    ? [...ideas].sort((a, b) => {
        const scoreA = scores.get(a.keyword)?.total_score ?? 0
        const scoreB = scores.get(b.keyword)?.total_score ?? 0
        return scoreB - scoreA
      })
    : ideas
  
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted-foreground">
            <th className="py-2 pr-4 font-medium">Keyword</th>
            {showScores && <th className="py-2 pr-4 font-medium">Opportunity</th>}
            <th className="py-2 pr-4 font-medium">Intent</th>
            <th className="py-2 pr-4 font-medium">Difficulty</th>
            <th className="py-2 pr-4 font-medium">Volume</th>
            <th className="py-2 font-medium">Type</th>
          </tr>
        </thead>
        <tbody>
          {sortedIdeas.map((idea, i) => {
            const intent = intents?.get(idea.keyword)
            const score = scores?.get(idea.keyword)
            const volumeDisplay = idea.volume_exact 
              ? idea.volume_exact.toLocaleString()
              : (idea.volume ?? '—')
            
            return (
              <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
                <td className="py-2 pr-4" style={{ color: 'var(--color-text)' }}>
                  {idea.keyword}
                </td>
                {showScores && (
                  <td className="py-2 pr-4">
                    {score ? (
                      <Badge className={`text-xs ${opportunityTierColor(score.tier)}`}>
                        {score.tier} ({Math.round(score.total_score * 100)}%)
                      </Badge>
                    ) : (
                      <span className="text-xs text-muted-foreground">—</span>
                    )}
                  </td>
                )}
                <td className="py-2 pr-4">
                  {intent ? (
                    <Badge className={`text-xs ${intentColor(intent.intent)}`}>
                      {intent.intent}
                    </Badge>
                  ) : (
                    <span className="text-xs text-muted-foreground">—</span>
                  )}
                </td>
                <td className={`py-2 pr-4 font-medium ${difficultyColor(idea.difficulty)}`}>
                  {idea.difficulty ?? '—'}
                </td>
                <td className="py-2 pr-4 text-muted-foreground">{volumeDisplay}</td>
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
            )
          })}
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
  const [intents, setIntents] = useState<Map<string, IntentClassification>>(new Map())
  const [scores, setScores] = useState<Map<string, OpportunityScore>>(new Map())
  const [existingSlugs, setExistingSlugs] = useState<string[]>([])

  const [loadingIdeas, setLoadingIdeas] = useState(false)
  const [loadingDiff, setLoadingDiff] = useState(false)
  const [analyzing, setAnalyzing] = useState(false)
  const { showError } = useErrorHandler()
  
  // Load existing article slugs for opportunity scoring
  useEffect(() => {
    if (!projectId) return
    listArticles(projectId).then(articles => {
      setExistingSlugs(articles.map((a: Article) => a.url_slug))
    }).catch(() => {
      // Silently fail - opportunity scoring will work without slugs
    })
  }, [projectId])

  async function fetchIdeas() {
    if (!keyword.trim()) return
    setLoadingIdeas(true)
    setAnalyzing(true)
    setIdeasResult(null)
    setIntents(new Map())
    setScores(new Map())
    
    try {
      const result = await seoGetKeywordIdeas(projectId, keyword, country)
      setIdeasResult(result)
      
      // Analyze intents and scores for all ideas
      const allIdeas = [...result.ideas, ...result.question_ideas]
      if (allIdeas.length > 0) {
        const keywords = allIdeas.map(i => i.keyword)
        
        // Classify intents
        try {
          const intentResults = await classifySearchIntent(projectId, keywords)
          const intentMap = new Map(intentResults.map(i => [i.keyword, i]))
          setIntents(intentMap)
        } catch {
          // Intent classification is optional
        }
        
        // Score opportunities
        try {
          const scoreResults = await scoreKeywordOpportunities(projectId, allIdeas, [], existingSlugs)
          const scoreMap = new Map(scoreResults.map(s => [s.keyword, s]))
          setScores(scoreMap)
        } catch {
          // Scoring is optional
        }
      }
    } catch (e) {
      showError(String(e))
    } finally {
      setLoadingIdeas(false)
      setAnalyzing(false)
    }
  }

  async function fetchDifficulty() {
    if (!keyword.trim()) return
    setLoadingDiff(true)
    setDiffResult(null)
    try {
      const result = await seoGetKeywordDifficulty(projectId, keyword, country)
      setDiffResult(result)
    } catch (e) {
      showError(String(e))
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

      {analyzing && (
        <div className="mx-3 mt-3 rounded border border-blue-200 bg-blue-100 px-3 py-2 text-sm text-blue-700">
          Analyzing intents and opportunities…
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
            <Tabs defaultValue="high-opportunity">
              <TabsList className="bg-card border border-border mb-3">
                <TabsTrigger
                  value="high-opportunity"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  High Opportunity ({allIdeas.filter(i => scores.get(i.keyword)?.tier === 'high').length})
                </TabsTrigger>
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
              <TabsContent value="high-opportunity">
                <IdeaTable 
                  ideas={allIdeas.filter(i => scores.get(i.keyword)?.tier === 'high')} 
                  intents={intents} 
                  scores={scores} 
                  showScores={true} 
                />
              </TabsContent>
              <TabsContent value="all">
                <IdeaTable ideas={allIdeas} intents={intents} scores={scores} showScores={true} />
              </TabsContent>
              <TabsContent value="ideas">
                <IdeaTable ideas={ideasResult.ideas} intents={intents} scores={scores} showScores={true} />
              </TabsContent>
              <TabsContent value="questions">
                <IdeaTable ideas={ideasResult.question_ideas} intents={intents} scores={scores} showScores={true} />
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
