import { useState } from 'react'
import { ArrowLeft, Copy, Save, Image, Sparkles } from 'lucide-react'
import { updateSocialPost } from '../../lib/tauri'
import type { SocialPost } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

interface Props {
  post: SocialPost
  onBack: () => void
  onUpdated: () => void
}

export function PostEditor({ post, onBack, onUpdated }: Props) {
  const [editedPost, setEditedPost] = useState<SocialPost>(post)
  const [saving, setSaving] = useState(false)
  const [copied, setCopied] = useState(false)
  const [imagePromptCopied, setImagePromptCopied] = useState(false)

  async function handleSave() {
    setSaving(true)
    try {
      await updateSocialPost(editedPost)
      onUpdated()
      onBack()
    } catch (e) {
      alert(String(e))
    } finally {
      setSaving(false)
    }
  }

  function handleCopy() {
    const text = `${editedPost.hook}\n\n${editedPost.caption}\n\n${editedPost.hashtags.join(' ')}`
    navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  function handleCopyImagePrompt() {
    if (editedPost.image_generation_prompt) {
      navigator.clipboard.writeText(editedPost.image_generation_prompt)
      setImagePromptCopied(true)
      setTimeout(() => setImagePromptCopied(false), 2000)
    }
  }

  const platformIcon = getPlatformIcon(post.platform)
  const statusColors: Record<string, string> = {
    draft: 'bg-muted text-muted-foreground',
    review: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
    approved: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300',
    scheduled: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    posted: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300',
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button
          variant="ghost"
          size="icon"
          onClick={onBack}
        >
          <ArrowLeft className="w-5 h-5" />
        </Button>
        <div className="flex-1 flex items-center gap-3">
          <span className="text-2xl">{platformIcon}</span>
          <div>
            <h2 className="text-xl font-semibold text-foreground">
              Edit Post
            </h2>
            <div className="flex items-center gap-2 mt-1">
              <span className={cn('px-2 py-0.5 text-xs font-medium rounded-full', statusColors[post.status] || 'bg-muted')}>
                {post.status}
              </span>
              <span className="text-sm text-muted-foreground capitalize">
                {post.source_type} • {post.format}
              </span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            onClick={handleCopy}
          >
            <Copy className="w-4 h-4 mr-2" />
            {copied ? 'Copied!' : 'Copy Text'}
          </Button>
          <Button
            onClick={handleSave}
            disabled={saving}
          >
            <Save className="w-4 h-4 mr-2" />
            {saving ? 'Saving...' : 'Save Changes'}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Left: Preview */}
        <div className="space-y-4">
          <h3 className="font-medium text-foreground">Preview</h3>
          
          {/* Image Preview */}
          <div className="aspect-square bg-muted rounded-xl overflow-hidden">
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
          </div>

          {/* Platform-specific preview mockup */}
          <div className="border border-border rounded-xl p-4 bg-card">
            <div className="flex items-center gap-2 mb-3">
              <div className="w-8 h-8 bg-muted rounded-full" />
              <div className="flex-1">
                <div className="h-3 bg-muted rounded w-24" />
              </div>
            </div>
            <p className="text-sm text-foreground whitespace-pre-wrap">
              {editedPost.hook}
            </p>
            <p className="text-sm text-muted-foreground mt-2 whitespace-pre-wrap">
              {editedPost.caption}
            </p>
            <div className="flex flex-wrap gap-2 mt-2">
              {editedPost.hashtags.map(tag => (
                <span key={tag} className="text-sm text-primary">
                  {tag}
                </span>
              ))}
            </div>
          </div>

          {/* Image Generation Prompt */}
          {editedPost.image_generation_prompt && (
            <div className="border border-border rounded-xl p-4 bg-card">
              <div className="flex items-center gap-2 mb-3">
                <Sparkles className="w-4 h-4 text-primary" />
                <h4 className="text-sm font-medium text-foreground">
                  AI Image Generation Prompt
                </h4>
              </div>
              <p className="text-sm text-muted-foreground mb-3 leading-relaxed">
                {editedPost.image_generation_prompt}
              </p>
              <div className="flex items-center gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleCopyImagePrompt}
                  className="flex-1"
                >
                  <Copy className="w-4 h-4 mr-2" />
                  {imagePromptCopied ? 'Copied!' : 'Copy Prompt'}
                </Button>
              </div>
              <p className="text-xs text-muted-foreground mt-2">
                Use this prompt in Midjourney, DALL-E, Leonardo, or any AI image generator. 
                Then overlay the text on the generated image.
              </p>
            </div>
          )}

          {/* Visual Asset Info */}
          {editedPost.visual_assets.length > 0 && editedPost.visual_assets[0].description && (
            <div className="p-4 bg-muted rounded-lg">
              <div className="flex items-center gap-2 mb-2">
                <Image className="w-4 h-4 text-muted-foreground" />
                <h4 className="text-sm font-medium text-foreground">
                  Visual Description
                </h4>
              </div>
              <p className="text-sm text-muted-foreground">
                {editedPost.visual_assets[0].description}
              </p>
              {editedPost.visual_assets[0].overlay_text && (
                <div className="mt-2 pt-2 border-t border-border">
                  <p className="text-xs text-muted-foreground">Suggested overlay text:</p>
                  <p className="text-sm text-foreground font-medium">
                    "{editedPost.visual_assets[0].overlay_text}"
                  </p>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Right: Editor */}
        <div className="space-y-4">
          <h3 className="font-medium text-foreground">Edit Content</h3>
          
          {/* Hook */}
          <div>
            <Label htmlFor="hook">Hook (First Line)</Label>
            <Textarea
              id="hook"
              value={editedPost.hook}
              onChange={(e) => setEditedPost(p => ({ ...p, hook: e.target.value }))}
              rows={2}
            />
            <p className="text-xs text-muted-foreground mt-1">
              {editedPost.hook.length} characters
            </p>
          </div>

          {/* Caption */}
          <div>
            <Label htmlFor="caption">Caption</Label>
            <Textarea
              id="caption"
              value={editedPost.caption}
              onChange={(e) => setEditedPost(p => ({ ...p, caption: e.target.value }))}
              rows={6}
            />
            <p className="text-xs text-muted-foreground mt-1">
              {editedPost.caption.length} characters
            </p>
          </div>

          {/* Hashtags */}
          <div>
            <Label htmlFor="hashtags">Hashtags (comma-separated)</Label>
            <Input
              id="hashtags"
              type="text"
              value={editedPost.hashtags.join(', ')}
              onChange={(e) => {
                const tags = e.target.value.split(',').map(t => t.trim()).filter(Boolean)
                setEditedPost(p => ({ ...p, hashtags: tags }))
              }}
            />
          </div>

          {/* CTA */}
          <div>
            <Label htmlFor="cta">Call to Action</Label>
            <Input
              id="cta"
              type="text"
              value={editedPost.cta}
              onChange={(e) => setEditedPost(p => ({ ...p, cta: e.target.value }))}
            />
          </div>

          {/* Source Info */}
          <div className="p-4 bg-muted rounded-lg">
            <h4 className="text-sm font-medium text-foreground mb-2">
              Source Information
            </h4>
            <div className="text-sm text-muted-foreground space-y-1">
              <p>Type: <span className="capitalize">{post.source_type}</span></p>
              <p>ID: {post.source_id}</p>
              <p>Template: {post.template_id}</p>
              {post.generated_by && <p>Generated by: {post.generated_by}</p>}
            </div>
          </div>
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
