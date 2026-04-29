import { useCallback, useEffect, useMemo, useState } from 'react'
import { Shell } from './components/layout/Shell'
import { TaskBoard } from './components/tasks/TaskBoard'
import { ArticleTable } from './components/articles/ArticleTable'
import { LiveSiteAudit } from './components/articles/LiveSiteAudit'
import { LiveSiteLinkingMap } from './components/articles/LiveSiteLinkingMap'
import { LiveSitePageTable } from './components/articles/LiveSitePageTable'
import { ContentHealth } from './components/articles/ContentHealth'
import { CtrHealthPanel } from './components/articles/CtrHealthPanel'
import { LinkingMap } from './components/articles/LinkingMap'
import { CannibalizationReview } from './components/cannibalization/CannibalizationReview'
import { Reddit } from './components/reddit/Reddit'
import { GSC } from './components/gsc/GSC'
import { SEO } from './components/seo/SEO'
import { SocialDashboard } from './components/social'
import { ProjectModal } from './components/projects/ProjectModal'
import { Settings } from './components/settings/Settings'
import { SchedulerConfig } from './components/workflow/SchedulerConfig'
import { RunHistory } from './components/workflow/RunHistory'
import { Overview } from './components/overview/Overview'
import { TaskRunner } from './components/tasks/TaskRunner'
import { listProjects } from './lib/tauri'
import type { Project, Task, View } from './lib/types'
import { Tabs, TabsList, TabsTrigger, TabsContent } from './components/ui/tabs'
import { QueueContext } from './lib/queue-context'
import { useQueueRunner } from './hooks/useQueueRunner'

declare global {
  interface Window {
    __pendingProjectId?: string
  }
}

const VALID_VIEWS: View[] = [
  'overview',
  'tasks',
  'articles',
  'settings',
  'reddit',
  'gsc',
  'seo',
  'scheduler',
  'history',
  'social',
  'cannibalization',
]

