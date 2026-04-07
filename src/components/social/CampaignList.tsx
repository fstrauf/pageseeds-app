import { useState } from 'react'
import { ChevronRight, MoreHorizontal, Trash2, Eye } from 'lucide-react'
import { deleteSocialCampaign } from '../../lib/tauri'
import type { SocialCampaign, CampaignStats } from '../../lib/types'
import { CampaignDetail } from './CampaignDetail'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface Props {
  campaigns: SocialCampaign[]
  stats: Record<string, CampaignStats>
  onRefresh: () => void
  projectId: string
}

const STATUS_COLORS: Record<string, string> = {
  draft: 'text-muted-foreground bg-muted',
  generating: 'text-blue-700 bg-blue-100 dark:text-blue-300 dark:bg-blue-900/30',
  active: 'text-emerald-700 bg-emerald-100 dark:text-emerald-300 dark:bg-emerald-900/30',
  completed: 'text-muted-foreground bg-muted',
}

export function CampaignList({ campaigns, stats, onRefresh, projectId }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [menuOpen, setMenuOpen] = useState<string | null>(null)

  if (campaigns.length === 0) {
    return (
      <div className="text-center py-12">
        <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center mx-auto mb-4">
          <span className="text-2xl">📱</span>
        </div>
        <h3 className="text-lg font-medium text-foreground mb-2">No campaigns yet</h3>
        <p className="text-muted-foreground max-w-sm mx-auto">
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
      className="group relative bg-card border border-border rounded-xl p-5 hover:border-border/80 hover:shadow-sm transition-all cursor-pointer"
    >
      <div className="flex items-start justify-between">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-3 mb-2">
            <h3 className="font-medium text-foreground truncate">
              {campaign.name}
            </h3>
            <span className={cn('px-2 py-0.5 text-xs font-medium rounded-full', statusClass)}>
              {campaign.status}
            </span>
          </div>
          
          {campaign.description && (
            <p className="text-sm text-muted-foreground mb-3 line-clamp-2">
              {campaign.description}
            </p>
          )}

          <div className="flex items-center gap-4 text-sm">
            <span className="text-muted-foreground">
              {totalPosts} posts
            </span>
            {draftCount > 0 && (
              <span className="text-amber-600 dark:text-amber-400">{draftCount} draft</span>
            )}
            {approvedCount > 0 && (
              <span className="text-emerald-600 dark:text-emerald-400">{approvedCount} approved</span>
            )}
            {scheduledCount > 0 && (
              <span className="text-blue-600 dark:text-blue-400">{scheduledCount} scheduled</span>
            )}
            {postedCount > 0 && (
              <span className="text-muted-foreground">{postedCount} posted</span>
            )}
          </div>

          <div className="flex items-center gap-2 mt-3">
            {campaign.target_platforms.map(platform => (
              <PlatformBadge key={platform} platform={platform} />
            ))}
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            onClick={(e) => {
              e.stopPropagation()
              onClick()
            }}
            title="View campaign"
          >
            <Eye className="w-4 h-4" />
          </Button>
          
          <div className="relative">
            <Button
              variant="ghost"
              size="icon"
              onClick={(e) => {
                e.stopPropagation()
                onMenuToggle()
              }}
            >
              <MoreHorizontal className="w-4 h-4" />
            </Button>
            
            {menuOpen && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={(e) => {
                    e.stopPropagation()
                    onMenuToggle()
                  }}
                />
                <div className="absolute right-0 top-full mt-1 w-40 bg-card border border-border rounded-lg shadow-lg z-20 py-1">
                  <button
                    onClick={(e) => {
                      e.stopPropagation()
                      onDelete()
                    }}
                    className="w-full flex items-center gap-2 px-4 py-2 text-sm text-destructive hover:bg-destructive/10 transition-colors"
                  >
                    <Trash2 className="w-4 h-4" />
                    Delete
                  </button>
                </div>
              </>
            )}
          </div>

          <ChevronRight className="w-5 h-5 text-muted-foreground" />
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
    <span className="inline-flex items-center gap-1 px-2 py-1 bg-muted text-muted-foreground text-xs rounded-md">
      <span>{icons[platform] || '📱'}</span>
      <span>{labels[platform] || platform}</span>
    </span>
  )
}
