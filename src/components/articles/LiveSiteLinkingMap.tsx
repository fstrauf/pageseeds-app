import { useMemo, useState } from 'react'
import { AlertCircle, ExternalLink, Link2, RefreshCw } from 'lucide-react'
import { scanLiveSiteLinks } from '../../lib/tauri'
import type { LiveSiteLinkProfile } from '../../lib/types'
import { useQuery } from '../../hooks/useQuery'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

interface LiveSiteLinkingMapProps {
  projectId: string
}

export function LiveSiteLinkingMap({ projectId }: LiveSiteLinkingMapProps) {
  const [query, setQuery] = useState('')
  const [showOrphansOnly, setShowOrphansOnly] = useState(false)

  const { data, error, isLoading, refetch } = useQuery(
    `live-site-links-${projectId}`,
    () => scanLiveSiteLinks(projectId),
    { enabled: !!projectId, staleTime: 0 },
  )

  const orphanSet = useMemo(() => new Set(data?.orphan_urls ?? []), [data?.orphan_urls])

  const filteredProfiles = useMemo(() => {
    const profiles = data?.profiles ?? []
    const needle = query.trim().toLowerCase()
    return profiles.filter(profile => {
      const matchesQuery = !needle ||
        profile.title.toLowerCase().includes(needle) ||
        profile.path.toLowerCase().includes(needle) ||
        profile.url.toLowerCase().includes(needle)

      if (!matchesQuery) return false
      if (!showOrphansOnly) return true
      return orphanSet.has(profile.url)
    })
  }, [data?.profiles, orphanSet, query, showOrphansOnly])

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-border px-6 py-4">
        <div>
          <h2 className="text-sm font-semibold text-foreground">Live Site Links</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            Link graph generated from imported site inventory only.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            value={query}
            onChange={event => setQuery(event.target.value)}
            placeholder="Filter pages"
            className="h-8 w-48 bg-card text-xs"
          />
          <Button
            size="sm"
            variant={showOrphansOnly ? 'default' : 'outline'}
            onClick={() => setShowOrphansOnly(value => !value)}
            className="h-8 text-xs"
          >
            {showOrphansOnly ? 'Showing orphans' : 'Orphans only'}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => refetch()}
            disabled={isLoading}
            className="text-muted-foreground"
          >
            <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-4 flex items-start gap-2 rounded-md bg-destructive/15 px-3 py-2.5 text-sm text-destructive">
          <AlertCircle size={14} className="mt-0.5 shrink-0" />
          {error.message}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          {[
            { label: 'Pages', value: data?.total_pages ?? 0 },
            { label: 'Internal links', value: data?.total_internal_links ?? 0 },
            { label: 'With incoming', value: data?.pages_with_incoming ?? 0 },
            { label: 'With outgoing', value: data?.pages_with_outgoing ?? 0 },
            { label: 'Orphans', value: data?.orphan_urls.length ?? 0 },
          ].map(stat => (
            <Card key={stat.label} className="bg-card border-border">
              <CardContent className="pt-4 pb-3 px-4">
                <div className="text-xs text-muted-foreground mb-1">{stat.label}</div>
                <div className="text-2xl font-bold text-foreground">{stat.value}</div>
              </CardContent>
            </Card>
          ))}
        </div>

        <Card className="bg-card border-border">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-semibold text-foreground">
              Link profiles
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading && filteredProfiles.length === 0 ? (
              <div className="py-8 text-center text-sm text-muted-foreground">Loading link graph…</div>
            ) : filteredProfiles.length === 0 ? (
              <div className="py-8 text-center text-sm text-muted-foreground">
                No imported link profiles match the current filters.
              </div>
            ) : (
              <div className="rounded-lg border border-border overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow className="bg-card hover:bg-card border-border">
                      <TableHead className="text-xs text-muted-foreground">Page</TableHead>
                      <TableHead className="w-24 text-center text-xs text-muted-foreground">Outgoing</TableHead>
                      <TableHead className="w-24 text-center text-xs text-muted-foreground">Incoming</TableHead>
                      <TableHead className="w-24 text-center text-xs text-muted-foreground">Unresolved</TableHead>
                      <TableHead className="w-24 text-xs text-muted-foreground">Status</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredProfiles.map((profile: LiveSiteLinkProfile) => {
                      const isOrphan = orphanSet.has(profile.url)
                      return (
                        <TableRow key={profile.url} className="border-border align-top">
                          <TableCell>
                            <div className="max-w-md">
                              <div className="truncate text-sm font-medium text-foreground">{profile.title || profile.path}</div>
                              <div className="mt-0.5 truncate text-xs text-muted-foreground">{profile.path}</div>
                            </div>
                          </TableCell>
                          <TableCell className="text-center">
                            {profile.outgoing_urls.length > 0 ? (
                              <span className="inline-flex items-center gap-1 text-xs text-emerald-600">
                                <Link2 size={11} />
                                {profile.outgoing_urls.length}
                              </span>
                            ) : (
                              <span className="text-xs text-muted-foreground">0</span>
                            )}
                          </TableCell>
                          <TableCell className="text-center">
                            {profile.incoming_urls.length > 0 ? (
                              <span className="inline-flex items-center gap-1 text-xs text-sky-500">
                                <ExternalLink size={11} />
                                {profile.incoming_urls.length}
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
      </div>
    </div>
  )
}