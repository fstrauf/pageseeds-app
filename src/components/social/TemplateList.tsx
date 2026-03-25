import { Trash2, Sparkles } from 'lucide-react'
import { deleteSocialTemplate } from '../../lib/tauri'
import type { ContentTemplate } from '../../lib/types'

interface Props {
  templates: ContentTemplate[]
  projectId: string
  onRefresh: () => void
}

export function TemplateList({ templates, onRefresh }: Props) {
  if (templates.length === 0) {
    return (
      <div className="text-center py-12">
        <div className="w-16 h-16 bg-zinc-100 rounded-full flex items-center justify-center mx-auto mb-4">
          <Sparkles className="w-8 h-8 text-zinc-400" />
        </div>
        <h3 className="text-lg font-medium text-zinc-900 mb-2">No custom templates</h3>
        <p className="text-zinc-500 max-w-sm mx-auto">
          Using default templates. Create custom templates to generate content in your unique style.
        </p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <p className="text-zinc-600 dark:text-zinc-400">
        {templates.length} template{templates.length !== 1 ? 's' : ''} available
      </p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {templates.map(template => (
          <TemplateCard
            key={template.id}
            template={template}
            onDelete={async () => {
              if (confirm('Are you sure you want to delete this template?')) {
                try {
                  await deleteSocialTemplate(template.id)
                  onRefresh()
                } catch (e) {
                  alert(String(e))
                }
              }
            }}
          />
        ))}
      </div>
    </div>
  )
}

function TemplateCard({
  template,
  onDelete,
}: {
  template: ContentTemplate
  onDelete: () => void
}) {
  return (
    <div className="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl p-5 hover:border-zinc-300 dark:hover:border-zinc-700 transition-colors">
      <div className="flex items-start justify-between">
        <div className="flex-1">
          <h3 className="font-medium text-zinc-900 dark:text-zinc-100">
            {template.name}
          </h3>
          
          {template.description && (
            <p className="text-sm text-zinc-500 mt-1 line-clamp-2">
              {template.description}
            </p>
          )}

          <div className="flex items-center gap-2 mt-3">
            <PlatformBadge platform={template.platform} />
            <FormatBadge format={template.format} />
          </div>

          {template.default_hashtags.length > 0 && (
            <div className="flex flex-wrap gap-1 mt-3">
              {template.default_hashtags.slice(0, 5).map(tag => (
                <span key={tag} className="text-xs text-blue-600">
                  {tag}
                </span>
              ))}
              {template.default_hashtags.length > 5 && (
                <span className="text-xs text-zinc-400">
                  +{template.default_hashtags.length - 5}
                </span>
              )}
            </div>
          )}

          {template.example_output && (
            <div className="mt-4 p-3 bg-zinc-50 dark:bg-zinc-800 rounded-lg">
              <div className="text-xs text-zinc-500 mb-1">Example Hook:</div>
              <p className="text-sm text-zinc-700 dark:text-zinc-300 line-clamp-2">
                {template.example_output.hook}
              </p>
            </div>
          )}
        </div>

        <button
          onClick={onDelete}
          className="p-2 text-zinc-400 hover:text-red-600 hover:bg-red-50 rounded-lg transition-colors"
          title="Delete template"
        >
          <Trash2 className="w-4 h-4" />
        </button>
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
    <span className="inline-flex items-center gap-1 px-2 py-1 bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 text-xs rounded-md">
      <span>{icons[platform] || '📱'}</span>
      <span>{labels[platform] || platform}</span>
    </span>
  )
}

function FormatBadge({ format }: { format: string }) {
  const labels: Record<string, string> = {
    single_image: 'Single',
    carousel: 'Carousel',
    video_hook: 'Video',
  }

  const colors: Record<string, string> = {
    single_image: 'bg-blue-50 text-blue-700',
    carousel: 'bg-purple-50 text-purple-700',
    video_hook: 'bg-amber-50 text-amber-700',
  }

  return (
    <span className={`inline-flex items-center px-2 py-1 text-xs rounded-md ${colors[format] || 'bg-zinc-100 text-zinc-700'}`}>
      {labels[format] || format}
    </span>
  )
}
