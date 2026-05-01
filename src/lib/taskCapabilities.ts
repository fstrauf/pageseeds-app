import type { TaskRunPolicy, TaskReviewSurface, FollowUpPolicy } from './types'

interface HasRunPolicy {
  run_policy: string
}

export function getTaskRunPolicy(task: HasRunPolicy): TaskRunPolicy {
  return task.run_policy as TaskRunPolicy
}

export function canEnqueue(task: HasRunPolicy): boolean {
  const policy = getTaskRunPolicy(task)
  return policy === 'auto_enqueue' || policy === 'user_enqueue'
}

export function canAutoEnqueue(task: HasRunPolicy): boolean {
  return getTaskRunPolicy(task) === 'auto_enqueue'
}

export function getTaskReviewSurface(task: { review_surface: string }): TaskReviewSurface {
  return task.review_surface as TaskReviewSurface
}

export function hasReviewSurface(task: { review_surface: string }): boolean {
  return getTaskReviewSurface(task) !== 'none'
}

export function getFollowUpPolicy(task: { follow_up_policy: string }): FollowUpPolicy {
  return task.follow_up_policy as FollowUpPolicy
}

export function getReviewLabel(surface: TaskReviewSurface | string): string {
  switch (surface) {
    case 'keyword_picker':
      return 'Select keywords'
    case 'reddit_picker':
      return 'Select opportunities'
    case 'cannibalization_picker':
      return 'Select recommendations'
    case 'artifact_review':
      return 'Review results'
    case 'follow_up_tasks':
      return 'View follow-ups'
    default:
      return 'Open task'
  }
}

export function getReviewSurfaceTitle(surface: TaskReviewSurface): string {
  switch (surface) {
    case 'keyword_picker':
      return 'Keyword Results'
    case 'reddit_picker':
      return 'Reddit Opportunities'
    case 'cannibalization_picker':
      return 'Cannibalization Recommendations'
    case 'follow_up_tasks':
      return 'Next Steps'
    case 'artifact_review':
      return 'Review'
    default:
      return ''
  }
}
