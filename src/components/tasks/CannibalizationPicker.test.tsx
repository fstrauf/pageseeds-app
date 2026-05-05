import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { CannibalizationPicker } from './CannibalizationPicker'
import type { Task } from '../../lib/types'

const mockEnqueueNext = vi.fn()
const mockShowError = vi.fn()
const mockCreateTasks = vi.fn()

vi.mock('../../lib/queue-context', () => ({
  useQueue: () => ({ enqueueNext: mockEnqueueNext }),
}))

vi.mock('../../lib/toast-context', () => ({
  useErrorHandler: () => ({ showError: mockShowError }),
}))

vi.mock('../../lib/tauri', () => ({
  createCannibalizationTasksFromSelection: (...args: unknown[]) => mockCreateTasks(...args),
}))

function makeTask(artifactContent: object): Task {
  return {
    id: 'task-1',
    type: 'cannibalization_audit',
    project_id: 'proj-1',
    status: 'review',
    priority: 'medium',
    run_policy: 'auto_enqueue',
    review_surface: 'cannibalization_picker',
    follow_up_policy: 'user_selection',
    agent_policy: 'optional',
    phase: 'investigation',
    title: 'Audit',
    description: null,
    depends_on: [],
    not_before: null,
    artifacts: [
      {
        key: 'cannibalization_strategy',
        path: null,
        type: 'json',
        source: 'cannibalization_audit',
        content: JSON.stringify(artifactContent),
      },
    ],
    run: { attempts: 0, last_error: null, provider: null, prompt_tokens: null, completion_tokens: null },
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
}

describe('CannibalizationPicker', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders recommendations from task artifact', () => {
    const task = makeTask({
      merge_recommendations: [
        { cluster_id: 'cluster-a', keep_url: '/a', redirect_urls: ['/b'], reason: 'Dup' },
      ],
      hub_recommendations: [
        { topic: 'hub-a', suggested_title: 'Hub A', suggested_url: '/hub/a', spoke_pages: [1] },
      ],
      territory_recommendations: [
        { theme: 'territory-a', priority: 'high' },
      ],
      calculator_recommendations: [
        { strategy: 'calc-a', ticker_universe: 'US', indexing_policy: 'weekly', reason: 'R' },
      ],
      risks: [],
    })

    render(<CannibalizationPicker task={task} onTasksCreated={vi.fn()} />)

    expect(screen.getByText('Merge: cluster-a')).toBeInTheDocument()
    expect(screen.getByText('Hub A')).toBeInTheDocument()
    expect(screen.getByText('Territory: territory-a')).toBeInTheDocument()
    expect(screen.getByText('Calculator: calc-a')).toBeInTheDocument()
  })

  it('shows empty state when no recommendations exist', () => {
    const task = makeTask({
      merge_recommendations: [],
      hub_recommendations: [],
      territory_recommendations: [],
      calculator_recommendations: [],
      risks: [],
    })

    render(<CannibalizationPicker task={task} onTasksCreated={vi.fn()} />)

    expect(screen.getByText(/No recommendations found/)).toBeInTheDocument()
  })

  it('allows selecting and creating tasks', async () => {
    const task = makeTask({
      merge_recommendations: [
        { cluster_id: 'cluster-a', keep_url: '/a', redirect_urls: ['/b'], reason: 'Dup' },
      ],
      hub_recommendations: [],
      territory_recommendations: [],
      calculator_recommendations: [],
      risks: [],
    })

    const onTasksCreated = vi.fn()
    mockCreateTasks.mockResolvedValueOnce([
      {
        id: 'child-1',
        task_type: 'consolidate_cluster',
        project_id: 'proj-1',
        title: 'Merge cluster: cluster-a',
        status: 'todo',
        priority: 'medium',
        run_policy: 'user_enqueue',
        review_surface: 'none',
        follow_up_policy: 'none',
        agent_policy: 'required',
        phase: 'implementation',
        description: null,
        depends_on: [],
        artifacts: [],
        run: { attempts: 0, last_error: null, provider: null },
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      },
    ])

    render(<CannibalizationPicker task={task} onTasksCreated={onTasksCreated} />)

    // Click the checkbox toggle for the first (and only) recommendation row
    const toggleButton = screen.getAllByRole('button').find(b =>
      b.querySelector('svg')
    )!
    fireEvent.click(toggleButton)

    const createButton = screen.getByRole('button', { name: /Create Tasks/i })
    fireEvent.click(createButton)

    await waitFor(() => {
      expect(mockCreateTasks).toHaveBeenCalledWith('task-1', [
        { recommendation_type: 'merge', recommendation_id: 'cluster-a' },
      ])
    })

    expect(onTasksCreated).toHaveBeenCalled()
    expect(mockEnqueueNext).toHaveBeenCalled()
  })

  it('disables create button when nothing is selected', () => {
    const task = makeTask({
      merge_recommendations: [
        { cluster_id: 'cluster-a', keep_url: '/a', redirect_urls: ['/b'], reason: 'Dup' },
      ],
      hub_recommendations: [],
      territory_recommendations: [],
      calculator_recommendations: [],
      risks: [],
    })

    render(<CannibalizationPicker task={task} onTasksCreated={vi.fn()} />)

    const createButton = screen.getByRole('button', { name: /Create Tasks/i })
    expect(createButton).toBeDisabled()
  })
})
