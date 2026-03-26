import { useState } from 'react'
import { X, ChevronRight, ChevronLeft, Check } from 'lucide-react'
import { createSocialCampaign } from '../../lib/tauri'
import type { ContentTemplate, SourceConfig, Platform } from '../../lib/types'

interface Props {
  projectId: string
  templates: ContentTemplate[]
  onClose: () => void
  onCreated: () => void
}

type Step = 'basics' | 'sources' | 'platforms' | 'templates' | 'review'

const PLATFORMS: { id: Platform; name: string; icon: string }[] = [
  { id: 'tiktok', name: 'TikTok', icon: '🎵' },
  { id: 'instagram_feed', name: 'Instagram Feed', icon: '📸' },
  { id: 'instagram_reel', name: 'Instagram Reels', icon: '🎬' },
  { id: 'instagram_story', name: 'Instagram Stories', icon: '⏱️' },
]

export function CampaignCreate({ projectId, templates, onClose, onCreated }: Props) {
  const [step, setStep] = useState<Step>('basics')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Form state
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [sourceConfig, setSourceConfig] = useState<SourceConfig>({
    include_articles: true,
    article_slugs: [],
    include_screenshots: true,
    screenshot_dirs: [],
    include_specs: false,
  })
  const [selectedPlatforms, setSelectedPlatforms] = useState<Platform[]>(['instagram_feed'])
  const [selectedTemplates, setSelectedTemplates] = useState<string[]>([])

  const steps: { key: Step; title: string }[] = [
    { key: 'basics', title: 'Basics' },
    { key: 'sources', title: 'Content Sources' },
    { key: 'platforms', title: 'Platforms' },
    { key: 'templates', title: 'Templates' },
    { key: 'review', title: 'Review' },
  ]

  const currentStepIndex = steps.findIndex(s => s.key === step)

  function canProceed(): boolean {
    switch (step) {
      case 'basics':
        return name.trim().length > 0
      case 'sources':
        return sourceConfig.include_articles || sourceConfig.include_screenshots || sourceConfig.include_specs
      case 'platforms':
        return selectedPlatforms.length > 0
      case 'templates':
        // Allow proceeding if templates are selected OR if no templates exist (uses backend defaults)
        return templates.length === 0 || selectedTemplates.length > 0
      default:
        return true
    }
  }

  async function handleCreate() {
    setLoading(true)
    setError(null)
    try {
      await createSocialCampaign({
        project_id: projectId,
        name: name.trim(),
        description: description.trim() || undefined,
        source_config: sourceConfig,
        target_platforms: selectedPlatforms,
        template_ids: selectedTemplates,
      })
      onCreated()
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50">
      <div className="bg-white dark:bg-zinc-900 rounded-xl shadow-xl w-full max-w-2xl max-h-[90vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-200 dark:border-zinc-800">
          <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
            Create Campaign
          </h2>
          <button
            onClick={onClose}
            className="p-2 text-zinc-400 hover:text-zinc-600 hover:bg-zinc-100 rounded-lg transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Progress */}
        <div className="flex items-center px-6 py-3 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50/50 dark:bg-zinc-900/50">
          {steps.map((s, i) => (
            <div key={s.key} className="flex items-center">
              <div
                className={`flex items-center justify-center w-7 h-7 rounded-full text-xs font-medium ${
                  i <= currentStepIndex
                    ? 'bg-zinc-900 text-white'
                    : 'bg-zinc-200 text-zinc-600'
                }`}
              >
                {i < currentStepIndex ? (
                  <Check className="w-4 h-4" />
                ) : (
                  i + 1
                )}
              </div>
              <span
                className={`ml-2 text-sm ${
                  i <= currentStepIndex
                    ? 'text-zinc-900 font-medium'
                    : 'text-zinc-500'
                }`}
              >
                {s.title}
              </span>
              {i < steps.length - 1 && (
                <ChevronRight className="w-4 h-4 mx-2 text-zinc-400" />
              )}
            </div>
          ))}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-auto p-6">
          {error && (
            <div className="mb-4 p-4 bg-red-50 text-red-700 rounded-lg text-sm">
              {error}
            </div>
          )}

          {step === 'basics' && (
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                  Campaign Name *
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g., March Content Push"
                  className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                  Description
                </label>
                <textarea
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="What is this campaign about?"
                  rows={3}
                  className="w-full px-3 py-2 border border-zinc-300 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:bg-zinc-800 dark:text-zinc-100"
                />
              </div>
            </div>
          )}

          {step === 'sources' && (
            <div className="space-y-4">
              <p className="text-sm text-zinc-500 mb-4">
                Select which content sources to use for generating social media posts.
              </p>
              
              <label className="flex items-center gap-3 p-4 border border-zinc-200 dark:border-zinc-800 rounded-lg hover:bg-zinc-50 dark:hover:bg-zinc-800/50 cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_articles}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_articles: e.target.checked }))}
                  className="w-5 h-5 rounded border-zinc-300"
                />
                <div>
                  <div className="font-medium text-zinc-900 dark:text-zinc-100">Articles</div>
                  <div className="text-sm text-zinc-500">Generate posts from your blog articles</div>
                </div>
              </label>

              <label className="flex items-center gap-3 p-4 border border-zinc-200 dark:border-zinc-800 rounded-lg hover:bg-zinc-50 dark:hover:bg-zinc-800/50 cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_screenshots}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_screenshots: e.target.checked }))}
                  className="w-5 h-5 rounded border-zinc-300"
                />
                <div>
                  <div className="font-medium text-zinc-900 dark:text-zinc-100">Screenshots</div>
                  <div className="text-sm text-zinc-500">Use screenshots from your project</div>
                </div>
              </label>

              <label className="flex items-center gap-3 p-4 border border-zinc-200 dark:border-zinc-800 rounded-lg hover:bg-zinc-50 dark:hover:bg-zinc-800/50 cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_specs}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_specs: e.target.checked }))}
                  className="w-5 h-5 rounded border-zinc-300"
                />
                <div>
                  <div className="font-medium text-zinc-900 dark:text-zinc-100">Specs</div>
                  <div className="text-sm text-zinc-500">Include landing page specs</div>
                </div>
              </label>
            </div>
          )}

          {step === 'platforms' && (
            <div className="space-y-4">
              <p className="text-sm text-zinc-500 mb-4">
                Select which platforms to generate content for.
              </p>
              
              <div className="grid grid-cols-2 gap-3">
                {PLATFORMS.map(platform => (
                  <label
                    key={platform.id}
                    className={`flex items-center gap-3 p-4 border rounded-lg cursor-pointer transition-colors ${
                      selectedPlatforms.includes(platform.id)
                        ? 'border-zinc-900 bg-zinc-50 dark:border-zinc-100 dark:bg-zinc-800'
                        : 'border-zinc-200 dark:border-zinc-800 hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
                    }`}
                  >
                    <input
                      type="checkbox"
                      checked={selectedPlatforms.includes(platform.id)}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setSelectedPlatforms([...selectedPlatforms, platform.id])
                        } else {
                          setSelectedPlatforms(selectedPlatforms.filter(p => p !== platform.id))
                        }
                      }}
                      className="w-5 h-5 rounded border-zinc-300"
                    />
                    <span className="text-2xl">{platform.icon}</span>
                    <span className="font-medium text-zinc-900 dark:text-zinc-100">
                      {platform.name}
                    </span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {step === 'templates' && (
            <div className="space-y-4">
              <p className="text-sm text-zinc-500 mb-4">
                Select templates to use for generating posts. Each template creates different styles of content.
              </p>
              
              <div className="space-y-3">
                {templates.length === 0 ? (
                  <div className="text-center py-8 text-zinc-500">
                    No templates available. Using default templates.
                  </div>
                ) : (
                  templates.map(template => (
                    <label
                      key={template.id}
                      className={`flex items-start gap-3 p-4 border rounded-lg cursor-pointer transition-colors ${
                        selectedTemplates.includes(template.id)
                          ? 'border-zinc-900 bg-zinc-50 dark:border-zinc-100 dark:bg-zinc-800'
                          : 'border-zinc-200 dark:border-zinc-800 hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={selectedTemplates.includes(template.id)}
                        onChange={(e) => {
                          if (e.target.checked) {
                            setSelectedTemplates([...selectedTemplates, template.id])
                          } else {
                            setSelectedTemplates(selectedTemplates.filter(t => t !== template.id))
                          }
                        }}
                        className="w-5 h-5 rounded border-zinc-300 mt-0.5"
                      />
                      <div className="flex-1">
                        <div className="font-medium text-zinc-900 dark:text-zinc-100">
                          {template.name}
                        </div>
                        <div className="text-sm text-zinc-500 mt-1">
                          {template.description}
                        </div>
                        <div className="flex items-center gap-2 mt-2">
                          <PlatformBadge platform={template.platform} />
                          <FormatBadge format={template.format} />
                        </div>
                      </div>
                    </label>
                  ))
                )}
              </div>
            </div>
          )}

          {step === 'review' && (
            <div className="space-y-6">
              <div className="bg-zinc-50 dark:bg-zinc-800/50 rounded-lg p-4 space-y-4">
                <div>
                  <div className="text-sm text-zinc-500">Campaign Name</div>
                  <div className="font-medium text-zinc-900 dark:text-zinc-100">{name}</div>
                </div>
                
                {description && (
                  <div>
                    <div className="text-sm text-zinc-500">Description</div>
                    <div className="text-zinc-700 dark:text-zinc-300">{description}</div>
                  </div>
                )}
                
                <div>
                  <div className="text-sm text-zinc-500">Content Sources</div>
                  <div className="flex flex-wrap gap-2 mt-1">
                    {sourceConfig.include_articles && <SourceBadge label="Articles" />}
                    {sourceConfig.include_screenshots && <SourceBadge label="Screenshots" />}
                    {sourceConfig.include_specs && <SourceBadge label="Specs" />}
                  </div>
                </div>
                
                <div>
                  <div className="text-sm text-zinc-500">Platforms</div>
                  <div className="flex flex-wrap gap-2 mt-1">
                    {selectedPlatforms.map(p => <PlatformBadge key={p} platform={p} />)}
                  </div>
                </div>
                
                <div>
                  <div className="text-sm text-zinc-500">Templates</div>
                  <div className="flex flex-wrap gap-2 mt-1">
                    {selectedTemplates.map(t => {
                      const template = templates.find(tm => tm.id === t)
                      return (
                        <SourceBadge
                          key={t}
                          label={template?.name || t}
                        />
                      )
                    })}
                  </div>
                </div>
              </div>

              <p className="text-sm text-zinc-500">
                Click Create to start generating content. This will create a task that runs through the workflow.
              </p>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-zinc-200 dark:border-zinc-800">
          <button
            onClick={step === 'basics' ? onClose : () => setStep(steps[currentStepIndex - 1].key)}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-100 rounded-lg transition-colors"
          >
            <ChevronLeft className="w-4 h-4" />
            {step === 'basics' ? 'Cancel' : 'Back'}
          </button>
          
          {step === 'review' ? (
            <button
              onClick={handleCreate}
              disabled={loading}
              className="inline-flex items-center gap-2 px-6 py-2 bg-zinc-900 text-white text-sm font-medium rounded-lg hover:bg-zinc-800 disabled:opacity-50 transition-colors"
            >
              {loading ? 'Creating...' : 'Create Campaign'}
            </button>
          ) : (
            <button
              onClick={() => setStep(steps[currentStepIndex + 1].key)}
              disabled={!canProceed()}
              className="inline-flex items-center gap-2 px-4 py-2 bg-zinc-900 text-white text-sm font-medium rounded-lg hover:bg-zinc-800 disabled:opacity-50 transition-colors"
            >
              Next
              <ChevronRight className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

function SourceBadge({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center px-2 py-1 bg-zinc-200 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 text-xs rounded-md">
      {label}
    </span>
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
    single_image: 'Single Image',
    carousel: 'Carousel',
    video_hook: 'Video',
  }

  return (
    <span className="inline-flex items-center px-2 py-1 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs rounded-md">
      {labels[format] || format}
    </span>
  )
}
