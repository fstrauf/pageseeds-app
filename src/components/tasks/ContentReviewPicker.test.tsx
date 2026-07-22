import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ContentReviewPicker } from './ContentReviewPicker'
import type { Task } from '../../lib/types'

const mockEnqueue = vi.fn()
const mockShowError = vi.fn()
const mockSelect = vi.fn()

vi.mock('../../lib/queue-context', () => ({
  useQueue: () => ({ enqueue: mockEnqueue }),
}))

vi.mock('../../lib/toast-context', () => ({
  useErrorHandler: () => ({ showError: mockShowError }),
}))

vi.mock('../../lib/tauri', () => ({
  selectContentReviewFollowUps: (...args: unknown[]) => mockSelect(...args),
}))

function makeTask(artifactContent: object): Task {
  return {
    id: 'task-1',
    type: 'content_review',
    project_id: 'proj-1',
    status: 'review',
    priority: 'medium',
    run_policy: 'user_enqueue',
    review_surface: 'content_review_picker',
    follow_up_policy: 'user_selection',
    agent_policy: 'required',
    phase: 'investigation',
    title: 'Content Review',
    description: null,
    depends_on: [],
    not_before: null,
    artifacts: [
      {
        key: 'content_review_proposals',
        path: null,
        type: 'json',
        source: 'recommendations',
        content: JSON.stringify(artifactContent),
      },
    ],
    run: { attempts: 0, last_error: null, provider: null, prompt_tokens: null, completion_tokens: null },
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
}

describe('ContentReviewPicker', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders proposals from task artifact', () => {
    const task = makeTask({
      findings_summary: '2 article(s) with fix recommendations',
      source: 'recommendations',
      dropped: [],
      proposals: [
        {
          id: 'fix_content_article:1',
          task_type: 'fix_content_article',
          title: 'Fix: Alpha',
          description: 'Apply SEO recommendations to Alpha',
          params: { article_id: 1 },
          idempotency_key: 'fix_content_article:proj-1:1',
          priority: 'high',
        },
        {
          id: 'fix_content_article:2',
          task_type: 'fix_content_article',
          title: 'Fix: Beta',
          description: null,
          params: { article_id: 2 },
          idempotency_key: 'fix_content_article:proj-1:2',
          priority: 'medium',
        },
      ],
    })

    render(<ContentReviewPicker task={task} onTasksCreated={vi.fn()} />)

    expect(screen.getByText('Fix: Alpha')).toBeInTheDocument()
    expect(screen.getByText('Fix: Beta')).toBeInTheDocument()
    expect(screen.getByText(/2 article\(s\) with fix recommendations/)).toBeInTheDocument()
  })

  it('shows empty state when no proposals exist', () => {
    const task = makeTask({
      findings_summary: null,
      source: 'recommendations',
      proposals: [],
      dropped: [{ reason: 'cap_exceeded', task_type: 'fix_content_article', detail: null }],
    })

    render(<ContentReviewPicker task={task} onTasksCreated={vi.fn()} />)

    expect(screen.getByText(/No fix proposals available/)).toBeInTheDocument()
    expect(screen.getByText(/1 proposal was dropped/)).toBeInTheDocument()
  })

  it('allows selecting and creating tasks', async () => {
    const task = makeTask({
      findings_summary: null,
      source: 'recommendations',
      dropped: [],
      proposals: [
        {
          id: 'fix_content_article:1',
          task_type: 'fix_content_article',
          title: 'Fix: Alpha',
          description: 'Apply fixes',
          params: { article_id: 1 },
          idempotency_key: 'fix_content_article:proj-1:1',
          priority: 'high',
        },
      ],
    })

    const onTasksCreated = vi.fn()
    mockSelect.mockResolvedValueOnce([
      {
        id: 'child-1',
        type: 'fix_content_article',
        project_id: 'proj-1',
        title: 'Fix: Alpha',
        status: 'todo',
        priority: 'high',
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

    render(<ContentReviewPicker task={task} onTasksCreated={onTasksCreated} />)

    const toggleButton = screen.getAllByRole('button').find(b =>
      b.querySelector('svg'),
    )!
    fireEvent.click(toggleButton)

    const createButton = screen.getByRole('button', { name: /Create Tasks/i })
    fireEvent.click(createButton)

    await waitFor(() => {
      expect(mockSelect).toHaveBeenCalledWith('task-1', ['fix_content_article:1'])
    })

    expect(onTasksCreated).toHaveBeenCalled()
    expect(mockEnqueue).toHaveBeenCalledWith([
      {
        taskId: 'child-1',
        projectId: 'proj-1',
        title: 'Fix: Alpha',
        taskType: 'fix_content_article',
        projectName: undefined,
      },
    ])
  })

  it('disables create button when nothing is selected', () => {
    const task = makeTask({
      findings_summary: null,
      source: 'recommendations',
      dropped: [],
      proposals: [
        {
          id: 'fix_content_article:1',
          task_type: 'fix_content_article',
          title: 'Fix: Alpha',
          description: null,
          params: { article_id: 1 },
          idempotency_key: 'fix_content_article:proj-1:1',
          priority: 'low',
        },
      ],
    })

    render(<ContentReviewPicker task={task} onTasksCreated={vi.fn()} />)

    const createButton = screen.getByRole('button', { name: /Create Tasks/i })
    expect(createButton).toBeDisabled()
  })
})
