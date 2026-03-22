import { useState } from 'react'
import { AlertCircle, RefreshCw, Link2, ExternalLink } from 'lucide-react'
import { scanContentLinks } from '../../lib/tauri'
import type { LinkScanResult } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

interface LinkingMapProps {
  projectId: string
}

export function LinkingMap({ projectId }: LinkingMapProps) {
  const [result, setResult] = useState<LinkScanResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showOrphansOnly, setShowOrphansOnly] = useState(false)

  async function handleScan() {
    setLoading(true)
    setError(null)
    try {
      const data = await scanContentLinks(projectId)
      setResult(data)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const profiles = result
    ? showOrphansOnly
      ? result.profiles.filter(p => result.orphan_ids.includes(p.article_id))
      : result.profiles
    : []

  return (
    <div className="p-6 space-y-6 overflow-y-auto">
      <div className="flex items-center justify-between">
        <h2 className="text-base font-semibold text-foreground">Internal Linking</h2>
        <Button
          size="sm"
          variant="outline"
          onClick={handleScan}
          disabled={loading}
          className="border-border text-muted-foreground hover:text-foreground"
        >
          <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
          Scan links
        </Button>
      </div>

      {error && (
        <div className="flex items-start gap-2 px-3 py-2.5 rounded-md text-sm bg-destructive/15 text-destructive">
          <AlertCircle size={14} className="mt-0.5 shrink-0" />
          {error}
        </div>
      )}

      {result && (
        <>
          {/* Summary stats */}
          <div className="grid grid-cols-4 gap-3">
            {(
              [
                { label: 'Total links', value: result.total_links },
                { label: 'With outgoing', value: result.articles_with_outgoing },
                { label: 'With incoming', value: result.articles_with_incoming },
                {
                  label: 'Orphans',
                  value: result.orphan_ids.length,
                  highlight: result.orphan_ids.length > 0,
                },
              ] as Array<{ label: string; value: number; highlight?: boolean }>
            ).map(stat => (
              <Card key={stat.label} className="bg-card border-border">
                <CardContent className="pt-3 pb-3">
                  <p className="text-xs text-muted-foreground mb-0.5">{stat.label}</p>
                  <p className={`text-xl font-bold ${stat.highlight ? 'text-amber-600' : 'text-foreground'}`}>
                    {stat.value}
                  </p>
                </CardContent>
              </Card>
            ))}
          </div>

          {/* Profile table */}
          <Card className="bg-card border-border">
            <CardHeader className="pb-3">
              <CardTitle className="text-sm font-semibold text-foreground flex items-center justify-between">
                <span>Link profiles</span>
                <button
                  onClick={() => setShowOrphansOnly(v => !v)}
                  className={`text-xs px-2 py-0.5 rounded ${
                    showOrphansOnly
                      ? 'bg-amber-100 text-amber-700'
                      : 'text-muted-foreground hover:text-foreground'
                  }`}
                >
                  {showOrphansOnly ? 'Orphans only' : 'All articles'}
                </button>
              </CardTitle>
            </CardHeader>
            <CardContent>
              {profiles.length === 0 ? (
                <p className="text-sm text-muted-foreground">No profiles to display.</p>
              ) : (
                <div className="rounded-lg border border-border overflow-hidden">
                  <Table>
                    <TableHeader>
                      <TableRow className="bg-card hover:bg-card border-border">
                        <TableHead className="text-xs text-muted-foreground w-16">ID</TableHead>
                        <TableHead className="text-xs text-muted-foreground">Title</TableHead>
                        <TableHead className="text-xs text-muted-foreground text-center w-24">Outgoing</TableHead>
                        <TableHead className="text-xs text-muted-foreground text-center w-24">Incoming</TableHead>
                        <TableHead className="text-xs text-muted-foreground text-center w-24">Unresolved</TableHead>
                        <TableHead className="text-xs text-muted-foreground w-20">Status</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {profiles.map(profile => {
                        const isOrphan = result.orphan_ids.includes(profile.article_id)
                        return (
                          <TableRow key={profile.article_id} className="border-border">
                            <TableCell className="font-mono text-xs text-muted-foreground">
                              {profile.article_id}
                            </TableCell>
                            <TableCell className="text-xs text-foreground max-w-[240px] truncate">
                              {profile.title}
                            </TableCell>
                            <TableCell className="text-center">
                              {profile.outgoing_ids.length > 0 ? (
                                <span className="inline-flex items-center gap-1 text-xs text-emerald-600">
                                  <Link2 size={11} />
                                  {profile.outgoing_ids.length}
                                </span>
                              ) : (
                                <span className="text-xs text-muted-foreground">0</span>
                              )}
                            </TableCell>
                            <TableCell className="text-center">
                              {profile.incoming_ids.length > 0 ? (
                                <span className="inline-flex items-center gap-1 text-xs text-sky-400">
                                  <ExternalLink size={11} />
                                  {profile.incoming_ids.length}
                                </span>
                              ) : (
                                <span className="text-xs text-muted-foreground">0</span>
                              )}
                            </TableCell>
                            <TableCell className="text-center">
                              {profile.unresolved_links.length > 0 ? (
                                <Badge className="bg-destructive/40 text-destructive-foreground border-transparent text-xs">
                                  {profile.unresolved_links.length}
                                </Badge>
                              ) : (
                                <span className="text-xs text-muted-foreground">—</span>
                              )}
                            </TableCell>
                            <TableCell>
                              {isOrphan ? (
                                <Badge className="bg-amber-100 text-amber-700 border-transparent text-xs">
                                  orphan
                                </Badge>
                              ) : (
                                <span className="text-xs text-emerald-600">linked</span>
                              )}
                            </TableCell>
                          </TableRow>
                        )
                      })}
                    </TableBody>
                  </Table>
                </div>
              )}
            </CardContent>
          </Card>
        </>
      )}

      {!result && !loading && (
        <div className="text-sm text-muted-foreground">
          Run a scan to see internal link profiles for your content.
        </div>
      )}
    </div>
  )
}
