import { useEffect, useState } from 'react'
import { FolderOpen } from 'lucide-react'
import { createProject, updateProject, openFolderDialog } from '../../lib/tauri'
import type { Project, ProjectMode } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'

const PROJECT_MODE_LABEL: Record<ProjectMode, string> = {
  workspace: 'Workspace Project',
  live_site: 'Live Site Project',
}

function deriveDefaultSitemapUrl(rawSiteUrl: string) {
  const trimmed = rawSiteUrl.trim()
  if (!trimmed) return ''

  try {
    const url = new URL(trimmed)
    url.pathname = '/sitemap.xml'
    url.search = ''
    url.hash = ''
    return url.toString()
  } catch {
    return ''
  }
}

interface ProjectModalProps {
  project?: Project | null
  onClose: () => void
  onSaved: (project: Project) => void
}

export function ProjectModal({ project, onClose, onSaved }: ProjectModalProps) {
  const isEdit = !!project
  const [name, setName] = useState(project?.name ?? '')
  const [path, setPath] = useState(project?.path ?? '')
  const [contentDir, setContentDir] = useState(project?.content_dir ?? '')
  const [siteUrl, setSiteUrl] = useState(project?.site_url ?? '')
  const [siteId, setSiteId] = useState(project?.site_id ?? '')
  const [sitemapUrl, setSitemapUrl] = useState(project?.sitemap_url ?? '')
  const [projectMode, setProjectMode] = useState<ProjectMode>(project?.project_mode ?? 'workspace')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const isWorkspaceProject = projectMode === 'workspace'

  useEffect(() => {
    if (project) {
      setName(project.name)
      setPath(project.path)
      setContentDir(project.content_dir ?? '')
      setSiteUrl(project.site_url ?? '')
      setSiteId(project.site_id ?? '')
      setSitemapUrl(project.sitemap_url ?? '')
      setProjectMode(project.project_mode)
    }
  }, [project])

  function fillDefaultSitemapFromSiteUrl() {
    if (isWorkspaceProject || sitemapUrl.trim()) return
    const derived = deriveDefaultSitemapUrl(siteUrl)
    if (derived) {
      setSitemapUrl(derived)
    }
  }

  async function handleSave() {
    if (!name.trim()) {
      setError('Project name is required.')
      return
    }
    if (isWorkspaceProject && !path.trim()) {
      setError('Workspace projects require a repository path.')
      return
    }
    if (!isWorkspaceProject && !siteUrl.trim()) {
      setError('Live site projects require a site URL.')
      return
    }
    setLoading(true)
    setError(null)
    try {
      let saved: Project
      if (isEdit && project) {
        saved = await updateProject({
          ...project,
          name: name.trim(),
          path: isWorkspaceProject ? path.trim() : project.path,
          content_dir: isWorkspaceProject ? (contentDir.trim() || null) : null,
          site_url: siteUrl.trim() || null,
          site_id: siteId.trim() || null,
          sitemap_url: sitemapUrl.trim() || null,
          project_mode: project.project_mode,
        })
      } else {
        saved = await createProject({
          name: name.trim(),
          path: isWorkspaceProject ? path.trim() : undefined,
          content_dir: isWorkspaceProject ? (contentDir.trim() || undefined) : undefined,
          site_url: siteUrl.trim() || undefined,
          site_id: siteId.trim() || undefined,
          sitemap_url: sitemapUrl.trim() || undefined,
          project_mode: projectMode,
        })
      }
      onSaved(saved)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <Dialog open onOpenChange={open => { if (!open) onClose() }}>
      <DialogContent className="max-w-lg bg-card text-card-foreground border-border">
        <DialogHeader>
          <DialogTitle>{isEdit ? 'Edit Project' : 'Add Project'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {error && (
            <div className="px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
              {error}
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="proj-name">Project Name <span className="text-destructive">*</span></Label>
            <Input
              id="proj-name"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="My Website"
              className="bg-secondary border-input"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="proj-mode">Project Type</Label>
            {isEdit ? (
              <div className="rounded-md border border-border bg-secondary/40 px-3 py-2">
                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="text-xs">
                    {PROJECT_MODE_LABEL[projectMode]}
                  </Badge>
                  <span className="text-xs text-muted-foreground">Project type is fixed after creation.</span>
                </div>
              </div>
            ) : (
              <Select value={projectMode} onValueChange={value => setProjectMode(value as ProjectMode)}>
                <SelectTrigger id="proj-mode" className="w-full bg-secondary border-input">
                  <SelectValue placeholder="Choose a project type" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="workspace">Workspace Project</SelectItem>
                  <SelectItem value="live_site">Live Site Project</SelectItem>
                </SelectContent>
              </Select>
            )}
            <p className="text-xs text-muted-foreground">
              {isWorkspaceProject
                ? 'Use your existing repo and markdown content workflow.'
                : 'Store synced site data inside PageSeeds without requiring a local repo.'}
            </p>
          </div>

          {isWorkspaceProject ? (
            <>
              <div className="space-y-1.5">
                <Label htmlFor="proj-path">Repository Path <span className="text-destructive">*</span></Label>
                <div className="flex gap-2">
                  <Input
                    id="proj-path"
                    value={path}
                    onChange={e => setPath(e.target.value)}
                    placeholder="/Users/you/projects/my-website"
                    className="flex-1 bg-secondary border-input font-mono text-xs"
                  />
                  <Button
                    variant="outline"
                    size="icon"
                    className="border-input bg-secondary"
                    title="Browse for folder"
                    onClick={async () => {
                      const selected = await openFolderDialog('Select project root')
                      if (selected) setPath(selected)
                    }}
                  >
                    <FolderOpen size={14} />
                  </Button>
                </div>
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="proj-content">
                  Content Directory
                  <span className="ml-1 text-xs text-muted-foreground font-normal">(relative, e.g. content/blog)</span>
                </Label>
                <Input
                  id="proj-content"
                  value={contentDir}
                  onChange={e => setContentDir(e.target.value)}
                  placeholder="content/blog"
                  className="bg-secondary border-input"
                />
              </div>
            </>
          ) : (
            <div className="rounded-md border border-border bg-secondary/40 px-3 py-2 text-xs text-muted-foreground">
              Live site projects keep imported data in PageSeeds-managed app storage. No repository path or markdown content directory is required.
            </div>
          )}

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="proj-url">Site URL {!isWorkspaceProject && <span className="text-destructive">*</span>}</Label>
              <Input
                id="proj-url"
                value={siteUrl}
                onChange={e => setSiteUrl(e.target.value)}
                onBlur={fillDefaultSitemapFromSiteUrl}
                placeholder="https://mysite.com"
                className="bg-secondary border-input"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="proj-siteid">Site ID</Label>
              <Input
                id="proj-siteid"
                value={siteId}
                onChange={e => setSiteId(e.target.value)}
                placeholder={isWorkspaceProject ? 'my-website' : 'sc-domain:example.com'}
                className="bg-secondary border-input"
              />
            </div>
          </div>

          {!isWorkspaceProject && (
            <div className="space-y-1.5">
              <Label htmlFor="proj-sitemap">Sitemap URL</Label>
              <Input
                id="proj-sitemap"
                value={sitemapUrl}
                onChange={e => setSitemapUrl(e.target.value)}
                placeholder="https://mysite.com/sitemap.xml"
                className="bg-secondary border-input"
              />
              <p className="text-xs text-muted-foreground">
                Optional. Leave blank to try the default <span className="font-mono">/sitemap.xml</span> path.
              </p>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} className="border-border text-muted-foreground">
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={loading}>
            {loading ? 'Saving…' : isEdit ? 'Save Changes' : 'Add Project'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
