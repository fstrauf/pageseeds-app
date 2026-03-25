import { CheckCircle, Clock, Trash2, ExternalLink, Calendar } from 'lucide-react'
import type { SocialPost } from '../../lib/types'

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
    color: 'bg-zinc-100 text-zinc-700',
    icon: <span className="w-2 h-2 bg-zinc-400 rounded-full" />,
  },
  review: {
    label: 'Review',
    color: 'bg-amber-100 text-amber-700',
    icon: <Clock className="w-3 h-3" />,
  },
  approved: {
    label: 'Approved',
    color: 'bg-emerald-100 text-emerald-700',
    icon: <CheckCircle className="w-3 h-3" />,
  },
  scheduled: {
    label: 'Scheduled',
    color: 'bg-blue-100 text-blue-700',
    icon: <Calendar className="w-3 h-3" />,
  },
  posted: {
    label: 'Posted',
    color: 'bg-purple-100 text-purple-700',
    icon: <ExternalLink className="w-3 h-3" />,
  },
  failed: {
    label: 'Failed',
    color: 'bg-red-100 text-red-700',
    icon: <span className="w-2 h-2 bg-red-400 rounded-full" />,
  },
}

export function PostCard({ post, onClick, onApprove, onSchedule, onMarkPosted, onDelete }: Props) {
  const status = STATUS_CONFIG[post.status] || STATUS_CONFIG.draft
  const platformIcon = getPlatformIcon(post.platform)
  const formatLabel = getFormatLabel(post.format)

  return (
    <div className="group bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl overflow-hidden hover:shadow-md transition-shadow">
      {/* Image Preview */}
      <div
        onClick={onClick}
        className="aspect-square bg-zinc-100 dark:bg-zinc-800 relative cursor-pointer overflow-hidden"
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
          <div className="w-full h-full flex items-center justify-center text-zinc-400">
            <span className="text-4xl">🖼️</span>
          </div>
        )}
        
        {/* Platform Badge */}
        <div className="absolute top-2 left-2 w-8 h-8 bg-white/90 backdrop-blur rounded-full flex items-center justify-center text-lg shadow-sm">
          {platformIcon}
        </div>
        
        {/* Status Badge */}
        <div className={`absolute top-2 right-2 flex items-center gap-1 px-2 py-1 rounded-full text-xs font-medium ${status.color}`}>
          {status.icon}
          <span>{status.label}</span>
        </div>
      </div>

      {/* Content */}
      <div className="p-4">
        <div className="flex items-center gap-2 mb-2">
          <span className="text-xs text-zinc-500">{formatLabel}</span>
          <span className="text-zinc-300">•</span>
          <span className="text-xs text-zinc-500 capitalize">{post.source_type}</span>
        </div>
        
        <p
          onClick={onClick}
          className="font-medium text-zinc-900 dark:text-zinc-100 line-clamp-2 cursor-pointer hover:text-zinc-600 transition-colors"
        >
          {post.hook}
        </p>
        
        <p className="text-sm text-zinc-500 mt-2 line-clamp-2">
          {post.caption}
        </p>

        {/* Hashtags */}
        {post.hashtags.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-3">
            {post.hashtags.slice(0, 3).map(tag => (
              <span key={tag} className="text-xs text-blue-600">
                {tag}
              </span>
            ))}
            {post.hashtags.length > 3 && (
              <span className="text-xs text-zinc-400">+{post.hashtags.length - 3}</span>
            )}
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-2 mt-4 pt-3 border-t border-zinc-100 dark:border-zinc-800">
          {post.status === 'draft' && (
            <button
              onClick={(e) => {
                e.stopPropagation()
                onApprove()
              }}
              className="flex-1 px-3 py-1.5 bg-zinc-900 text-white text-sm font-medium rounded-lg hover:bg-zinc-800 transition-colors"
            >
              Approve
            </button>
          )}
          
          {post.status === 'approved' && (
            <button
              onClick={(e) => {
                e.stopPropagation()
                onSchedule()
              }}
              className="flex-1 px-3 py-1.5 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 transition-colors"
            >
              Schedule
            </button>
          )}
          
          {post.status === 'scheduled' && (
            <button
              onClick={(e) => {
                e.stopPropagation()
                onMarkPosted()
              }}
              className="flex-1 px-3 py-1.5 bg-emerald-600 text-white text-sm font-medium rounded-lg hover:bg-emerald-700 transition-colors"
            >
              Mark Posted
            </button>
          )}
          
          {post.status === 'posted' && post.platform_post_url && (
            <a
              href={post.platform_post_url}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(e) => e.stopPropagation()}
              className="flex-1 px-3 py-1.5 bg-zinc-100 text-zinc-700 text-sm font-medium rounded-lg hover:bg-zinc-200 transition-colors text-center"
            >
              View Live
            </a>
          )}
          
          <button
            onClick={(e) => {
              e.stopPropagation()
              onDelete()
            }}
            className="p-1.5 text-zinc-400 hover:text-red-600 hover:bg-red-50 rounded-lg transition-colors"
            title="Delete"
          >
            <Trash2 className="w-4 h-4" />
          </button>
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
