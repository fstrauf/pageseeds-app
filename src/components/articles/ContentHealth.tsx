import { useState } from 'react'
import { AlertCircle, CheckCircle, RefreshCw, Wrench } from 'lucide-react'
import { scanContentHealth, fixContentDates } from '../../lib/tauri'
import type { CleaningResult, DateFixResult } from '../../lib/types'
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
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'

const ISSUE_BADGE: Record<string, string> = {
  duplicate_title: 'bg-amber-100 text-amber-700 border-transparent',
  missing_frontmatter: 'bg-destructive/40 text-destructive-foreground border-transparent',
  missing_blank_line: 'bg-secondary text-muted-foreground border-transparent',
  future_date: 'bg-amber-100 text-amber-700 border-transparent',
  duplicate_date: 'bg-indigo-100 text-indigo-700 border-transparent',
  missing_date: 'bg-secondary text-muted-foreground border-transparent',
  invalid_format: 'bg-destructive/40 text-destructive-foreground border-transparent',
}

interface ContentHealthProps {
  projectId: string
}

export function ContentHealth({ projectId }: ContentHealthProps) {
  const [cleaning, setCleaning] = useState<CleaningResult | null>(null)
  const [dateResult, setDateResult] = useState<DateFixResult | null>(null)
  const [scanLoading, setScanLoading] = useState(false)
  const [dateLoading, setDateLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState('structural')

  async function handleScan(fix: boolean) {
    setScanLoading(true)
    setError(null)
    try {
      const result = await scanContentHealth(projectId, !fix)
      setCleaning(result)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setScanLoading(false)
    }
  }

  async function handleAnalyseDates(apply: boolean) {
    setDateLoading(true)
    setError(null)
    try {
      const result = await fixContentDates(projectId, !apply)
      setDateResult(result)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setDateLoading(false)
    }
  }

  return (
    <div className="p-6 space-y-6 overflow-y-auto">
      <h2 className="text-base font-semibold text-foreground">Content Health</h2>

      {error && (
        <div className="flex items-start gap-2 px-3 py-2.5 rounded-md text-sm bg-destructive/15 text-destructive">
          <AlertCircle size={14} className="mt-0.5 shrink-0" />
          {error}
        </div>
      )}

      <Tabs value={tab} onValueChange={setTab}>
        <TabsList className="bg-card border border-border">
          <TabsTrigger value="structural" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
            Structural
          </TabsTrigger>
          <TabsTrigger value="dates" className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
            Dates
          </TabsTrigger>
        </TabsList>

        {/* ── Structural Issues ─── */}
        <TabsContent value="structural" className="mt-4 space-y-4">
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => handleScan(false)}
              disabled={scanLoading}
              className="border-border text-muted-foreground hover:text-foreground"
            >
              <RefreshCw size={13} className={scanLoading ? 'animate-spin' : ''} />
              Scan (dry run)
            </Button>
            {cleaning && cleaning.issues.some(i => !i.fixed) && (
              <Button
                size="sm"
                onClick={() => handleScan(true)}
                disabled={scanLoading}
              >
                <Wrench size={13} />
                Apply fixes
              </Button>
            )}
          </div>

          {cleaning && (
            <Card className="bg-card border-border">
              <CardHeader className="pb-3">
                <CardTitle className="text-sm font-semibold text-foreground flex items-center justify-between">
                  <span>Scan results</span>
                  <div className="flex items-center gap-2 text-xs font-normal text-muted-foreground">
                    <span>{cleaning.files_checked} files checked</span>
                    {cleaning.issues_fixed > 0 && (
                      <Badge className="bg-emerald-100 text-emerald-700 border-transparent text-xs">
                        {cleaning.issues_fixed} fixed
                      </Badge>
                    )}
                  </div>
                </CardTitle>
              </CardHeader>
              <CardContent>
                {cleaning.issues.length === 0 ? (
                  <div className="flex items-center gap-2 text-sm text-emerald-600">
                    <CheckCircle size={15} />
                    No structural issues found
                  </div>
                ) : (
                  <div className="rounded-lg border border-border overflow-hidden">
                    <Table>
                      <TableHeader>
                        <TableRow className="bg-card hover:bg-card border-border">
                          <TableHead className="text-xs text-muted-foreground">File</TableHead>
                          <TableHead className="text-xs text-muted-foreground">Issue</TableHead>
                          <TableHead className="text-xs text-muted-foreground">Description</TableHead>
                          <TableHead className="text-xs text-muted-foreground w-16">Fixed</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {cleaning.issues.map((issue, i) => (
                          <TableRow key={i} className="border-border">
                            <TableCell className="font-mono text-xs text-muted-foreground max-w-[160px] truncate">
                              {issue.file}
                            </TableCell>
                            <TableCell>
                              <Badge className={`text-xs ${ISSUE_BADGE[issue.issue_type] ?? 'bg-secondary text-muted-foreground border-transparent'}`}>
                                {issue.issue_type.replace(/_/g, ' ')}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-xs text-foreground max-w-[280px]">
                              {issue.description}
                            </TableCell>
                            <TableCell>
                              {issue.fixed
                                ? <CheckCircle size={14} className="text-emerald-600" />
                                : <span className="text-xs text-muted-foreground">—</span>
                              }
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </TabsContent>

        {/* ── Date Distribution ─── */}
        <TabsContent value="dates" className="mt-4 space-y-4">
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => handleAnalyseDates(false)}
              disabled={dateLoading}
              className="border-border text-muted-foreground hover:text-foreground"
            >
              <RefreshCw size={13} className={dateLoading ? 'animate-spin' : ''} />
              Analyse dates
            </Button>
            {dateResult && dateResult.fixes.length > 0 && dateResult.dry_run && (
              <Button
                size="sm"
                onClick={() => handleAnalyseDates(true)}
                disabled={dateLoading}
              >
                <Wrench size={13} />
                Apply {dateResult.fixes.length} fix{dateResult.fixes.length !== 1 ? 'es' : ''}
              </Button>
            )}
          </div>

          {dateResult && (
            <Card className="bg-card border-border">
              <CardHeader className="pb-3">
                <CardTitle className="text-sm font-semibold text-foreground flex items-center justify-between">
                  <span>Date analysis{dateResult.dry_run ? ' (preview)' : ' (applied)'}</span>
                  <span className="text-xs font-normal text-muted-foreground">
                    {dateResult.articles_fixed} article{dateResult.articles_fixed !== 1 ? 's' : ''} affected
                  </span>
                </CardTitle>
              </CardHeader>
              <CardContent>
                {dateResult.fixes.length === 0 ? (
                  <div className="flex items-center gap-2 text-sm text-emerald-600">
                    <CheckCircle size={15} />
                    No date issues found
                  </div>
                ) : (
                  <div className="rounded-lg border border-border overflow-hidden">
                    <Table>
                      <TableHeader>
                        <TableRow className="bg-card hover:bg-card border-border">
                          <TableHead className="text-xs text-muted-foreground w-16">ID</TableHead>
                          <TableHead className="text-xs text-muted-foreground">Current date</TableHead>
                          <TableHead className="text-xs text-muted-foreground">New date</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {dateResult.fixes.map((fix, i) => (
                          <TableRow key={i} className="border-border">
                            <TableCell className="text-xs text-muted-foreground font-mono">
                              {fix.article_id}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {fix.old_date || '—'}
                            </TableCell>
                            <TableCell className="text-xs text-emerald-600 font-medium">
                              {fix.new_date}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </TabsContent>
      </Tabs>
    </div>
  )
}
