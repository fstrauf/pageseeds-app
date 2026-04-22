import { useEffect, useState } from 'react'
import { Plus, LayoutGrid, BarChart3, Settings } from 'lucide-react'
import { listSocialCampaigns, getSocialCampaignStats, listSocialTemplates } from '../../lib/tauri'
import type { CampaignStats } from '../../lib/types'
import { CampaignList } from './CampaignList'
import { CampaignCreate } from './CampaignCreate'
import { TemplateList } from './TemplateList'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import { useQuery } from '../../hooks/useQuery'
import { useErrorHandler } from '../../lib/toast-context'

interface Props {
  projectId: string
}

type View = 'campaigns' | 'templates' | 'stats'

function statNumber(value: number | bigint | null | undefined): number {
  return Number(value ?? 0)
}

export function SocialDashboard({ projectId }: Props) {
  const { showError } = useErrorHandler()
  const [view, setView] = useState<View>('campaigns')
  const [showCreate, setShowCreate] = useState(false)

  const { data, isLoading: loading, refetch, error: queryError } = useQuery(
    `social-dashboard-${projectId}`,
    async () => {
      const [campaignsData, templatesData] = await Promise.all([
        listSocialCampaigns(projectId),
        listSocialTemplates(projectId),
      ])

      // Load stats for each campaign
      const statsMap: Record<string, CampaignStats> = {}
      for (const campaign of campaignsData) {
        try {
          const campaignStats = await getSocialCampaignStats(campaign.id)
          statsMap[campaign.id] = campaignStats
        } catch (e) {
          console.warn(`Failed to load stats for campaign ${campaign.id}:`, e)
        }
      }
      return { campaigns: campaignsData, templates: templatesData, stats: statsMap }
    },
    { enabled: !!projectId, staleTime: 0 }
  )

  useEffect(() => {
    if (queryError) {
      showError(queryError.message)
    }
  }, [queryError, showError])

  const campaigns = data?.campaigns ?? []
  const templates = data?.templates ?? []
  const stats = data?.stats ?? {}

  const totalPosts = Object.values(stats).reduce((sum, s) => sum + statNumber(s.total_posts), 0)
  const totalDrafts = Object.values(stats).reduce(
    (sum, s) => sum + statNumber(s.by_status.draft),
    0
  )
  const totalScheduled = Object.values(stats).reduce(
    (sum, s) => sum + statNumber(s.by_status.scheduled),
    0
  )
  const totalPosted = Object.values(stats).reduce(
    (sum, s) => sum + statNumber(s.by_status.posted),
    0
  )

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-border">
        <div>
          <h1 className="text-xl font-semibold text-foreground">
            Social Media Marketing
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            Generate and manage social content for your project
          </p>
        </div>
        <Button onClick={() => setShowCreate(true)}>
          <Plus className="w-4 h-4 mr-2" />
          New Campaign
        </Button>
      </div>

      {/* Stats Bar */}
      <div className="grid grid-cols-4 gap-4 px-6 py-4 border-b border-border bg-muted/30">
        <StatCard label="Total Posts" value={totalPosts} />
        <StatCard label="In Draft" value={totalDrafts} color="amber" />
        <StatCard label="Scheduled" value={totalScheduled} color="blue" />
        <StatCard label="Posted" value={totalPosted} color="emerald" />
      </div>

      {/* Navigation */}
      <div className="flex items-center gap-1 px-6 py-2 border-b border-border">
        <NavButton
          active={view === 'campaigns'}
          onClick={() => setView('campaigns')}
          icon={<LayoutGrid className="w-4 h-4" />}
          label="Campaigns"
        />
        <NavButton
          active={view === 'templates'}
          onClick={() => setView('templates')}
          icon={<Settings className="w-4 h-4" />}
          label="Templates"
        />
        <NavButton
          active={view === 'stats'}
          onClick={() => setView('stats')}
          icon={<BarChart3 className="w-4 h-4" />}
          label="Analytics"
        />
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-6">
        {loading && campaigns.length === 0 ? (
          <div className="flex items-center justify-center h-64 text-muted-foreground">
            Loading...
          </div>
        ) : (
          <>
            {view === 'campaigns' && (
              <CampaignList
                campaigns={campaigns}
                stats={stats}
                onRefresh={refetch}
                projectId={projectId}
              />
            )}
            {view === 'templates' && (
              <TemplateList
                templates={templates}
                projectId={projectId}
                onRefresh={refetch}
              />
            )}
            {view === 'stats' && (
              <div className="text-center py-12 text-muted-foreground">
                <BarChart3 className="w-12 h-12 mx-auto mb-4 opacity-50" />
                <p>Analytics coming soon</p>
              </div>
            )}
          </>
        )}
      </div>

      {/* Create Campaign Modal */}
      {showCreate && (
        <CampaignCreate
          projectId={projectId}
          templates={templates}
          onClose={() => setShowCreate(false)}
          onCreated={() => {
            setShowCreate(false)
            refetch()
          }}
        />
      )}
    </div>
  )
}

function StatCard({
  label,
  value,
  color = 'default',
}: {
  label: string
  value: number
  color?: 'default' | 'amber' | 'blue' | 'emerald'
}) {
  const colorClasses = {
    default: 'bg-card text-foreground border border-border',
    amber: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
    blue: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    emerald: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300',
  }

  return (
    <Card className={colorClasses[color]}>
      <CardContent className="p-4">
        <div className="text-2xl font-bold">{value}</div>
        <div className="text-sm text-muted-foreground">{label}</div>
      </CardContent>
    </Card>
  )
}

function NavButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: React.ReactNode
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-lg transition-colors',
        active
          ? 'bg-secondary text-foreground'
          : 'text-muted-foreground hover:bg-secondary/50'
      )}
    >
      {icon}
      {label}
    </button>
  )
}
