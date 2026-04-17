import { useState, useEffect } from 'react'
import { Play, Loader2 } from 'lucide-react'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { OpportunityFeed } from './OpportunityFeed'
import { OpportunityDetail } from './OpportunityDetail'
import { RedditSearch } from './RedditSearch'
import { RedditStats } from './RedditStats'
import { createTask } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import { invalidateQueries } from '../../hooks/useQuery'
import type { Project, RedditOpportunity } from '../../lib/types'

interface Props {
  projectId: string
  project?: Project
}

export function Reddit({ projectId, project }: Props) {
  const [selected, setSelected] = useState<RedditOpportunity | null>(null)
  const [searching, setSearching] = useState(false)
  const [searchMsg, setSearchMsg] = useState<string | null>(null)
  const [showContextDialog, setShowContextDialog] = useState(false)
  const [userContext, setUserContext] = useState('')
  const queue = useQueue()
  
  // Listen for queue completion to refresh feed
  const [lastQueueActive, setLastQueueActive] = useState(queue.isActive)
  useEffect(() => {
    if (lastQueueActive && !queue.isActive) {
      // Queue was active and is now inactive - refresh the feed
      invalidateQueries('reddit')
      setSearchMsg(null)
    }
    setLastQueueActive(queue.isActive)
  }, [queue.isActive, lastQueueActive])

  function handleStatusChange() {
    invalidateQueries('reddit')
    setSelected(null)
  }

  function handleSearchSaved() {
    invalidateQueries('reddit')
  }

  function handleRunSearch() {
    setShowContextDialog(true)
  }

  async function handleConfirmSearch() {
    setShowContextDialog(false)
    setSearching(true)
    setSearchMsg('Creating task...')
    try {
      // Create task with user context in description (JSON format expected by backend)
      const description = userContext.trim()
        ? JSON.stringify({ user_context: userContext.trim() })
        : undefined
      
      const task = await createTask(
        projectId,
        'reddit_opportunity_search',
        `Reddit Opportunity Search — ${new Date().toLocaleDateString()}`,
        description,
        'high',
      )
      
      // Add to queue - execution happens via queue system
      queue.enqueue([{
        taskId: task.id,
        projectId: task.project_id,
        projectName: project?.name,
        title: task.title ?? 'Reddit Opportunity Search',
        taskType: task.type,
        status: 'pending',
      }])
      
      setSearchMsg('Added to queue...')
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setSearchMsg(`✗ Failed: ${msg}`)
    } finally {
      setSearching(false)
      setUserContext('')
    }
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Context dialog */}
      {showContextDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div
            className="w-full max-w-md rounded-lg p-5 shadow-xl"
            style={{ background: 'var(--color-surface)', border: '1px solid var(--color-border)' }}
          >
            <p className="text-sm font-semibold mb-1" style={{ color: 'var(--color-text)' }}>
              Run Reddit Opportunity Search
            </p>
            <p className="text-xs mb-3" style={{ color: 'var(--color-text-muted)' }}>
              Optionally add extra focus for this search — e.g. a specific pain point, topic, or audience angle. Leave blank to use config defaults.
            </p>
            <textarea
              autoFocus
              value={userContext}
              onChange={e => setUserContext(e.target.value)}
              rows={3}
              placeholder="e.g. focus on users frustrated with spreadsheet-based tracking"
              className="w-full rounded px-3 py-2 text-xs resize-none outline-none focus:ring-1 focus:ring-primary/50 mb-3"
              style={{
                background: 'var(--color-background)',
                border: '1px solid var(--color-border)',
                color: 'var(--color-text)',
              }}
            />
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => { setShowContextDialog(false); setUserContext('') }}
                className="px-3 py-1.5 rounded text-xs border transition-colors hover:bg-white/5"
                style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-muted)' }}
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmSearch}
                className="px-3 py-1.5 rounded text-xs font-medium transition-colors"
                style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
              >
                Run Search
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Left: tabbed panel */}
      <div
        className={`flex flex-col overflow-hidden transition-all ${selected ? 'flex-1' : 'w-full'}`}
      >
        <Tabs defaultValue="feed" className="flex flex-col h-full">
          <div className="px-4 pt-3 border-b shrink-0" style={{ borderColor: 'var(--color-border)' }}>
            <div className="flex items-center justify-between gap-2 mb-2">
              <TabsList className="bg-card border border-border">
                <TabsTrigger
                  value="feed"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  Opportunities
                </TabsTrigger>
                <TabsTrigger
                  value="search"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  Search
                </TabsTrigger>
                <TabsTrigger
                  value="stats"
                  className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  Stats
                </TabsTrigger>
              </TabsList>

              {/* Automated search trigger */}
              <button
                onClick={handleRunSearch}
                disabled={searching}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition-colors disabled:opacity-50"
                style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
                title="Run automated search using reddit_config.md"
              >
                {searching
                  ? <><Loader2 className="h-3 w-3 animate-spin" /> Searching…</>
                  : <><Play className="h-3 w-3" /> Run Search</>
                }
              </button>
            </div>

            {/* Status message */}
            {searchMsg && (
              <p
                className="text-[10px] pb-2"
                style={{ color: searchMsg.startsWith('✓') ? 'var(--color-success, #4ade80)' : 'var(--color-destructive, #f87171)' }}
              >
                {searchMsg}
              </p>
            )}
          </div>

          <TabsContent value="feed" className="flex-1 overflow-hidden mt-0 p-0">
            <OpportunityFeed
              projectId={projectId}
              selectedId={selected?.post_id}
              onSelect={setSelected}
              onStatusChange={handleStatusChange}
            />
          </TabsContent>

          <TabsContent value="search" className="flex-1 overflow-hidden mt-0 p-0">
            <RedditSearch projectId={projectId} onSaved={handleSearchSaved} />
          </TabsContent>

          <TabsContent value="stats" className="flex-1 overflow-y-auto mt-0 p-0">
            <RedditStats projectId={projectId} />
          </TabsContent>
        </Tabs>
      </div>

      {/* Right: detail panel */}
      {selected && (
        <div
          className="w-96 shrink-0 overflow-hidden border-l flex flex-col"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <OpportunityDetail
            opportunity={selected}
            projectId={projectId}
            onClose={() => setSelected(null)}
            onStatusChange={handleStatusChange}
          />
        </div>
      )}
    </div>
  )
}
