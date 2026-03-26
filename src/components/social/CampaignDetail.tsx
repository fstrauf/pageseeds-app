import { useEffect, useState } from 'react'
import { ArrowLeft, RefreshCw, Play, Loader2 } from 'lucide-react'
import { getCampaignPosts, updateSocialPostStatus, scheduleSocialPost, markSocialPostPosted, deleteSocialPost, runSocialCampaign } from '../../lib/tauri'
import type { SocialCampaign, SocialPost, PostStatus } from '../../lib/types'
import { PostCard } from './PostCard'
import { PostEditor } from './PostEditor'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

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
  const [generating, setGenerating] = useState(false)
  const [generateMsg, setGenerateMsg] = useState<string | null>(null)

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

  async function handleGeneratePosts() {
    setGenerating(true)
    setGenerateMsg(null)
    try {
      const task = await runSocialCampaign(campaign.id)
      setGenerateMsg(`Task created: ${task.title}. Check the Tasks section to monitor progress.`)
    } catch (e) {
      setGenerateMsg(`Error: ${String(e)}`)
    } finally {
      setGenerating(false)
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
        <Button
          variant="ghost"
          size="icon"
          onClick={onBack}
        >
          <ArrowLeft className="w-5 h-5" />
        </Button>
        <div className="flex-1">
          <h2 className="text-xl font-semibold text-foreground">
            {campaign.name}
          </h2>
          {campaign.description && (
            <p className="text-sm text-muted-foreground mt-1">{campaign.description}</p>
          )}
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={loadPosts}
          title="Refresh"
        >
          <RefreshCw className="w-5 h-5" />
        </Button>
        <Button
          onClick={handleGeneratePosts}
          disabled={generating}
          className="gap-2"
        >
          {generating ? (
            <><Loader2 className="w-4 h-4 animate-spin" /> Generating...</>
          ) : (
            <><Play className="w-4 h-4" /> Generate Posts</>
          )}
        </Button>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-border">
        {TABS.map(tab => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={cn(
              'px-4 py-2 text-sm font-medium border-b-2 transition-colors',
              activeTab === tab.key
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            )}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Error */}
      {error && (
        <div className="p-4 bg-destructive/10 text-destructive rounded-lg text-sm">
          {error}
        </div>
      )}
      
      {/* Generate Message */}
      {generateMsg && (
        <div className={cn(
          'p-4 rounded-lg text-sm',
          generateMsg.startsWith('Error') 
            ? 'bg-destructive/10 text-destructive' 
            : 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300'
        )}>
          {generateMsg}
        </div>
      )}

      {/* Posts Grid */}
      {loading ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
          Loading posts...
        </div>
      ) : posts.length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 bg-muted rounded-full flex items-center justify-center mx-auto mb-4">
            <span className="text-2xl">📝</span>
          </div>
          <h3 className="text-lg font-medium text-foreground mb-2">No posts yet</h3>
          <p className="text-muted-foreground max-w-sm mx-auto mb-6">
            This campaign doesn't have any posts yet. Click "Generate Posts" to create social media content from your sources.
          </p>
          <Button 
            onClick={handleGeneratePosts} 
            disabled={generating}
            size="lg"
            className="gap-2"
          >
            {generating ? (
              <><Loader2 className="w-4 h-4 animate-spin" /> Generating...</>
            ) : (
              <><Play className="w-4 h-4" /> Generate Posts</>
            )}
          </Button>
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
