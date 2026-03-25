import { useState } from 'react'
import { ArrowLeft, Copy, Save } from 'lucide-react'
import { updateSocialPost } from '../../lib/tauri'
import type { SocialPost } from '../../lib/types'

interface Props {
  post: SocialPost
  onBack: () => void
  onUpdated: () => void
}

export function PostEditor({ post, onBack, onUpdated }: Props) {
  const [editedPost, setEditedPost] = useState<SocialPost>(post)
  const [saving, setSaving] = useState(false)
  const [copied, setCopied] = useState(false)

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

  const platformIcon = getPlatformIcon(post.platform)
  const statusColors: Record<string, string> = {
    draft: 'bg-zinc-100 text-zinc-700',
    review: 'bg-amber-100 text-amber-700',
    approved: 'bg-emerald-100 text-emerald-700',
    scheduled: 'bg-blue-100 text-blue-700',
    posted: 'bg-purple-100 text-purple-700',
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <button
          onClick={onBack}
          className="p-2 text-zinc-600 hover:bg-zinc-100 rounded-lg transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div className="flex-1 flex items-center gap-3">
          <span className="text-2xl">{platformIcon}</span>
          <div>
            <h2 className="text-xl font-semibold text-zinc-900 dark:text-zinc-100">
              Edit Post
            </h2>
            <div className="flex items-center gap-2 mt-1">
              <span className={`px-2 py-0.5 text-xs font-medium rounded-full ${statusColors[post.status] || 'bg-zinc-100'}`}>
                {post.status}
              </span>
              <span className="text-sm text-zinc-500 capitalize">
                {post.source_type} • {post.format}
              </span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleCopy}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-zinc-700 bg-zinc-100 hover:bg-zinc-200 rounded-lg transition-colors"
          >
            <Copy className="w-4 h-4" />
            {copied ? 'Copied!' : 'Copy Text'}
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-zinc-900 hover:bg-zinc-800 rounded-lg transition-colors disabled:opacity-50"
          >
            <Save className="w-4 h-4" />
            {saving ? 'Saving...' : 'Save Changes'}
          </button>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Left: Preview */}
        <div className="space-y-4">
          <h3 className="font-medium text-zinc-900 dark:text-zinc-100">Preview</h3>
          
          {/* Image Preview */}
          <div className="aspect-square bg-zinc-100 dark:bg-zinc-800 rounded-xl overflow-hidden">
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
          </div>

          {/* Platform-specific preview mockup */}
          <div className="border border-zinc-200 dark:border-zinc-800 rounded-xl p-4 bg-white dark:bg-zinc-900">
            <div className="flex items-center gap-2 mb-3">
              <div className="w-8 h-8 bg-zinc-200 rounded-full" />
              <div className="flex-1">
                <div className="h-3 bg-zinc-200 rounded w-24" />
              </div>
            </div>
            <p className="text-sm text-zinc-900 dark:text-zinc-100 whitespace-pre-wrap">
              {editedPost.hook}
            </p>
            <p className="text-sm text-zinc-600 dark:text-zinc-400 mt-2 whitespace-pre-wrap">
              {editedPost.caption}
            </p>
            <div className="flex flex-wrap gap-2 mt-2">
              {editedPost.hashtags.map(tag => (
                <span key={tag} className="text-sm text-blue-600">
                  {tag}
                </span>
              ))}
            </div>
          </div>
        </div>

        {/* Right: Editor */}
        <div className="space-y-4">
          <h3 className="font-medium text-zinc-900 dark:text-zinc-100">Edit Content</h3>
          
          {/* Hook */}
          <div>
            <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
              Hook (First Line)
            </label>
            <textarea
              value={editedPost.hook}
              onChange={(e) => setEditedPost(p => ({ ...p, hook: e.target.value }))}
              rows={2}
              className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
            />
            <p className="text-xs text-zinc-500 mt-1">
              {editedPost.hook.length} characters
            </p>
          </div>

          {/* Caption */}
          <div>
            <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
              Caption
            </label>
            <textarea
              value={editedPost.caption}
              onChange={(e) => setEditedPost(p => ({ ...p, caption: e.target.value }))}
              rows={6}
              className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
            />
            <p className="text-xs text-zinc-500 mt-1">
              {editedPost.caption.length} characters
            </p>
          </div>

          {/* Hashtags */}
          <div>
            <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
              Hashtags (comma-separated)
            </label>
            <input
              type="text"
              value={editedPost.hashtags.join(', ')}
              onChange={(e) => {
                const tags = e.target.value.split(',').map(t => t.trim()).filter(Boolean)
                setEditedPost(p => ({ ...p, hashtags: tags }))
              }}
              className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
            />
          </div>

          {/* CTA */}
          <div>
            <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
              Call to Action
            </label>
            <input
              type="text"
              value={editedPost.cta}
              onChange={(e) => setEditedPost(p => ({ ...p, cta: e.target.value }))}
              className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
            />
          </div>

          {/* Source Info */}
          <div className="p-4 bg-zinc-50 dark:bg-zinc-800/50 rounded-lg">
            <h4 className="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">
              Source Information
            </h4>
            <div className="text-sm text-zinc-600 dark:text-zinc-400 space-y-1">
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
