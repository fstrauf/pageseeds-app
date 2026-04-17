import { createContext, useContext, useState, useCallback, type ReactNode } from 'react'

export interface Toast {
  id: string
  message: string
  type: 'error' | 'success' | 'info'
}

interface ToastContextValue {
  toasts: Toast[]
  showError: (message: string) => void
  showSuccess: (message: string) => void
  showInfo: (message: string) => void
  removeToast: (id: string) => void
}

const ToastContext = createContext<ToastContextValue | null>(null)

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([])

  const addToast = useCallback((message: string, type: Toast['type']) => {
    const id = `${Date.now()}-${Math.random()}`
    setToasts(prev => [...prev, { id, message, type }])
    // Auto-dismiss after 5s for success/info, 8s for errors
    const duration = type === 'error' ? 8000 : 5000
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id))
    }, duration)
  }, [])

  const showError = useCallback((message: string) => addToast(message, 'error'), [addToast])
  const showSuccess = useCallback((message: string) => addToast(message, 'success'), [addToast])
  const showInfo = useCallback((message: string) => addToast(message, 'info'), [addToast])

  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  return (
    <ToastContext.Provider value={{ toasts, showError, showSuccess, showInfo, removeToast }}>
      {children}
      <ToastContainer />
    </ToastContext.Provider>
  )
}

export function useErrorHandler() {
  const ctx = useContext(ToastContext)
  if (!ctx) {
    throw new Error('useErrorHandler must be used within ToastProvider')
  }
  return ctx
}

function ToastContainer() {
  const ctx = useContext(ToastContext)
  if (!ctx || ctx.toasts.length === 0) return null

  return (
    <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2">
      {ctx.toasts.map(toast => (
        <div
          key={toast.id}
          className={[
            'pointer-events-auto flex max-w-sm items-start gap-3 rounded-md border px-4 py-3 shadow-lg',
            toast.type === 'error'
              ? 'border-destructive bg-destructive/10 text-destructive'
              : toast.type === 'success'
                ? 'border-green-600 bg-green-600/10 text-green-700'
                : 'border-border bg-card text-foreground',
          ].join(' ')}
        >
          <span className="text-sm">{toast.message}</span>
          <button
            onClick={() => ctx.removeToast(toast.id)}
            className="ml-2 text-xs opacity-70 hover:opacity-100"
            aria-label="Dismiss"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  )
}
