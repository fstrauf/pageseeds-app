import { useEffect, useState } from 'react'
import { FolderOpen } from 'lucide-react'
import { createProject, updateProject, openFolderDialog } from '../../lib/tauri'
import type { Project } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'

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
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (project) {
      setName(project.name)
      setPath(project.path)
      setContentDir(project.content_dir ?? '')
      setSiteUrl(project.site_url ?? '')
      setSiteId(project.site_id ?? '')
    }
  }, [project])

  async function handleSave() {
    if (!name.trim() || !path.trim()) {
      setError('Name and path are required.')
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
          path: path.trim(),
          content_dir: contentDir.trim() || undefined,
          site_url: siteUrl.trim() || undefined,
          site_id: siteId.trim() || undefined,
        })
      } else {
        saved = await createProject({
          name: name.trim(),
          path: path.trim(),
          content_dir: contentDir.trim() || undefined,
          site_url: siteUrl.trim() || undefined,
          site_id: siteId.trim() || undefined,
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

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="proj-url">Site URL</Label>
              <Input
                id="proj-url"
                value={siteUrl}
                onChange={e => setSiteUrl(e.target.value)}
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
                placeholder="my-website"
                className="bg-secondary border-input"
              />
            </div>
          </div>
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
