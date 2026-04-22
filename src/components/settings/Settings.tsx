import { useEffect, useState, useCallback } from 'react'
import { CheckCircle, XCircle, RefreshCw, Copy, Bot, FolderOpen, FileText, ExternalLink, ScrollText, Globe } from 'lucide-react'
import { LogViewer } from './LogViewer'
import { getSecretsStatus, getSecretsFilePath, checkAgentStatus, setAgentProvider, importEnvFile, openFolderDialog, getProjectConfigFilesStatus, getSeoProvider, setSeoProvider } from '../../lib/tauri'
import type { AgentStatus, ProjectConfigFileStatus, SecretsStatus, SeoProvider } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Table,
  TableBody,
  TableCell,
  TableRow,
} from '@/components/ui/table'
import { useErrorHandler } from '../../lib/toast-context'

interface SettingsProps {
  projectId?: string
}

export function Settings({ projectId }: SettingsProps) {
  const { showError } = useErrorHandler()
  const [secrets, setSecrets] = useState<SecretsStatus | null>(null)
  const [secretsPath, setSecretsPath] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [copied, setCopied] = useState(false)

  const [importPath, setImportPath] = useState('')
  const [importWorking, setImportWorking] = useState(false)
  const [importResult, setImportResult] = useState<string | null>(null)

  const [agentStatus, setAgentStatus] = useState<AgentStatus | null>(null)
  const [agentLoading, setAgentLoading] = useState(false)
  const [agentSaving, setAgentSaving] = useState(false)
  const [selectedProvider, setSelectedProvider] = useState<string>('kimi')
  const [agentSaved, setAgentSaved] = useState(false)
  const [hasUnsavedProviderChange, setHasUnsavedProviderChange] = useState(false)

  const [configFiles, setConfigFiles] = useState<ProjectConfigFileStatus[]>([])
  const [configLoading, setConfigLoading] = useState(false)

  // SEO Provider state
  const [seoProvider, setSeoProviderState] = useState<SeoProvider>('ahrefs')
  const [seoLoading, setSeoLoading] = useState(false)
  const [seoSaving, setSeoSaving] = useState(false)
  const [seoSaved, setSeoSaved] = useState(false)

  const load = useCallback(async () => {
    if (!projectId) return
    setLoading(true)
    try {
      const [s, p] = await Promise.all([
        getSecretsStatus(projectId),
        getSecretsFilePath(),
      ])
      setSecrets(s)
      setSecretsPath(p)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId, showError])

  const loadConfigFiles = useCallback(async () => {
    if (!projectId) return
    setConfigLoading(true)
    try {
      const files = await getProjectConfigFilesStatus(projectId)
      setConfigFiles(files)
    } catch {
      // keep UI responsive even if config scan fails
      setConfigFiles([])
    } finally {
      setConfigLoading(false)
    }
  }, [projectId])

  const loadAgentStatus = useCallback(async (force = false) => {
    setAgentLoading(true)
    try {
      const status = await checkAgentStatus()
      setAgentStatus(status)
      // Only update selected provider if user hasn't made unsaved changes (unless forced)
      if (force || !hasUnsavedProviderChange) {
        setSelectedProvider(status.configured_provider)
      }
    } catch (e) {
      console.error('[Settings] Failed to load agent status:', e)
    } finally {
      setAgentLoading(false)
    }
  }, [hasUnsavedProviderChange])

  const saveProvider = useCallback(async () => {
    setAgentSaving(true)
    try {
      await setAgentProvider(selectedProvider)
      setAgentSaved(true)
      setHasUnsavedProviderChange(false)
      // Reload to verify the save persisted (force refresh)
      await loadAgentStatus(true)
      setTimeout(() => setAgentSaved(false), 2000)
    } catch (e) {
      showError(String(e))
    } finally {
      setAgentSaving(false)
    }
  }, [selectedProvider, loadAgentStatus, showError])

  // Load SEO provider
  const loadSeoProvider = useCallback(async () => {
    if (!projectId) return
    setSeoLoading(true)
    try {
      const provider = await getSeoProvider(projectId)
      setSeoProviderState(provider as SeoProvider)
    } catch (e) {
      showError(String(e))
    } finally {
      setSeoLoading(false)
    }
  }, [projectId, showError])

  // Save SEO provider
  const saveSeoProvider = useCallback(async () => {
    if (!projectId) return
    setSeoSaving(true)
    try {
      await setSeoProvider(projectId, seoProvider)
      setSeoSaved(true)
      // Reload to verify the save persisted
      await loadSeoProvider()
      setTimeout(() => setSeoSaved(false), 2000)
    } catch (e) {
      showError(String(e))
    } finally {
      setSeoSaving(false)
    }
  }, [projectId, seoProvider, loadSeoProvider, showError])

  // Load global settings once on mount
  useEffect(() => {
    loadAgentStatus()
  }, [loadAgentStatus])

  // Load project-specific settings when project changes
  useEffect(() => {
    // Reset unsaved change tracking when project changes
    setHasUnsavedProviderChange(false)
    load()
    loadConfigFiles()
    loadSeoProvider()
  }, [load, loadConfigFiles, loadSeoProvider, projectId])

  const copyPath = useCallback(async () => {
    if (!secretsPath) return
    await navigator.clipboard.writeText(secretsPath)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }, [secretsPath])

  const pickEnvFile = useCallback(async () => {
    const picked = await openFolderDialog('Select folder containing .env file')
    if (picked) setImportPath(picked.endsWith('.env') ? picked : picked + '/.env')
  }, [])

  const runImport = useCallback(async () => {
    if (!importPath.trim()) return
    setImportWorking(true)
    setImportResult(null)
    try {
      const keys = await importEnvFile(importPath.trim())
      setImportResult(
        keys.length > 0
          ? `Imported ${keys.length} key${keys.length > 1 ? 's' : ''}: ${keys.join(', ')}`
          : 'No keys found in that file.'
      )
      await load() // refresh secrets status
    } catch (e) {
      showError(String(e))
    } finally {
      setImportWorking(false)
    }
  }, [importPath, load, showError])

  const SOURCE_LABELS: Record<string, string> = {
    'secrets_env': 'secrets.env',
    'env_local': '.env.local',
    'dotenv': '.env',
    'shell': 'shell env',
  }

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex-1 min-h-0">
        <ScrollArea className="h-full">
          <div className="p-6 max-w-2xl pb-20">
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

                    {!loading && !secrets && (
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
                    </div>
                  </CardContent>
                </Card>

                <Separator className="bg-border" />

                <Card className="bg-card border-border">
                  <CardHeader className="flex flex-row items-center justify-between pb-3">
                    <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-2">
                      <FileText size={14} className="text-muted-foreground" />
                      Project Configuration Files
                    </CardTitle>
                    <Button variant="ghost" size="icon-sm" onClick={loadConfigFiles} disabled={configLoading} className="text-muted-foreground">
                      <RefreshCw size={13} className={configLoading ? 'animate-spin' : ''} />
                    </Button>
                  </CardHeader>
                  <CardContent>
                    {configFiles.length > 0 ? (
                      <div className="space-y-3">
                        {configFiles.map(file => (
                          <div 
                            key={file.id} 
                            className="flex items-start gap-3 p-3 rounded-lg border border-border bg-secondary/30"
                          >
                            <div className="mt-0.5 shrink-0">
                              {file.configured
                                ? <CheckCircle size={16} className="text-[var(--color-success)]" />
                                : <XCircle size={16} className="text-destructive" />
                              }
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center gap-2 flex-wrap">
                                <span className="text-sm font-medium text-foreground">{file.label}</span>
                                {file.required && (
                                  <Badge variant="outline" className="border-transparent bg-blue-100 text-blue-700 text-[10px] px-1.5 py-0">
                                    required
                                  </Badge>
                                )}
                                <Badge
                                  variant="outline"
                                  className={file.configured
                                    ? 'border-transparent bg-emerald-100 text-emerald-700 text-[10px] px-1.5 py-0'
                                    : 'border-transparent bg-destructive/15 text-destructive text-[10px] px-1.5 py-0'}
                                >
                                  {file.configured ? 'configured' : 'missing'}
                                </Badge>
                              </div>
                              <div className="mt-1.5 space-y-1">
                                <a 
                                  href={file.full_link}
                                  className="inline-flex items-center gap-1 text-xs font-mono text-muted-foreground hover:text-primary hover:underline max-w-full"
                                  title={file.full_path}
                                >
                                  <span className="truncate">{file.relative_path}</span>
                                  <ExternalLink size={10} className="shrink-0 opacity-60" />
                                </a>
                                <p className="text-xs text-muted-foreground">{file.used_by}</p>
                                {file.detail && (
                                  <p className="text-xs text-muted-foreground/80">{file.detail}</p>
                                )}
                              </div>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <p className="text-sm text-muted-foreground">
                        {configLoading ? 'Checking configuration files…' : 'No project config files detected.'}
                      </p>
                    )}
                  </CardContent>
                </Card>

                <Separator className="bg-border" />

                {/* SEO Provider section */}
                <Card className="bg-card border-border">
                  <CardHeader className="flex flex-row items-center justify-between pb-3">
                    <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-2">
                      <Globe size={14} className="text-muted-foreground" />
                      SEO Data Provider
                    </CardTitle>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={loadSeoProvider}
                      disabled={seoLoading}
                      className="text-muted-foreground"
                    >
                      <RefreshCw size={13} className={seoLoading ? 'animate-spin' : ''} />
                    </Button>
                  </CardHeader>
                  <CardContent className="space-y-4">
                    <div className="space-y-3">
                      <label className="flex items-start gap-3 p-3 rounded-lg border border-border bg-secondary/30 cursor-pointer hover:bg-secondary/50 transition-colors">
                        <input
                          type="radio"
                          name="seo-provider"
                          value="ahrefs"
                          checked={seoProvider === 'ahrefs'}
                          onChange={(e) => setSeoProviderState(e.target.value as SeoProvider)}
                          className="mt-0.5 shrink-0"
                        />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-foreground">Ahrefs (Free)</span>
                            <Badge variant="outline" className="border-transparent bg-emerald-100 text-emerald-700 text-[10px] px-1.5 py-0">
                              Free
                            </Badge>
                          </div>
                          <p className="text-xs text-muted-foreground mt-1">
                            Uses Ahrefs free tier with CapSolver. Returns categorical volume labels (e.g., "MoreThanThousand").
                            Requires CAPSOLVER_API_KEY.
                          </p>
                        </div>
                      </label>

                      <label className="flex items-start gap-3 p-3 rounded-lg border border-border bg-secondary/30 cursor-pointer hover:bg-secondary/50 transition-colors">
                        <input
                          type="radio"
                          name="seo-provider"
                          value="dataforseo"
                          checked={seoProvider === 'dataforseo'}
                          onChange={(e) => setSeoProviderState(e.target.value as SeoProvider)}
                          className="mt-0.5 shrink-0"
                        />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-foreground">DataForSEO (Paid)</span>
                            <Badge variant="outline" className="border-transparent bg-blue-100 text-blue-700 text-[10px] px-1.5 py-0">
                              Paid
                            </Badge>
                          </div>
                          <p className="text-xs text-muted-foreground mt-1">
                            Pay-as-you-go API with precise numeric volumes, CPC data, and competition scores.
                            Requires DATAFORSEO_LOGIN and DATAFORSEO_PASSWORD.
                          </p>
                        </div>
                      </label>
                    </div>

                    <div className="flex items-center gap-3 pt-2">
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={saveSeoProvider}
                        disabled={seoSaving}
                        className="border-border text-foreground"
                      >
                        {seoSaved ? 'Saved!' : seoSaving ? 'Saving…' : 'Save Provider'}
                      </Button>
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
                      onClick={() => {
                        void loadAgentStatus()
                      }}
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
                        onChange={e => {
                          setSelectedProvider(e.target.value)
                          setHasUnsavedProviderChange(true)
                        }}
                        className="flex-1 text-sm bg-secondary border border-border rounded-md px-3 py-1.5 text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                      >
                        <option value="copilot">copilot</option>
                        <option value="claude">claude</option>
                        <option value="kimi">kimi</option>
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

                {/* Logs section */}
                <Card className="bg-card border-border">
                  <CardHeader className="pb-3">
                    <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-2">
                      <ScrollText size={14} className="text-muted-foreground" />
                      Application Logs
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <LogViewer />
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
        </ScrollArea>
      </div>
    </div>
  )
}
