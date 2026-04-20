import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { TaskBoard } from './TaskBoard'
import { useQueueStore } from '../../stores/queueStore'
import { ToastProvider } from '../../lib/toast-context'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    listTasks: vi.fn(() => Promise.resolve([])),
    getTask: vi.fn(() => Promise.resolve(null)),
    deleteTask: vi.fn(() => Promise.resolve()),
    importFromRepo: vi.fn(() => Promise.resolve()),
    exportToRepo: vi.fn(() => Promise.resolve()),
    analyzeArticleDatePolicy: vi.fn(() => Promise.resolve()),
  }
})

vi.mock('@/components/ui/tabs', () => ({
  Tabs: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TabsList: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TabsTrigger: ({ children }: { children: React.ReactNode }) => <button>{children}</button>,
}))

vi.mock('@/components/ui/select', () => ({
  Select: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SelectContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SelectItem: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SelectTrigger: ({ children }: { children: React.ReactNode }) => <button>{children}</button>,
  SelectValue: () => <span>value</span>,
}))

vi.mock('@/components/ui/sheet', () => ({
  Sheet: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SheetContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}))

describe('TaskBoard', () => {
  beforeEach(() => {
    useQueueStore.setState({
      items: [],
      isRunning: false,
      isPaused: false,
      isVisible: false,
    })
  })

  it('mounts without excessive re-renders', async () => {
    const renderSpy = vi.fn()

    function InstrumentedTaskBoard(props: React.ComponentProps<typeof TaskBoard>) {
      renderSpy()
      return <TaskBoard {...props} />
    }

    render(
      <ToastProvider>
        <InstrumentedTaskBoard
          projectId="p1"
          projectName="Test Project"
          runCompletedTick={1}
        />
      </ToastProvider>
    )

    await waitFor(() => {
      expect(renderSpy.mock.calls.length).toBeLessThan(10)
    }, { timeout: 2000 })
  })
})
