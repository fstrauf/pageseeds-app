import { useEffect, useState } from 'react'
import { CheckCircle, XCircle, RefreshCw, Copy, Bot, FolderOpen } from 'lucide-react'
import { getSecretsStatus, getSecretsFilePath, checkAgentStatus, setAgentProvider, importEnvFile, openFolderDialog } from '../../lib/tauri'
import type { AgentStatus, SecretsStatus } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import {
  Table,
  TableBody,
  TableCell,
  TableRow,
} from '@/components/ui/table'

interface SettingsProps {
  projectId?: string
}

export function Settings({ projectId }: SettingsProps) {
  const [secrets, setSecrets] = useState<SecretsStatus | null>(null)
  const [secretsPath, setSecretsPath] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const [importPath, setImportPath] = useState('')
  const [importWorking, setImportWorking] = useState(false)
  const [importResult, setImportResult] = useState<string | null>(null)
  const [importError, setImportError] = useState<string | null>(null)

  const [agentStatus, setAgentStatus] = useState<AgentStatus | null>(null)
  const [agentLoading, setAgentLoading] = useState(false)
  const [agentSaving, setAgentSaving] = useState(false)
  const [selectedProvider, setSelectedProvider] = useState<string>('copilot')
  const [agentSaved, setAgentSaved] = useState(false)

  async function load() {
    if (!projectId) return
    setLoading(true)
    setError(null)
    try {
      const [s, p] = await Promise.all([
        getSecretsStatus(projectId),
        getSecretsFilePath(),
      ])
      setSecrets(s)
      setSecretsPath(p)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function loadAgentStatus() {
    if (!projectId) return
    setAgentLoading(true)
    try {
      const status = await checkAgentStatus(projectId)
      setAgentStatus(status)
      setSelectedProvider(status.configured_provider)
    } catch {
      // agent check is best-effort
    } finally {
      setAgentLoading(false)
    }
  }

  async function saveProvider() {
    if (!projectId) return
    setAgentSaving(true)
    try {
      await setAgentProvider(projectId, selectedProvider)
      setAgentSaved(true)
      setTimeout(() => setAgentSaved(false), 2000)
      await loadAgentStatus()
    } catch {
      // ignore
    } finally {
      setAgentSaving(false)
    }
  }


  useEffect(() => {
    load()
    loadAgentStatus()
  }, [projectId])

  async function copyPath() {
    if (!secretsPath) return
    await navigator.clipboard.writeText(secretsPath)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  async function pickEnvFile() {
    // openFolderDialog only picks directories; use the input field instead.
    // Provide a quick-pick by opening the parent folder and letting the user type.
    const picked = await openFolderDialog('Select folder containing .env file')
    if (picked) setImportPath(picked.endsWith('.env') ? picked : picked + '/.env')
  }

  async function runImport() {
    if (!importPath.trim()) return
    setImportWorking(true)
    setImportResult(null)
    setImportError(null)
    try {
      const keys = await importEnvFile(importPath.trim())
      setImportResult(
        keys.length > 0
          ? `Imported ${keys.length} key${keys.length > 1 ? 's' : ''}: ${keys.join(', ')}`
          : 'No keys found in that file.'
      )
      await load() // refresh secrets status
    } catch (e) {
      setImportError(String(e))
    } finally {
      setImportWorking(false)
    }
  }

  const SOURCE_LABELS: Record<string, string> = {
    'secrets_env': 'secrets.env',
    'env_local': '.env.local',
    'dotenv': '.env',
    'shell': 'shell env',
  }

  return (
    <div className="p-6 max-w-2xl">
      <h1 className="text-lg font-semibold text-foreground mb-6">Settings</h1>

      {!projectId && (
        <p className="text-sm text-muted-foreground">Select a project to view settings.</p>
      )}

      {projectId && (
        <div className="space-y-6">
          {/* Secrets section */}
          <Card className="bg-card border-border">
            <CardHeader className="flex flex-row items-center justify-between pb-3">
              <CardTitle className="text-sm font-semibold text-foreground">
                Secrets &amp; Environment
              </CardTitle>
              <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
                <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
              </Button>
            </CardHeader>
            <CardContent className="space-y-4">
              {error && (
                <div className="px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
                  {error}
                </div>
              )}

              {secretsPath && (
                <div className="flex items-center gap-2 px-3 py-2.5 rounded-md border border-border bg-secondary">
                  <div className="flex-1 min-w-0">
                    <div className="text-xs text-muted-foreground mb-0.5">Secrets file</div>
                    <div className="text-xs font-mono truncate text-foreground">{secretsPath}</div>
                  </div>
                  <Button variant="outline" size="xs" onClick={copyPath} className="border-border text-muted-foreground shrink-0">
                    {copied ? 'Copied!' : <Copy size={12} />}
                  </Button>
                </div>
              )}

              {secrets && (
                <div className="rounded-lg border border-border overflow-hidden">
                  <Table>
                    <TableBody>
                      {secrets.secrets.map(status => (
                        <TableRow key={status.key} className="border-border">
                          <TableCell className="pl-4 w-8">
                            {status.configured
                              ? <CheckCircle size={15} className="text-[var(--color-success)] shrink-0" />
                              : <XCircle size={15} className="text-destructive shrink-0" />
                            }
                          </TableCell>
                          <TableCell>
                            <div className="text-sm font-mono text-foreground">{status.key}</div>
                            {status.source && (
                              <div className="text-xs text-muted-foreground mt-0.5">
                                from {SOURCE_LABELS[status.source] ?? status.source}
                              </div>
                            )}
                          </TableCell>
                          <TableCell className="text-right pr-4">
                            <Badge
                              variant="outline"
                              className={status.configured
                                ? 'border-transparent bg-emerald-100 text-emerald-700'
                                : 'border-transparent bg-destructive/15 text-destructive'}
                            >
                              {status.configured ? 'configured' : 'missing'}
                            </Badge>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}

              {!loading && !secrets && !error && (
                <p className="text-sm text-muted-foreground">No secrets data available.</p>
              )}

              {/* One-time .env import */}
              <div className="mt-4 pt-4 border-t border-border space-y-2">
                <div className="text-xs font-medium text-foreground">Import credentials from .env file</div>
                <p className="text-xs text-muted-foreground">
                  One-time migration: reads an existing <code>.env</code> and merges the values
                  into <code className="font-mono">~/.config/automation/secrets.env</code>.
                </p>
                <div className="flex gap-2 items-center">
                  <input
                    type="text"
                    value={importPath}
                    onChange={e => setImportPath(e.target.value)}
                    placeholder="/path/to/automation/.env"
                    className="flex-1 px-2 py-1 rounded-md border border-border bg-secondary text-xs font-mono text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                  <Button variant="outline" size="xs" onClick={pickEnvFile} className="border-border text-muted-foreground shrink-0" title="Browse">
                    <FolderOpen size={12} />
                  </Button>
                  <Button
                    variant="default"
                    size="xs"
                    onClick={runImport}
                    disabled={importWorking || !importPath.trim()}
                    className="shrink-0"
                  >
                    {importWorking ? 'Importing…' : 'Import'}
                  </Button>
                </div>
                {importResult && (
                  <div className="px-3 py-1.5 rounded-md text-xs bg-emerald-100 text-emerald-700">{importResult}</div>
                )}
                {importError && (
                  <div className="px-3 py-1.5 rounded-md text-xs bg-destructive/15 text-destructive">{importError}</div>
                )}
              </div>
            </CardContent>
          </Card>

          <Separator className="bg-border" />

          {/* Agent configuration section */}
          <Card className="bg-card border-border">
            <CardHeader className="flex flex-row items-center justify-between pb-3">
              <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot size={14} className="text-muted-foreground" />
                Agent
              </CardTitle>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={loadAgentStatus}
                disabled={agentLoading}
                className="text-muted-foreground"
              >
                <RefreshCw size={13} className={agentLoading ? 'animate-spin' : ''} />
              </Button>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* Available CLIs */}
              {agentStatus && (
                <div className="rounded-lg border border-border overflow-hidden">
                  <Table>
                    <TableBody>
                      {agentStatus.available_agents.map(a => (
                        <TableRow key={a.name} className="border-border">
                          <TableCell className="pl-4 w-8">
                            {a.available
                              ? <CheckCircle size={15} className="text-[var(--color-success)] shrink-0" />
                              : <XCircle size={15} className="text-destructive shrink-0" />
                            }
                          </TableCell>
                          <TableCell>
                            <div className="text-sm font-mono text-foreground">{a.name}</div>
                            {a.version && (
                              <div className="text-xs text-muted-foreground mt-0.5">{a.version}</div>
                            )}
                          </TableCell>
                          <TableCell className="text-right pr-4">
                            <Badge
                              variant="outline"
                              className={a.available
                                ? 'border-transparent bg-emerald-100 text-emerald-700'
                                : 'border-transparent bg-destructive/15 text-destructive'}
                            >
                              {a.available ? 'available' : 'not found'}
                            </Badge>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}

              {/* Provider selector */}
              <div className="flex items-center gap-3">
                <label className="text-xs text-muted-foreground w-20 shrink-0">Provider</label>
                <select
                  value={selectedProvider}
                  onChange={e => setSelectedProvider(e.target.value)}
                  className="flex-1 text-sm bg-secondary border border-border rounded-md px-3 py-1.5 text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                >
                  <option value="copilot">copilot</option>
                  <option value="claude">claude</option>
                </select>
                <Button
                  variant="outline"
                  size="xs"
                  onClick={saveProvider}
                  disabled={agentSaving}
                  className="border-border text-foreground shrink-0"
                >
                  {agentSaved ? 'Saved!' : agentSaving ? 'Saving…' : 'Save'}
                </Button>
              </div>

              {!agentLoading && !agentStatus && (
                <p className="text-sm text-muted-foreground">Unable to detect agents. Select a project first.</p>
              )}
            </CardContent>
          </Card>

          <Separator className="bg-border" />
          <Card className="bg-card border-border">
            <CardHeader className="pb-3">
              <CardTitle className="text-sm font-semibold text-foreground">About</CardTitle>
            </CardHeader>
            <CardContent>
              <Table>
                <TableBody>
                  {([['App', 'PageSeeds Desktop'], ['Version', '0.1.0']] as const).map(([label, value]) => (
                    <TableRow key={label} className="border-none">
                      <TableCell className="pl-0 py-1 w-28 text-sm text-muted-foreground">{label}</TableCell>
                      <TableCell className="py-1 text-sm text-foreground">{value}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  )
}

