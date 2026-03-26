import { Trash2, Sparkles } from 'lucide-react'
import { deleteSocialTemplate } from '../../lib/tauri'
import type { ContentTemplate } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface Props {
  templates: ContentTemplate[]
  projectId: string
  onRefresh: () => void
}

export function TemplateList({ templates, onRefresh }: Props) {
  if (templates.length === 0) {
    return (
      <div className="text-center py-12">
        <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center mx-auto mb-4">
          <Sparkles className="w-8 h-8 text-muted-foreground" />
        </div>
        <h3 className="text-lg font-medium text-foreground mb-2">No custom templates</h3>
        <p className="text-muted-foreground max-w-sm mx-auto">
          Using default templates. Create custom templates to generate content in your unique style.
        </p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <p className="text-muted-foreground">
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
    <div className="bg-card border border-border rounded-xl p-5 hover:border-border/80 transition-colors">
      <div className="flex items-start justify-between">
        <div className="flex-1">
          <h3 className="font-medium text-foreground">
            {template.name}
          </h3>
          
          {template.description && (
            <p className="text-sm text-muted-foreground mt-1 line-clamp-2">
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
                <span key={tag} className="text-xs text-primary">
                  {tag}
                </span>
              ))}
              {template.default_hashtags.length > 5 && (
                <span className="text-xs text-muted-foreground">
                  +{template.default_hashtags.length - 5}
                </span>
              )}
            </div>
          )}

          {template.example_output && (
            <div className="mt-4 p-3 bg-muted rounded-lg">
              <div className="text-xs text-muted-foreground mb-1">Example Hook:</div>
              <p className="text-sm text-foreground/80 line-clamp-2">
                {template.example_output.hook}
              </p>
            </div>
          )}
        </div>

        <Button
          variant="ghost"
          size="icon"
          onClick={onDelete}
          className="text-muted-foreground hover:text-destructive"
          title="Delete template"
        >
          <Trash2 className="w-4 h-4" />
        </Button>
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

function FormatBadge({ format }: { format: string }) {
  const labels: Record<string, string> = {
    single_image: 'Single',
    carousel: 'Carousel',
    video_hook: 'Video',
  }

  const colors: Record<string, string> = {
    single_image: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    carousel: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300',
    video_hook: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
  }

  return (
    <span className={cn('inline-flex items-center px-2 py-1 text-xs rounded-md', colors[format] || 'bg-muted text-muted-foreground')}>
      {labels[format] || format}
    </span>
  )
}
