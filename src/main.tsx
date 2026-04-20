import React, { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/globals.css'
import App from './App.tsx'
import { ToastProvider } from './lib/toast-context.tsx'
import { ErrorBoundary } from './components/ui/ErrorBoundary.tsx'

async function bootstrap() {
  // why-did-you-render dev guard — logs exactly which prop/reference changed
  // Must load BEFORE createRoot so hook patching happens before any renders.
  if (import.meta.env.DEV) {
    const wdyr = await import('@welldone-software/why-did-you-render')
    wdyr.default(React, {
      trackAllPureComponents: true,
    })
  }

  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <ErrorBoundary>
        <ToastProvider>
          <App />
        </ToastProvider>
      </ErrorBoundary>
    </StrictMode>,
  )
}

bootstrap()
