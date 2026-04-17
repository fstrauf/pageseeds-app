import { useState } from 'react'
import { AlertCircle, AlertTriangle, Info, ChevronDown, Wrench, Key, FileSearch, WifiOff, RefreshCw } from 'lucide-react'
import { cn } from '../../lib/utils'
import { Button } from '@/components/ui/button'

export interface ExplainedError {
  title: string
  summary: string
  steps: string[]
  severity: 'error' | 'warn' | 'info'
  canRetry: boolean
  icon: React.ReactNode
}

function detectKeywordResearchError(error: string): ExplainedError | null {
  const lower = error.toLowerCase()

  if (lower.includes('capsolver_api_key not set')) {
    return {
      title: 'CapSolver API key is missing',
      summary: 'Keyword research needs a CapSolver API key to bypass Ahrefs captcha. Without it, the tool cannot fetch keyword data.',
      steps: [
        'Open Settings → Secrets',
        'Add your CAPSOLVER_API_KEY',
        'Return to this task and click Retry',
      ],
      severity: 'error',
      canRetry: true,
      icon: <Key size={18} />,
    }
  }

  if (lower.includes('no keyword themes found in seed extraction artifact')) {
    return {
      title: 'Keyword themes were not extracted',
      summary: 'The first step of keyword research (agentic theme extraction) did not produce usable themes. This usually means the task description is too vague or empty.',
      steps: [
        'Edit this task and enter clear keyword themes in the description (one per line)',
        'Example: "content marketing", "SEO tools", "blog writing tips"',
        'Save changes, then click Retry',
      ],
      severity: 'warn',
      canRetry: true,
      icon: <FileSearch size={18} />,
    }
  }

  if (lower.includes('workspace not initialised') && lower.includes('articles.json')) {
    return {
      title: 'Workspace not initialized',
      summary: 'The project workspace is missing its articles.json file, which keyword research needs to avoid suggesting keywords you already cover.',
      steps: [
        'Go to Project Settings',
        'Click "Init Workspace"',
        'Return to this task and click Retry',
      ],
      severity: 'error',
      canRetry: true,
      icon: <Wrench size={18} />,
    }
  }

  if (lower.includes('no new keyword ideas found for themes')) {
    return {
      title: 'No new keyword ideas found',
      summary: 'The API did not return any fresh keywords for your themes. This can happen if all related topics are already covered in your articles.json.',
      steps: [
        'Try broader or more specific themes in the task description',
        'Check your existing articles to see if these topics are already covered',
        'If using DataForSEO, verify your account has API credits remaining',
      ],
      severity: 'info',
      canRetry: true,
      icon: <Info size={18} />,
    }
  }

  if (lower.includes('ahrefs') && (lower.includes('status') || lower.includes('returned status') || lower.includes('unable to parse'))) {
    return {
      title: 'Ahrefs API error',
      summary: 'The Ahrefs free API returned an unexpected response. This is often temporary due to rate limiting, captcha changes, or API downtime.',
      steps: [
        'Wait a minute and click Retry',
        'Check that your CapSolver key is valid and has credits',
        'If the error persists, try switching to DataForSEO in Project Settings',
      ],
      severity: 'warn',
      canRetry: true,
      icon: <WifiOff size={18} />,
    }
  }

  if (lower.includes('keyword research failed') || lower.includes('keyword research thread panicked')) {
    return {
      title: 'Keyword research pipeline failed',
      summary: 'An unexpected error occurred while running the keyword research pipeline. The detailed error is shown below.',
      steps: [
        'Check the technical details below for clues',
        'Verify your internet connection and API credentials',
        'Click Retry to attempt again',
      ],
      severity: 'error',
      canRetry: true,
      icon: <AlertCircle size={18} />,
    }
  }

  return null
}

