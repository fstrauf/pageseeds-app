import { useEffect, useState } from 'react'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { Label } from '../ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card'
import { Badge } from '../ui/badge'
import { clarityGetStatus, clarityTestConnection, updateProject } from '../../lib/tauri'
import type { ClarityConnectionStatus, Project } from '../../lib/types'

interface Props {
  projectId: string
  project?: Project
  onChanged?: () => void
}

export function ClaritySettings({ project, onChanged }: Props) {
  const [status, setStatus] = useState<ClarityConnectionStatus | null>(null)
  const [projectIdInput, setProjectIdInput] = useState('')
  const [testing, setTesting] = useState(false)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)

  useEffect(() => {
    if (project) {
      setProjectIdInput(project.clarity_project_id ?? '')
      clarityGetStatus(project).then(setStatus).catch(console.error)
    }
  }, [project])

  const currentProject = project
    ? { ...project, clarity_project_id: projectIdInput.trim() || null }
    : undefined

  const handleTest = async () => {
    if (!currentProject) return
    setTesting(true)
    setMessage(null)
    try {
      const result = await clarityTestConnection(currentProject)
      setStatus(result)
      setMessage(result.connected ? 'Connection successful' : result.message)
    } catch (e) {
      setMessage(`Test failed: ${e}`)
    } finally {
      setTesting(false)
    }
  }

  const handleSaveProjectId = async () => {
    if (!project) return
    setSaving(true)
    setMessage(null)
    try {
      await updateProject({ ...project, clarity_project_id: projectIdInput.trim() || null })
      setMessage('Project ID saved')
      const newStatus = await clarityGetStatus({ ...project, clarity_project_id: projectIdInput.trim() || null })
      setStatus(newStatus)
      onChanged?.()
    } catch (e) {
      setMessage(`Save failed: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="space-y-6 max-w-2xl">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Microsoft Clarity Connection</CardTitle>
          <CardDescription>
            Connect PageSeeds to your Clarity project to collect behavioral metrics and
            surface UX/SEO findings.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">Status:</span>
            {status ? (
              <Badge variant={status.connected ? 'default' : 'secondary'}>
                {status.connected ? 'Connected' : 'Not connected'}
              </Badge>
            ) : (
              <Badge variant="outline">Loading…</Badge>
            )}
          </div>

          {status && (
            <p className="text-sm text-muted-foreground">{status.message}</p>
          )}

          <div className="space-y-2">
            <Label htmlFor="clarity-project-id">Clarity Project ID</Label>
            <Input
              id="clarity-project-id"
              placeholder="e.g. w6k9cmvgx3"
              value={projectIdInput}
              onChange={e => setProjectIdInput(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              Find this in your Clarity project settings under Setup → How to install.
            </p>
          </div>

          <div className="flex gap-2">
            <Button onClick={handleSaveProjectId} disabled={saving || !projectIdInput.trim()}>
              {saving ? 'Saving…' : 'Save Project ID'}
            </Button>
            <Button onClick={handleTest} disabled={testing} variant="outline">
              {testing ? 'Testing…' : 'Test Connection'}
            </Button>
          </div>

          {message && (
            <p className={`text-sm ${message.includes('failed') ? 'text-destructive' : 'text-green-600'}`}>
              {message}
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">API Token</CardTitle>
          <CardDescription>
            Clarity requires a{' '}
            <a
              href="https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-api"
              target="_blank"
              rel="noopener noreferrer"
              className="underline"
            >
              Data Export API token
            </a>{' '}
            from your Clarity project.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            Add <code className="text-xs bg-secondary px-1 rounded">CLARITY_API_TOKEN</code> in
            Settings → Secrets. The token is stored outside the repo and never committed.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
