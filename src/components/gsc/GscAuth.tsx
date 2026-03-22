import { useEffect, useState } from 'react'
import { gscGetAuthStatus, gscAuthenticate, gscOAuthStart } from '../../lib/tauri'
import type { GscAuthStatus } from '../../lib/types'

interface Props {
  projectId: string
  onAuthenticated?: () => void
}

export function GscAuth({ projectId, onAuthenticated }: Props) {
  const [status, setStatus] = useState<GscAuthStatus | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)

  useEffect(() => {
    loadStatus()
  }, [projectId])

  async function loadStatus() {
    try {
      const s = await gscGetAuthStatus(projectId)
      setStatus(s)
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleSaAuth() {
    setLoading(true)
    setError(null)
    setMessage(null)
    try {
      await gscAuthenticate(projectId)
      setMessage('Authenticated via service account.')
      onAuthenticated?.()
      await loadStatus()
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function handleOAuth() {
    setLoading(true)
    setError(null)
    setMessage(null)
    try {
      setMessage('Opening browser… waiting for OAuth callback (3 min timeout).')
      await gscOAuthStart(projectId)
      setMessage('OAuth authentication complete.')
      onAuthenticated?.()
      await loadStatus()
    } catch (e) {
      setError(String(e))
      setMessage(null)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="max-w-xl space-y-4">
      <h2 className="text-base font-semibold">Search Console Authentication</h2>

      {status && (
        <div
          className="rounded-md border p-4 space-y-2 text-sm"
          style={{ borderColor: 'var(--color-border)', background: 'var(--color-card)' }}
        >
          <StatusRow label="Service account" ok={status.service_account_configured} detail={status.sa_path} />
          <StatusRow label="OAuth credentials" ok={status.oauth_configured} detail={status.oauth_path} />
          <StatusRow
            label="Authenticated"
            ok={status.authenticated}
            detail={status.method ? `via ${status.method}` : undefined}
          />
        </div>
      )}

      {error && (
        <p className="text-xs text-destructive bg-destructive/10 rounded px-3 py-2">{error}</p>
      )}
      {message && (
        <p className="text-xs text-muted-foreground bg-muted/40 rounded px-3 py-2">{message}</p>
      )}

      <div className="flex gap-2 flex-wrap">
        <button
          onClick={handleSaAuth}
          disabled={loading || !status?.service_account_configured}
          className="px-3 py-1.5 rounded text-xs bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
        >
          {loading ? 'Authenticating…' : 'Authenticate (Service Account)'}
        </button>
        <button
          onClick={handleOAuth}
          disabled={loading || !status?.oauth_configured}
          className="px-3 py-1.5 rounded text-xs border border-border hover:bg-muted disabled:opacity-40"
        >
          {loading ? 'Authenticating…' : 'Authenticate (OAuth2)'}
        </button>
        <button
          onClick={loadStatus}
          disabled={loading}
          className="px-3 py-1.5 rounded text-xs border border-border hover:bg-muted disabled:opacity-40"
        >
          Refresh
        </button>
      </div>

      <div
        className="rounded-md border p-3 text-xs text-muted-foreground space-y-1"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <p className="font-medium text-foreground">Required environment variables</p>
        <p><code className="font-mono">GSC_SERVICE_ACCOUNT_PATH</code> — path to service account JSON key</p>
        <p><code className="font-mono">GSC_REPORT_OAUTH_CLIENT_SECRETS</code> — path to OAuth client secrets JSON</p>
        <p>Set these in your project's <code className="font-mono">secrets.env</code> or shell environment.</p>
      </div>
    </div>
  )
}

function StatusRow({
  label,
  ok,
  detail,
}: {
  label: string
  ok: boolean
  detail?: string
}) {
  return (
    <div className="flex items-center gap-2">
      <span className={`w-2 h-2 rounded-full ${ok ? 'bg-green-500' : 'bg-muted-foreground/40'}`} />
      <span className="font-medium w-40">{label}</span>
      {ok ? (
        <span className="text-muted-foreground truncate">{detail ?? '✓'}</span>
      ) : (
        <span className="text-muted-foreground">not configured</span>
      )}
    </div>
  )
}