function detectGenericError(error: string, taskType: string): ExplainedError {
  const lower = error.toLowerCase()

  if (lower.includes('no handler found')) {
    return {
      title: 'Task type not supported',
      summary: `The app does not yet have an execution handler for "${taskType}" tasks.`,
      steps: [
        'This task type may need to be run manually',
        'Check the docs or ask for support if you expected this to be automated',
      ],
      severity: 'error',
      canRetry: false,
      icon: <AlertTriangle size={18} />,
    }
  }

  if (lower.includes('failed to open db')) {
    return {
      title: 'Database connection failed',
      summary: 'The app could not open its local SQLite database. This is usually a file-permission or disk-space issue.',
      steps: [
        'Restart the app',
        'Check that your disk is not full',
        'If the problem persists, contact support',
      ],
      severity: 'error',
      canRetry: true,
      icon: <AlertCircle size={18} />,
    }
  }

  if (taskType === 'reddit_opportunity_search' && lower.includes('reddit')) {
    return {
      title: 'Reddit search failed',
      summary: 'The Reddit API or search step encountered an error.',
      steps: [
        'Check your internet connection',
        'Verify the search keywords in the task description',
        'Click Retry to attempt again',
      ],
      severity: 'warn',
      canRetry: true,
      icon: <WifiOff size={18} />,
    }
  }

  return {
    title: 'Task execution failed',
    summary: `The "${taskType}" task failed with an error.`,
    steps: [
      'Check the technical details below',
      'Fix any configuration issues mentioned',
      'Click Retry to attempt again',
    ],
    severity: 'error',
    canRetry: true,
    icon: <AlertCircle size={18} />,
  }
}

export function explainTaskError(error: string, taskType: string): ExplainedError {
  const keywordError = detectKeywordResearchError(error)
  if (keywordError) return keywordError
  return detectGenericError(error, taskType)
}

interface ErrorExplainerProps {
  error: string
  taskType: string
  onRetry?: () => void
  retrying?: boolean
  className?: string
}

export function ErrorExplainer({ error, taskType, onRetry, retrying, className }: ErrorExplainerProps) {
  const [showRaw, setShowRaw] = useState(false)
  const explained = explainTaskError(error, taskType)

  const severityClasses = {
    error: 'bg-destructive/10 text-destructive border-destructive/20',
    warn: 'bg-amber-500/10 text-amber-700 border-amber-500/20 dark:text-amber-400',
    info: 'bg-blue-500/10 text-blue-700 border-blue-500/20 dark:text-blue-400',
  }

  return (
    <div className={cn('rounded-lg border p-4 space-y-3', severityClasses[explained.severity], className)}>
      <div className="flex items-start gap-3">
        <div className="mt-0.5 shrink-0 opacity-80">{explained.icon}</div>
        <div className="min-w-0 flex-1 space-y-1">
          <div className="font-medium text-sm">{explained.title}</div>
          <div className="text-xs opacity-90 leading-relaxed">{explained.summary}</div>
        </div>
      </div>

      {explained.steps.length > 0 && (
        <div className="pt-1">
          <div className="text-[11px] font-medium uppercase tracking-wide opacity-70 mb-1.5">What to do</div>
          <ol className="space-y-1.5">
            {explained.steps.map((step, i) => (
              <li key={i} className="flex items-start gap-2 text-xs">
                <span className="inline-flex items-center justify-center w-4 h-4 rounded-full bg-background/60 text-[10px] font-medium shrink-0 mt-0.5">
                  {i + 1}
                </span>
                <span className="opacity-90 leading-relaxed">{step}</span>
              </li>
            ))}
          </ol>
        </div>
      )}

      {explained.canRetry && onRetry && (
        <div className="pt-1">
          <Button
            size="xs"
            variant="outline"
            onClick={onRetry}
            disabled={retrying}
            className="text-xs border-current/30 bg-background/50 hover:bg-background"
          >
            <RefreshCw size={12} className={cn('mr-1.5', retrying && 'animate-spin')} />
            {retrying ? 'Retrying…' : 'Retry task'}
          </Button>
        </div>
      )}

      <div className="pt-1 border-t border-current/10">
        <button
          onClick={() => setShowRaw(v => !v)}
          className="flex items-center gap-1 text-[11px] opacity-70 hover:opacity-100 transition-opacity"
        >
          <ChevronDown size={12} className={cn('transition-transform', showRaw && 'rotate-180')} />
          {showRaw ? 'Hide technical details' : 'Show technical details'}
        </button>
        {showRaw && (
          <pre className="mt-2 p-2.5 rounded bg-background/60 text-[11px] font-mono whitespace-pre-wrap break-words opacity-80">
            {error}
          </pre>
        )}
      </div>
    </div>
  )
}
