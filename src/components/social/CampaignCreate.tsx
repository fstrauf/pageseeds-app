import { useState } from 'react'
import { X, ChevronRight, ChevronLeft, Check } from 'lucide-react'
import { createSocialCampaign } from '../../lib/tauri'
import { useErrorHandler } from '../../lib/toast-context'
import type { ContentTemplate, SourceConfig, Platform } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

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
  const { showError } = useErrorHandler()

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
    try {
      await createSocialCampaign({
        project_id: projectId,
        name: name.trim(),
        description: description.trim() || null,
        source_config: sourceConfig,
        target_platforms: selectedPlatforms,
        template_ids: selectedTemplates,
      })
      onCreated()
    } catch (e) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50">
      <div className="bg-card border border-border rounded-xl shadow-xl w-full max-w-2xl max-h-[90vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border">
          <h2 className="text-lg font-semibold text-foreground">
            Create Campaign
          </h2>
          <Button
            variant="ghost"
            size="icon"
            onClick={onClose}
          >
            <X className="w-5 h-5" />
          </Button>
        </div>

        {/* Progress */}
        <div className="flex items-center px-6 py-3 border-b border-border bg-muted/50">
          {steps.map((s, i) => (
            <div key={s.key} className="flex items-center">
              <div
                className={cn(
                  'flex items-center justify-center w-7 h-7 rounded-full text-xs font-medium',
                  i <= currentStepIndex
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-muted text-muted-foreground'
                )}
              >
                {i < currentStepIndex ? (
                  <Check className="w-4 h-4" />
                ) : (
                  i + 1
                )}
              </div>
              <span
                className={cn(
                  'ml-2 text-sm',
                  i <= currentStepIndex
                    ? 'text-foreground font-medium'
                    : 'text-muted-foreground'
                )}
              >
                {s.title}
              </span>
              {i < steps.length - 1 && (
                <ChevronRight className="w-4 h-4 mx-2 text-muted-foreground" />
              )}
            </div>
          ))}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-auto p-6">
          {step === 'basics' && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="name">Campaign Name *</Label>
                <Input
                  id="name"
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g., March Content Push"
                />
              </div>
              <div>
                <Label htmlFor="description">Description</Label>
                <Textarea
                  id="description"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="What is this campaign about?"
                  rows={3}
                />
              </div>
            </div>
          )}

          {step === 'sources' && (
            <div className="space-y-4">
              <p className="text-sm text-muted-foreground mb-4">
                Select which content sources to use for generating social media posts.
              </p>
              
              <label className="flex items-center gap-3 p-4 border border-border rounded-lg hover:bg-accent cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_articles}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_articles: e.target.checked }))}
                  className="w-5 h-5 rounded border-border"
                />
                <div>
                  <div className="font-medium text-foreground">Articles</div>
                  <div className="text-sm text-muted-foreground">Generate posts from your blog articles</div>
                </div>
              </label>

              <label className="flex items-center gap-3 p-4 border border-border rounded-lg hover:bg-accent cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_screenshots}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_screenshots: e.target.checked }))}
                  className="w-5 h-5 rounded border-border"
                />
                <div>
                  <div className="font-medium text-foreground">Screenshots</div>
                  <div className="text-sm text-muted-foreground">Use screenshots from your project</div>
                </div>
              </label>

              <label className="flex items-center gap-3 p-4 border border-border rounded-lg hover:bg-accent cursor-pointer">
                <input
                  type="checkbox"
                  checked={sourceConfig.include_specs}
                  onChange={(e) => setSourceConfig(c => ({ ...c, include_specs: e.target.checked }))}
                  className="w-5 h-5 rounded border-border"
                />
                <div>
                  <div className="font-medium text-foreground">Specs</div>
                  <div className="text-sm text-muted-foreground">Include landing page specs</div>
                </div>
              </label>
            </div>
          )}

          {step === 'platforms' && (
            <div className="space-y-4">
              <p className="text-sm text-muted-foreground mb-4">
                Select which platforms to generate content for.
              </p>
              
              <div className="grid grid-cols-2 gap-3">
                {PLATFORMS.map(platform => (
                  <label
                    key={platform.id}
                    className={cn(
                      'flex items-center gap-3 p-4 border rounded-lg cursor-pointer transition-colors',
                      selectedPlatforms.includes(platform.id)
                        ? 'border-primary bg-accent'
                        : 'border-border hover:bg-accent'
                    )}
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
                      className="w-5 h-5 rounded border-border"
                    />
                    <span className="text-2xl">{platform.icon}</span>
                    <span className="font-medium text-foreground">
                      {platform.name}
                    </span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {step === 'templates' && (
            <div className="space-y-4">
              <p className="text-sm text-muted-foreground mb-4">
                Select templates to use for generating posts. Each template creates different styles of content.
              </p>
              
              <div className="space-y-3">
                {templates.length === 0 ? (
                  <div className="text-center py-8 text-muted-foreground">
                    No templates available. Using default templates.
                  </div>
                ) : (
                  templates.map(template => (
                    <label
                      key={template.id}
                      className={cn(
                        'flex items-start gap-3 p-4 border rounded-lg cursor-pointer transition-colors',
                        selectedTemplates.includes(template.id)
                          ? 'border-primary bg-accent'
                          : 'border-border hover:bg-accent'
                      )}
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
                        className="w-5 h-5 rounded border-border mt-0.5"
                      />
                      <div className="flex-1">
                        <div className="font-medium text-foreground">
                          {template.name}
                        </div>
                        <div className="text-sm text-muted-foreground mt-1">
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
              <div className="bg-muted rounded-lg p-4 space-y-4">
                <div>
                  <div className="text-sm text-muted-foreground">Campaign Name</div>
                  <div className="font-medium text-foreground">{name}</div>
                </div>
                
                {description && (
                  <div>
                    <div className="text-sm text-muted-foreground">Description</div>
                    <div className="text-foreground/80">{description}</div>
                  </div>
                )}
                
                <div>
                  <div className="text-sm text-muted-foreground">Content Sources</div>
                  <div className="flex flex-wrap gap-2 mt-1">
                    {sourceConfig.include_articles && <SourceBadge label="Articles" />}
                    {sourceConfig.include_screenshots && <SourceBadge label="Screenshots" />}
                    {sourceConfig.include_specs && <SourceBadge label="Specs" />}
                  </div>
                </div>
                
                <div>
                  <div className="text-sm text-muted-foreground">Platforms</div>
                  <div className="flex flex-wrap gap-2 mt-1">
                    {selectedPlatforms.map(p => <PlatformBadge key={p} platform={p} />)}
                  </div>
                </div>
                
                <div>
                  <div className="text-sm text-muted-foreground">Templates</div>
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

              <p className="text-sm text-muted-foreground">
                Click Create to start generating content. This will create a task that runs through the workflow.
              </p>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-border">
          <Button
            variant="ghost"
            onClick={step === 'basics' ? onClose : () => setStep(steps[currentStepIndex - 1].key)}
          >
            <ChevronLeft className="w-4 h-4 mr-2" />
            {step === 'basics' ? 'Cancel' : 'Back'}
          </Button>
          
          {step === 'review' ? (
            <Button
              onClick={handleCreate}
              disabled={loading}
            >
              {loading ? 'Creating...' : 'Create Campaign'}
            </Button>
          ) : (
            <Button
              onClick={() => setStep(steps[currentStepIndex + 1].key)}
              disabled={!canProceed()}
            >
              Next
              <ChevronRight className="w-4 h-4 ml-2" />
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

function SourceBadge({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center px-2 py-1 bg-secondary text-secondary-foreground text-xs rounded-md">
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
    <span className="inline-flex items-center gap-1 px-2 py-1 bg-muted text-muted-foreground text-xs rounded-md">
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
    <span className="inline-flex items-center px-2 py-1 bg-primary/10 text-primary text-xs rounded-md">
      {labels[format] || format}
    </span>
  )
}
