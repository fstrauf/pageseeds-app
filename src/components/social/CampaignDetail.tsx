import { useEffect, useState } from 'react'
import { ArrowLeft, RefreshCw } from 'lucide-react'
import { getCampaignPosts, updateSocialPostStatus, scheduleSocialPost, markSocialPostPosted, deleteSocialPost } from '../../lib/tauri'
import type { SocialCampaign, SocialPost, PostStatus } from '../../lib/types'
import { PostCard } from './PostCard'
import { PostEditor } from './PostEditor'

interface Props {
  campaign: SocialCampaign
  projectId: string
  onBack: () => void
}

const TABS: { key: PostStatus | 'all'; label: string }[] = [
  { key: 'all', label: 'All Posts' },
  { key: 'draft', label: 'Draft' },
  { key: 'approved', label: 'Approved' },
  { key: 'scheduled', label: 'Scheduled' },
  { key: 'posted', label: 'Posted' },
]

export function CampaignDetail({ campaign, onBack }: Props) {
  const [posts, setPosts] = useState<SocialPost[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<PostStatus | 'all'>('all')
  const [selectedPost, setSelectedPost] = useState<SocialPost | null>(null)
  const [refreshKey, setRefreshKey] = useState(0)

  useEffect(() => {
    loadPosts()
  }, [campaign.id, activeTab, refreshKey])

  async function loadPosts() {
    setLoading(true)
    setError(null)
    try {
      const status = activeTab === 'all' ? undefined : activeTab
      const data = await getCampaignPosts(campaign.id, status)
      setPosts(data)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function handleApprove(post: SocialPost) {
    try {
      await updateSocialPostStatus(post.id, 'approved')
      setRefreshKey(k => k + 1)
    } catch (e) {
      alert(String(e))
    }
  }

  async function handleSchedule(post: SocialPost) {
    const date = prompt('Enter schedule date (YYYY-MM-DD HH:MM):')
    if (!date) return
    try {
      await scheduleSocialPost(post.id, new Date(date).toISOString())
      setRefreshKey(k => k + 1)
    } catch (e) {
      alert(String(e))
    }
  }

  async function handleMarkPosted(post: SocialPost) {
    const url = prompt('Enter the URL of the posted content:')
    if (!url) return
    try {
      await markSocialPostPosted(post.id, url)
      setRefreshKey(k => k + 1)
    } catch (e) {
      alert(String(e))
    }
  }

  async function handleDelete(post: SocialPost) {
    if (!confirm('Are you sure you want to delete this post?')) return
    try {
      await deleteSocialPost(post.id)
      setRefreshKey(k => k + 1)
    } catch (e) {
      alert(String(e))
    }
  }

  if (selectedPost) {
    return (
      <PostEditor
        post={selectedPost}
        onBack={() => setSelectedPost(null)}
        onUpdated={() => setRefreshKey(k => k + 1)}
      />
    )
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
        <div className="flex-1">
          <h2 className="text-xl font-semibold text-zinc-900 dark:text-zinc-100">
            {campaign.name}
          </h2>
          {campaign.description && (
            <p className="text-sm text-zinc-500 mt-1">{campaign.description}</p>
          )}
        </div>
        <button
          onClick={loadPosts}
          className="p-2 text-zinc-600 hover:bg-zinc-100 rounded-lg transition-colors"
          title="Refresh"
        >
          <RefreshCw className="w-5 h-5" />
        </button>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-zinc-200 dark:border-zinc-800">
        {TABS.map(tab => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              activeTab === tab.key
                ? 'border-zinc-900 text-zinc-900 dark:border-zinc-100 dark:text-zinc-100'
                : 'border-transparent text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Error */}
      {error && (
        <div className="p-4 bg-red-50 text-red-700 rounded-lg text-sm">
          {error}
        </div>
      )}

      {/* Posts Grid */}
      {loading ? (
        <div className="flex items-center justify-center h-64 text-zinc-500">
          Loading posts...
        </div>
      ) : posts.length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 bg-zinc-100 rounded-full flex items-center justify-center mx-auto mb-4">
            <span className="text-2xl">📝</span>
          </div>
          <h3 className="text-lg font-medium text-zinc-900 mb-2">No posts yet</h3>
          <p className="text-zinc-500 max-w-sm mx-auto">
            This campaign doesn't have any posts in this status. Run the campaign workflow to generate content.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {posts.map(post => (
            <PostCard
              key={post.id}
              post={post}
              onClick={() => setSelectedPost(post)}
              onApprove={() => handleApprove(post)}
              onSchedule={() => handleSchedule(post)}
              onMarkPosted={() => handleMarkPosted(post)}
              onDelete={() => handleDelete(post)}
            />
          ))}
        </div>
      )}
    </div>
  )
}
