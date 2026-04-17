import { useCallback, useEffect, useState } from 'react'
import { Shell } from './components/layout/Shell'
import { TaskBoard } from './components/tasks/TaskBoard'
import { ArticleTable } from './components/articles/ArticleTable'
import { ContentHealth } from './components/articles/ContentHealth'
import { LinkingMap } from './components/articles/LinkingMap'
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
    const { view, projectId } = parseUrlHash()
    if (view) setActiveView(view)
    // project selection happens after projects load
    if (projectId) {
      // stash it temporarily so loadProjects can use it
      ;(window as any).__pendingProjectId = projectId
    }
    loadProjects()
  }, [])

  // Sync URL whenever view or project changes
  useEffect(() => {
    if (ready) {
      writeUrlHash(activeView, activeProject?.id ?? null)
    }
  }, [activeView, activeProject, ready])

  async function loadProjects() {
    try {
      const data = await listProjects()
      setProjects(data)
      const pendingId = (window as any).__pendingProjectId as string | undefined
      delete (window as any).__pendingProjectId

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

  function handleProjectSaved(project: Project) {
    setActiveProject(project)
    setModalProject(undefined)
    loadProjects()
  }

  function handleCloseModal() {
    // Only allow closing if there's already an active project
    if (activeProject) setModalProject(undefined)
  }

  function handleRunTasks(tasks: Task[]) {
    if (tasks.length === 0) return
    queue.enqueue(
      tasks.map(t => ({
        taskId: t.id,
        projectId: t.project_id,
        title: t.title ?? t.type ?? 'Untitled',
        taskType: t.type ?? '',
        projectName: activeProject?.name,
      })),
    )
  }

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
    <QueueContext.Provider
      value={{
        enqueue: queue.enqueue,
        enqueueNext: queue.enqueueNext,
        isActive: queue.isVisible,
      }}
    >
      <Shell
        activeView={activeView}
        onViewChange={setActiveView}
        projects={projects}
        activeProjectId={activeProject?.id}
        onProjectSelect={setActiveProject}
        onAddProject={() => setModalProject(null)}
        onEditProject={(p) => setModalProject(p)}
      >
        {activeView === 'overview' && (
          <Overview
            project={activeProject}
            onViewChange={(view, taskId) => {
              setActiveView(view)
              if (taskId) setPendingTaskId(taskId)
            }}
            onRunTasks={handleRunTasks}
            runCompletedTick={runCompletedTick}
          />
        )}
        {activeView === 'tasks' && (
          <TaskBoard
            projectId={activeProject?.id}
            projectName={activeProject?.name}
            initialTaskId={pendingTaskId}
            onTaskOpened={() => setPendingTaskId(undefined)}
            onRunTasks={handleRunTasks}
            runCompletedTick={runCompletedTick}
          />
        )}
        {activeView === 'articles' && (
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
            </Tabs>
          </div>
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
      </Shell>

      {queue.isVisible && (
        <TaskRunner
          items={queue.items}
          isRunning={queue.isRunning}
          isPaused={queue.isPaused}
          onPause={queue.pause}
          onResume={queue.resume}
          onRemove={queue.removeItem}
          onClose={queue.close}
          onOpenTask={(taskId) => {
            setActiveView('tasks')
            setPendingTaskId(taskId)
          }}
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

