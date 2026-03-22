import React from 'react'
import type { Project, View } from '../../lib/types'
import { Sidebar } from './Sidebar'

interface ShellProps {
  activeView: View
  onViewChange: (view: View) => void
  projects: Project[]
  activeProjectId?: string
  onProjectSelect: (project: Project) => void
  onAddProject: () => void
  onEditProject: (project: Project) => void
  children: React.ReactNode
}

export function Shell({
  activeView,
  onViewChange,
  projects,
  activeProjectId,
  onProjectSelect,
  onAddProject,
  onEditProject,
  children,
}: ShellProps) {
  return (
    <div className="flex h-screen" style={{ background: 'var(--color-background)', color: 'var(--color-text)' }}>
      <Sidebar
        activeView={activeView}
        onViewChange={onViewChange}
        projects={projects}
        activeProjectId={activeProjectId}
        onProjectSelect={onProjectSelect}
        onAddProject={onAddProject}
        onEditProject={onEditProject}
      />
      <main className="flex-1 overflow-hidden flex flex-col">
        {children}
      </main>
    </div>
  )
}