function parseUrlHash(): { view: View | null; projectId: string | null } {
  const hash = window.location.hash.replace(/^#/, '')
  const [path, search] = hash.split('?')
  const params = new URLSearchParams(search || '')
  const rawView = path || params.get('view')
  const view = VALID_VIEWS.includes(rawView as View) ? (rawView as View) : null
  return { view, projectId: params.get('project') }
}

function writeUrlHash(view: View, projectId: string | null) {
  const hash = projectId ? `#/${view}?project=${projectId}` : `#/${view}`
  if (window.location.hash !== hash) {
    window.history.replaceState(null, '', hash)
  }
}

export default function App() {
  const [activeView, setActiveView] = useState<View>('overview')
  const [projects, setProjects] = useState<Project[]>([])
  const [activeProject, setActiveProject] = useState<Project | null>(null)
  const [pendingTaskId, setPendingTaskId] = useState<string | undefined>(undefined)
  const [runCompletedTick, setRunCompletedTick] = useState(0)

  const handleRunnerCompleted = useCallback(() => {
    setRunCompletedTick(v => v + 1)
  }, [])

  const queue = useQueueRunner(handleRunnerCompleted)
  // undefined = modal closed | null = new project | Project = edit that project
  const [modalProject, setModalProject] = useState<Project | null | undefined>(undefined)
  const [ready, setReady] = useState(false)

  // Restore state from URL on initial load
  useEffect(() => {
    async function loadProjects() {
      try {
        const data = await listProjects()
        setProjects(data)
        const pendingId = window.__pendingProjectId
        delete window.__pendingProjectId

        if (data.length > 0) {
          const match = pendingId ? data.find(p => p.id === pendingId) : null
          if (match) {
            setActiveProject(match)
          } else if (!activeProject) {
            setActiveProject(data[0])
          }
        }
        if (data.length === 0) {
          setModalProject(null)
        }
      } catch {
        setModalProject(null)
      } finally {
        setReady(true)
      }
    }

    const { view, projectId } = parseUrlHash()
    if (view) setActiveView(view)
    // project selection happens after projects load
    if (projectId) {
      // stash it temporarily so loadProjects can use it
      window.__pendingProjectId = projectId
    }
    loadProjects()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Sync URL whenever view or project changes
  useEffect(() => {
    if (ready) {
      writeUrlHash(activeView, activeProject?.id ?? null)
    }
  }, [activeView, activeProject, ready])

  const loadProjects = useCallback(async () => {
    try {
      const data = await listProjects()
      setProjects(data)
      const pendingId = window.__pendingProjectId
      delete window.__pendingProjectId

      if (data.length > 0) {
        const match = pendingId ? data.find(p => p.id === pendingId) : null
        if (match) {
          setActiveProject(match)
        } else if (!activeProject) {
          setActiveProject(data[0])
        }
      }
      if (data.length === 0) {
        setModalProject(null)
      }
    } catch {
      setModalProject(null)
    } finally {
      setReady(true)
    }
  }, [activeProject])

  const handleProjectSaved = useCallback((project: Project) => {
    setActiveProject(project)
    setModalProject(undefined)
    loadProjects()
  }, [loadProjects])

  const handleCloseModal = useCallback(() => {
    // Only allow closing if there's already an active project
    if (activeProject) setModalProject(undefined)
  }, [activeProject])

  const handleRunTasks = useCallback((tasks: Task[]) => {
    const runnableTasks = tasks.filter(task => task.execution_mode !== 'manual')
    if (runnableTasks.length === 0) return
    queue.enqueue(
      runnableTasks.map(t => ({
        taskId: t.id,
        projectId: t.project_id,
        title: t.title ?? t.type ?? 'Untitled',
        taskType: t.type ?? '',
        projectName: activeProject?.name,
      })),
    )
  }, [queue, activeProject?.name])

  const queueContextValue = useMemo(
    () => ({
      enqueue: queue.enqueue,
      enqueueNext: queue.enqueueNext,
      isActive: queue.isVisible,
    }),
    [queue.enqueue, queue.enqueueNext, queue.isVisible],
  )

  const handleViewChange = useCallback((view: View, taskId?: string) => {
    setActiveView(view)
    if (taskId) setPendingTaskId(taskId)
  }, [])

  const handleTaskOpened = useCallback(() => {
    setPendingTaskId(undefined)
  }, [])

  const handleAddProject = useCallback(() => {
    setModalProject(null)
  }, [])

  const handleEditProject = useCallback((p: Project) => {
    setModalProject(p)
  }, [])

  const handleOpenTask = useCallback((taskId: string) => {
    setActiveView('tasks')
    setPendingTaskId(taskId)
  }, [])

  if (!ready) {
    return (
      <div
        className="flex h-screen items-center justify-center text-sm"
        style={{ background: 'var(--color-background)', color: 'var(--color-text-muted)' }}
      >
        Loading…
      </div>
    )
  }

  return (
    <QueueContext.Provider value={queueContextValue}>
      <Shell
        activeView={activeView}
        onViewChange={setActiveView}
        projects={projects}
        activeProjectId={activeProject?.id}
        onProjectSelect={setActiveProject}
        onAddProject={handleAddProject}
        onEditProject={handleEditProject}
      >
        {activeView === 'overview' && (
          <Overview
            project={activeProject}
            onViewChange={handleViewChange}
            onRunTasks={handleRunTasks}
            runCompletedTick={runCompletedTick}
          />
        )}
        {activeView === 'tasks' && (
          <TaskBoard
            projectId={activeProject?.id}
            projectName={activeProject?.name}
            initialTaskId={pendingTaskId}
            onTaskOpened={handleTaskOpened}
            onRunTasks={handleRunTasks}
            runCompletedTick={runCompletedTick}
          />
        )}
        {activeView === 'articles' && (
          activeProject?.project_mode === 'live_site' ? (
            <div className="flex flex-col h-full overflow-hidden">
              <Tabs defaultValue="pages" className="flex flex-col h-full">
                <div className="px-6 pt-4 border-b border-border shrink-0">
                  <TabsList className="bg-card border border-border">
                    <TabsTrigger value="pages" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Pages
                    </TabsTrigger>
                    <TabsTrigger value="audit" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Audit
                    </TabsTrigger>
                    <TabsTrigger value="links" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Links
                    </TabsTrigger>
                  </TabsList>
                </div>
                <TabsContent value="pages" className="flex-1 overflow-y-auto mt-0 p-0">
                  <LiveSitePageTable
                    projectId={activeProject?.id ?? ''}
                    project={activeProject ?? undefined}
                  />
                </TabsContent>
                <TabsContent value="audit" className="flex-1 overflow-y-auto mt-0 p-0">
                  <LiveSiteAudit projectId={activeProject?.id ?? ''} />
                </TabsContent>
                <TabsContent value="links" className="flex-1 overflow-y-auto mt-0 p-0">
                  <LiveSiteLinkingMap projectId={activeProject?.id ?? ''} />
                </TabsContent>
              </Tabs>
            </div>
          ) : (
            <div className="flex flex-col h-full overflow-hidden">
              <Tabs defaultValue="list" className="flex flex-col h-full">
                <div className="px-6 pt-4 border-b border-border shrink-0">
                  <TabsList className="bg-card border border-border">
                    <TabsTrigger value="list" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Articles
                    </TabsTrigger>
                    <TabsTrigger value="health" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Health
                    </TabsTrigger>
                    <TabsTrigger value="links" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      Links
                    </TabsTrigger>
                    <TabsTrigger value="ctr" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                      CTR
                    </TabsTrigger>
                  </TabsList>
                </div>
                <TabsContent value="list" className="flex-1 overflow-y-auto mt-0 p-0">
                  <ArticleTable
                    projectId={activeProject?.id ?? ''}
                    project={activeProject ?? undefined}
                    onEditProject={activeProject ? () => setModalProject(activeProject) : undefined}
                  />
                </TabsContent>
                <TabsContent value="health" className="flex-1 overflow-y-auto mt-0 p-0">
                  <ContentHealth projectId={activeProject?.id ?? ''} />
                </TabsContent>
                <TabsContent value="links" className="flex-1 overflow-y-auto mt-0 p-0">
                  <LinkingMap projectId={activeProject?.id ?? ''} />
                </TabsContent>
                <TabsContent value="ctr" className="flex-1 overflow-y-auto mt-0 p-0">
                  <CtrHealthPanel projectId={activeProject?.id ?? ''} runCompletedTick={runCompletedTick} />
                </TabsContent>
              </Tabs>
            </div>
          )
        )}
        {activeView === 'settings' && <Settings projectId={activeProject?.id} />}
        {activeView === 'reddit' && (
          <Reddit
            projectId={activeProject?.id ?? ''}
            project={activeProject ?? undefined}
          />
        )}
        {activeView === 'gsc' && (
          <GSC
            projectId={activeProject?.id ?? ''}
            project={activeProject ?? undefined}
          />
        )}
        {activeView === 'seo' && (
          <SEO
            projectId={activeProject?.id ?? ''}
            project={activeProject ?? undefined}
            onRunTasks={handleRunTasks}
          />
        )}
        {activeView === 'scheduler' && <SchedulerConfig projectId={activeProject?.id ?? ''} />}
        {activeView === 'history' && <RunHistory projectId={activeProject?.id ?? ''} />}
        {activeView === 'social' && (
          <SocialDashboard
            projectId={activeProject?.id ?? ''}
          />
        )}
        {activeView === 'cannibalization' && (
          <CannibalizationReview projectId={activeProject?.id ?? ''} />
        )}
      </Shell>

      {queue.isVisible && (
        <TaskRunner
          items={queue.items}
          isRunning={queue.isRunning}
          isPaused={queue.isPaused}
          isStarting={queue.isStarting}
          onPause={queue.pause}
          onResume={queue.resume}
          onRemove={queue.removeItem}
          onClose={queue.close}
          onOpenTask={handleOpenTask}
        />
      )}

      {modalProject !== undefined && (
        <ProjectModal
          project={modalProject}
          onClose={handleCloseModal}
          onSaved={handleProjectSaved}
        />
      )}
    </QueueContext.Provider>
  )
}

