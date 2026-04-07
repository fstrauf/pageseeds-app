import { CheckCircle, Clock, Trash2, ExternalLink, Calendar } from 'lucide-react'
import type { SocialPost } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface Props {
  post: SocialPost
  onClick: () => void
  onApprove: () => void
  onSchedule: () => void
  onMarkPosted: () => void
  onDelete: () => void
}

const STATUS_CONFIG: Record<string, { label: string; color: string; icon: React.ReactNode }> = {
  draft: {
    label: 'Draft',
    color: 'bg-muted text-muted-foreground',
    icon: <span className="w-2 h-2 bg-muted-foreground rounded-full" />,
  },
  review: {
    label: 'Review',
    color: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
    icon: <Clock className="w-3 h-3" />,
  },
  approved: {
    label: 'Approved',
    color: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300',
    icon: <CheckCircle className="w-3 h-3" />,
  },
  scheduled: {
    label: 'Scheduled',
    color: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    icon: <Calendar className="w-3 h-3" />,
  },
  posted: {
    label: 'Posted',
    color: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300',
    icon: <ExternalLink className="w-3 h-3" />,
  },
  failed: {
    label: 'Failed',
    color: 'bg-destructive/10 text-destructive',
    icon: <span className="w-2 h-2 bg-destructive rounded-full" />,
  },
}

export function PostCard({ post, onClick, onApprove, onSchedule, onMarkPosted, onDelete }: Props) {
  const status = STATUS_CONFIG[post.status] || STATUS_CONFIG.draft
  const platformIcon = getPlatformIcon(post.platform)
  const formatLabel = getFormatLabel(post.format)

  return (
    <div className="group bg-card border border-border rounded-xl overflow-hidden hover:shadow-md transition-shadow">
      {/* Image Preview */}
      <div
        onClick={onClick}
        className="aspect-square bg-muted relative cursor-pointer overflow-hidden"
      >
        {post.visual_assets.length > 0 ? (
          <img
            src={post.visual_assets[0].path}
            alt={post.visual_assets[0].description}
            className="w-full h-full object-cover"
            onError={(e) => {
              (e.target as HTMLImageElement).style.display = 'none'
            }}
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-muted-foreground">
            <span className="text-4xl">🖼️</span>
          </div>
        )}
        
        {/* Platform Badge */}
        <div className="absolute top-2 left-2 w-8 h-8 bg-card/90 backdrop-blur rounded-full flex items-center justify-center text-lg shadow-sm">
          {platformIcon}
        </div>
        
        {/* Status Badge */}
        <div className={cn('absolute top-2 right-2 flex items-center gap-1 px-2 py-1 rounded-full text-xs font-medium', status.color)}>
          {status.icon}
          <span>{status.label}</span>
        </div>
      </div>

      {/* Content */}
      <div className="p-4">
        <div className="flex items-center gap-2 mb-2">
          <span className="text-xs text-muted-foreground">{formatLabel}</span>
          <span className="text-border">•</span>
          <span className="text-xs text-muted-foreground capitalize">{post.source_type}</span>
        </div>
        
        <p
          onClick={onClick}
          className="font-medium text-foreground line-clamp-2 cursor-pointer hover:text-foreground/70 transition-colors"
        >
          {post.hook}
        </p>
        
        <p className="text-sm text-muted-foreground mt-2 line-clamp-2">
          {post.caption}
        </p>

        {/* Hashtags */}
        {post.hashtags.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-3">
            {post.hashtags.slice(0, 3).map(tag => (
              <span key={tag} className="text-xs text-primary">
                {tag}
              </span>
            ))}
            {post.hashtags.length > 3 && (
              <span className="text-xs text-muted-foreground">+{post.hashtags.length - 3}</span>
            )}
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-2 mt-4 pt-3 border-t border-border">
          {post.status === 'draft' && (
            <Button
              onClick={(e) => {
                e.stopPropagation()
                onApprove()
              }}
              className="flex-1"
              size="sm"
            >
              Approve
            </Button>
          )}
          
          {post.status === 'approved' && (
            <Button
              onClick={(e) => {
                e.stopPropagation()
                onSchedule()
              }}
              className="flex-1"
              size="sm"
              variant="secondary"
            >
              Schedule
            </Button>
          )}
          
          {post.status === 'scheduled' && (
            <Button
              onClick={(e) => {
                e.stopPropagation()
                onMarkPosted()
              }}
              className="flex-1"
              size="sm"
              variant="secondary"
            >
              Mark Posted
            </Button>
          )}
          
          {post.status === 'posted' && post.platform_post_url && (
            <Button
              asChild
              className="flex-1"
              size="sm"
              variant="secondary"
            >
              <a
                href={post.platform_post_url}
                target="_blank"
                rel="noopener noreferrer"
                onClick={(e) => e.stopPropagation()}
              >
                View Live
              </a>
            </Button>
          )}
          
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={(e) => {
              e.stopPropagation()
              onDelete()
            }}
            className="text-muted-foreground hover:text-destructive"
            title="Delete"
          >
            <Trash2 className="w-4 h-4" />
          </Button>
        </div>
      </div>
    </div>
  )
}

function getPlatformIcon(platform: string): string {
  const icons: Record<string, string> = {
    tiktok: '🎵',
    instagram_feed: '📸',
    instagram_reel: '🎬',
    instagram_story: '⏱️',
  }
  return icons[platform] || '📱'
}

function getFormatLabel(format: string): string {
  const labels: Record<string, string> = {
    single_image: 'Single Image',
    carousel: 'Carousel',
    video_hook: 'Video Hook',
  }
  return labels[format] || format
}
