import { useState } from 'react'
import { ChevronRight, MoreHorizontal, Trash2, Eye } from 'lucide-react'
import { deleteSocialCampaign } from '../../lib/tauri'
import type { SocialCampaign, CampaignStats } from '../../lib/types'
import { CampaignDetail } from './CampaignDetail'

interface Props {
  campaigns: SocialCampaign[]
  stats: Record<string, CampaignStats>
  onRefresh: () => void
  projectId: string
}

const STATUS_COLORS: Record<string, string> = {
  draft: 'text-zinc-600 bg-zinc-100',
  generating: 'text-blue-600 bg-blue-100',
  active: 'text-emerald-600 bg-emerald-100',
  completed: 'text-zinc-600 bg-zinc-100',
}

export function CampaignList({ campaigns, stats, onRefresh, projectId }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [menuOpen, setMenuOpen] = useState<string | null>(null)

  if (campaigns.length === 0) {
    return (
      <div className="text-center py-12">
        <div className="w-16 h-16 bg-zinc-100 rounded-full flex items-center justify-center mx-auto mb-4">
          <span className="text-2xl">📱</span>
        </div>
        <h3 className="text-lg font-medium text-zinc-900 mb-2">No campaigns yet</h3>
        <p className="text-zinc-500 max-w-sm mx-auto">
          Create your first campaign to start generating social media content from your articles and screenshots.
        </p>
      </div>
    )
  }

  const selectedCampaign = campaigns.find(c => c.id === selectedId)

  if (selectedCampaign) {
    return (
      <CampaignDetail
        campaign={selectedCampaign}
        projectId={projectId}
        onBack={() => setSelectedId(null)}
      />
    )
  }

  return (
    <div className="space-y-4">
      {campaigns.map(campaign => {
        const campaignStats = stats[campaign.id]
        return (
          <CampaignCard
            key={campaign.id}
            campaign={campaign}
            stats={campaignStats}
            onClick={() => setSelectedId(campaign.id)}
            menuOpen={menuOpen === campaign.id}
            onMenuToggle={() => setMenuOpen(menuOpen === campaign.id ? null : campaign.id)}
            onDelete={async () => {
              if (confirm('Are you sure you want to delete this campaign?')) {
                try {
                  await deleteSocialCampaign(campaign.id)
                  onRefresh()
                } catch (e) {
                  alert(String(e))
                }
              }
            }}
          />
        )
      })}
    </div>
  )
}

function CampaignCard({
  campaign,
  stats,
  onClick,
  menuOpen,
  onMenuToggle,
  onDelete,
}: {
  campaign: SocialCampaign
  stats?: CampaignStats
  onClick: () => void
  menuOpen: boolean
  onMenuToggle: () => void
  onDelete: () => void
}) {
  const statusClass = STATUS_COLORS[campaign.status] || STATUS_COLORS.draft
  const totalPosts = stats?.total_posts || 0
  const draftCount = stats?.by_status.draft || 0
  const approvedCount = stats?.by_status.approved || 0
  const scheduledCount = stats?.by_status.scheduled || 0
  const postedCount = stats?.by_status.posted || 0

  return (
    <div
      onClick={onClick}
      className="group relative bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 hover:border-zinc-300 dark:hover:border-zinc-700 hover:shadow-sm transition-all cursor-pointer"
    >
      <div className="flex items-start justify-between">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-3 mb-2">
            <h3 className="font-medium text-zinc-900 dark:text-zinc-100 truncate">
              {campaign.name}
            </h3>
            <span className={`px-2 py-0.5 text-xs font-medium rounded-full ${statusClass}`}>
              {campaign.status}
            </span>
          </div>
          
          {campaign.description && (
            <p className="text-sm text-zinc-500 mb-3 line-clamp-2">
              {campaign.description}
            </p>
          )}

          <div className="flex items-center gap-4 text-sm">
            <span className="text-zinc-600 dark:text-zinc-400">
              {totalPosts} posts
            </span>
            {draftCount > 0 && (
              <span className="text-amber-600">{draftCount} draft</span>
            )}
            {approvedCount > 0 && (
              <span className="text-emerald-600">{approvedCount} approved</span>
            )}
            {scheduledCount > 0 && (
              <span className="text-blue-600">{scheduledCount} scheduled</span>
            )}
            {postedCount > 0 && (
              <span className="text-zinc-600">{postedCount} posted</span>
            )}
          </div>

          <div className="flex items-center gap-2 mt-3">
            {campaign.target_platforms.map(platform => (
              <PlatformBadge key={platform} platform={platform} />
            ))}
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={(e) => {
              e.stopPropagation()
              onClick()
            }}
            className="p-2 text-zinc-400 hover:text-zinc-600 hover:bg-zinc-100 rounded-lg transition-colors"
            title="View campaign"
          >
            <Eye className="w-4 h-4" />
          </button>
          
          <div className="relative">
            <button
              onClick={(e) => {
                e.stopPropagation()
                onMenuToggle()
              }}
              className="p-2 text-zinc-400 hover:text-zinc-600 hover:bg-zinc-100 rounded-lg transition-colors"
            >
              <MoreHorizontal className="w-4 h-4" />
            </button>
            
            {menuOpen && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={(e) => {
                    e.stopPropagation()
                    onMenuToggle()
                  }}
                />
                <div className="absolute right-0 top-full mt-1 w-40 bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-lg shadow-lg z-20 py-1">
                  <button
                    onClick={(e) => {
                      e.stopPropagation()
                      onDelete()
                    }}
                    className="w-full flex items-center gap-2 px-4 py-2 text-sm text-red-600 hover:bg-red-50 transition-colors"
                  >
                    <Trash2 className="w-4 h-4" />
                    Delete
                  </button>
                </div>
              </>
            )}
          </div>

          <ChevronRight className="w-5 h-5 text-zinc-400" />
        </div>
      </div>
    </div>
  )
}

function PlatformBadge({ platform }: { platform: string }) {
  const icons: Record<string, string> = {
    tiktok: '🎵',
    instagram_feed: '📸',
    instagram_reel: '🎬',
    instagram_story: '⏱️',
  }

  const labels: Record<string, string> = {
    tiktok: 'TikTok',
    instagram_feed: 'Feed',
    instagram_reel: 'Reels',
    instagram_story: 'Stories',
  }

  return (
    <span className="inline-flex items-center gap-1 px-2 py-1 bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 text-xs rounded-md">
      <span>{icons[platform] || '📱'}</span>
      <span>{labels[platform] || platform}</span>
    </span>
  )
}
