import React, { useState } from 'react'
import { Layers, FileText, Search, Globe, Settings, ChevronDown, Plus, Pencil, Check, Clock, History, LayoutDashboard, Share2, GitMerge } from 'lucide-react'
import { cn } from '../../lib/utils'
import type { Project, View } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

interface NavItem {
  id: View
  label: string
  icon: React.ReactNode
  phase?: string
  badge?: number
  disabled?: boolean
}

const NAV_ITEMS: NavItem[] = [
  { id: 'overview', label: 'Overview', icon: <LayoutDashboard size={16} /> },
  { id: 'tasks', label: 'Tasks', icon: <Layers size={16} /> },
  { id: 'articles', label: 'Articles', icon: <FileText size={16} /> },
  { id: 'reddit', label: 'Reddit', icon: <Search size={16} /> },
  { id: 'gsc', label: 'Search Console', icon: <Globe size={16} /> },
  { id: 'seo', label: 'Keywords', icon: <Search size={16} /> },
  { id: 'social', label: 'Social', icon: <Share2 size={16} /> },
  { id: 'cannibalization', label: 'Cannibalization', icon: <GitMerge size={16} /> },
  { id: 'scheduler', label: 'Scheduler', icon: <Clock size={16} /> },
  { id: 'history', label: 'Run History', icon: <History size={16} /> },
  { id: 'settings', label: 'Settings', icon: <Settings size={16} /> },
]

interface SidebarProps {
  activeView: View
  onViewChange: (view: View) => void
  projects: Project[]
  activeProjectId?: string
  onProjectSelect: (project: Project) => void
  onAddProject: () => void
  onEditProject: (project: Project) => void
}

export function Sidebar({
  activeView,
  onViewChange,
  projects,
  activeProjectId,
  onProjectSelect,
  onAddProject,
  onEditProject,
}: SidebarProps) {
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const activeProject = projects.find(p => p.id === activeProjectId)

  return (
    <aside className="w-56 h-full flex flex-col shrink-0 border-r border-border bg-card">

      {/* App header */}
      <div className="px-4 py-4 border-b border-border">
        <div className="text-sm font-semibold tracking-wide text-primary">
          PageSeeds
        </div>
        <div className="text-xs mt-0.5 text-muted-foreground">
          SEO Automation
        </div>
      </div>

      {/* Project selector — Radix DropdownMenu */}
      <div className="mx-3 my-3">
        <DropdownMenu open={dropdownOpen} onOpenChange={setDropdownOpen}>
          <DropdownMenuTrigger asChild>
            <Button
              variant="outline"
              className="w-full justify-between border-border bg-secondary text-foreground h-auto py-2 px-3"
            >
              <div className="min-w-0 text-left">
                <div className="text-xs text-muted-foreground">Project</div>
                <div className="text-sm font-medium truncate">
                  {activeProject?.name ?? 'Select project…'}
                </div>
              </div>
              <ChevronDown
                size={14}
                className={cn('shrink-0 ml-2 text-muted-foreground transition-transform', dropdownOpen && 'rotate-180')}
              />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            className="w-48 bg-popover border-border text-popover-foreground"
          >
            {projects.length === 0 && (
              <div className="px-2 py-1.5 text-xs text-muted-foreground">No projects yet</div>
            )}
            {projects.map(p => (
              <DropdownMenuItem
                key={p.id}
                onSelect={() => onProjectSelect(p)}
                className="flex items-center gap-2 text-sm cursor-pointer"
              >
                <Check
                  size={12}
                  className={cn('text-primary shrink-0', p.id === activeProjectId ? 'opacity-100' : 'opacity-0')}
                />
                <span className="flex-1 truncate">{p.name}</span>
              </DropdownMenuItem>
            ))}

            <DropdownMenuSeparator className="bg-border" />

            {activeProject && (
              <DropdownMenuItem
                onSelect={() => onEditProject(activeProject)}
                className="flex items-center gap-2 text-sm text-muted-foreground cursor-pointer"
              >
                <Pencil size={12} className="shrink-0" />
                Edit current project
              </DropdownMenuItem>
            )}

            <DropdownMenuItem
              onSelect={onAddProject}
              className="flex items-center gap-2 text-sm text-primary cursor-pointer"
            >
              <Plus size={12} className="shrink-0" />
              Add project
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <Separator className="bg-border" />

      {/* Navigation */}
      <ScrollArea className="flex-1 py-2">
        <nav className="px-2 space-y-0.5">
          {NAV_ITEMS.map((item) => (
            <Button
              key={item.id}
              variant={activeView === item.id ? 'default' : 'ghost'}
              size="sm"
              disabled={item.disabled}
              onClick={() => !item.disabled && onViewChange(item.id)}
              className={cn(
                'w-full justify-start gap-2.5 text-sm',
                activeView === item.id
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:text-foreground',
                item.disabled && 'opacity-40',
              )}
            >
              {item.icon}
              <span className="flex-1 text-left">{item.label}</span>
              {item.disabled && (
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-secondary text-muted-foreground">
                  Soon
                </span>
              )}
            </Button>
          ))}
        </nav>
      </ScrollArea>

      {/* Footer */}
      <div className="px-4 py-3 border-t border-border text-xs text-muted-foreground">
        v0.1.0
      </div>
    </aside>
  )
}
