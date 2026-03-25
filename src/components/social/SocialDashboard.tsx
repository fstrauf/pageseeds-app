import { useEffect, useState } from 'react'
import { Plus, LayoutGrid, BarChart3, Settings } from 'lucide-react'
import { listSocialCampaigns, getSocialCampaignStats, listSocialTemplates } from '../../lib/tauri'
import type { SocialCampaign, CampaignStats, ContentTemplate } from '../../lib/types'
import { CampaignList } from './CampaignList'
import { CampaignCreate } from './CampaignCreate'
import { TemplateList } from './TemplateList'

interface Props {
  projectId: string
}

type View = 'campaigns' | 'templates' | 'stats'

export function SocialDashboard({ projectId }: Props) {
  const [view, setView] = useState<View>('campaigns')
  const [campaigns, setCampaigns] = useState<SocialCampaign[]>([])
  const [templates, setTemplates] = useState<ContentTemplate[]>([])
  const [stats, setStats] = useState<Record<string, CampaignStats>>({})
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [refreshKey, setRefreshKey] = useState(0)

  useEffect(() => {
    loadData()
  }, [projectId, refreshKey])

  async function loadData() {
    if (!projectId) return
    setLoading(true)
    setError(null)
    try {
      const [campaignsData, templatesData] = await Promise.all([
        listSocialCampaigns(projectId),
        listSocialTemplates(projectId),
      ])
      setCampaigns(campaignsData)
      setTemplates(templatesData)

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
      setStats(statsMap)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const totalPosts = Object.values(stats).reduce((sum, s) => sum + s.total_posts, 0)
  const totalDrafts = Object.values(stats).reduce(
    (sum, s) => sum + (s.by_status.draft || 0),
    0
  )
  const totalScheduled = Object.values(stats).reduce(
    (sum, s) => sum + (s.by_status.scheduled || 0),
    0
  )
  const totalPosted = Object.values(stats).reduce(
    (sum, s) => sum + (s.by_status.posted || 0),
    0
  )

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-200 dark:border-zinc-800">
        <div>
          <h1 className="text-xl font-semibold text-zinc-900 dark:text-zinc-100">
            Social Media Marketing
          </h1>
          <p className="text-sm text-zinc-500 mt-1">
            Generate and manage social content for your project
          </p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="inline-flex items-center gap-2 px-4 py-2 bg-zinc-900 text-white text-sm font-medium rounded-lg hover:bg-zinc-800 transition-colors"
        >
          <Plus className="w-4 h-4" />
          New Campaign
        </button>
      </div>

      {/* Stats Bar */}
      <div className="grid grid-cols-4 gap-4 px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-900/50">
        <StatCard label="Total Posts" value={totalPosts} />
        <StatCard label="In Draft" value={totalDrafts} color="amber" />
        <StatCard label="Scheduled" value={totalScheduled} color="blue" />
        <StatCard label="Posted" value={totalPosted} color="emerald" />
      </div>

      {/* Navigation */}
      <div className="flex items-center gap-1 px-6 py-2 border-b border-zinc-200 dark:border-zinc-800">
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
        {error && (
          <div className="mb-4 p-4 bg-red-50 text-red-700 rounded-lg text-sm">
            {error}
          </div>
        )}

        {loading && campaigns.length === 0 ? (
          <div className="flex items-center justify-center h-64 text-zinc-500">
            Loading...
          </div>
        ) : (
          <>
            {view === 'campaigns' && (
              <CampaignList
                campaigns={campaigns}
                stats={stats}
                onRefresh={loadData}
                projectId={projectId}
              />
            )}
            {view === 'templates' && (
              <TemplateList
                templates={templates}
                projectId={projectId}
                onRefresh={loadData}
              />
            )}
            {view === 'stats' && (
              <div className="text-center py-12 text-zinc-500">
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
            setRefreshKey(k => k + 1)
          }}
        />
      )}
    </div>
  )
}

function StatCard({
  label,
  value,
  color = 'zinc',
}: {
  label: string
  value: number
  color?: 'zinc' | 'amber' | 'blue' | 'emerald'
}) {
  const colorClasses = {
    zinc: 'bg-zinc-100 text-zinc-900',
    amber: 'bg-amber-50 text-amber-700',
    blue: 'bg-blue-50 text-blue-700',
    emerald: 'bg-emerald-50 text-emerald-700',
  }

  return (
    <div className={`p-4 rounded-lg ${colorClasses[color]}`}>
      <div className="text-2xl font-bold">{value}</div>
      <div className="text-sm opacity-70">{label}</div>
    </div>
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
      className={`inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-lg transition-colors ${
        active
          ? 'bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-100'
          : 'text-zinc-600 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:bg-zinc-800/50'
      }`}
    >
      {icon}
      {label}
    </button>
  )
}
