import { useState } from 'react'
import type { ExecutionResult } from '../lib/types'

// ─── Result shapes ────────────────────────────────────────────────────────────

/** Rich step-by-step result from a workflow execution. */
export interface StepResultPayload {
  kind: 'execution'
  data: ExecutionResult
}

/** Simple key/value summary — for one-shot operations like "fix dates". */
export interface SummaryResultPayload {
  kind: 'summary'
  success: boolean
  items: Array<{ label: string; value: string }>
  message?: string
}

/** Plain success/error message with no detail. */
export interface MessageResultPayload {
  kind: 'message'
  success: boolean
  text: string
}

export type ActionResultPayload =
  | StepResultPayload
  | SummaryResultPayload
  | MessageResultPayload

// ─── State ────────────────────────────────────────────────────────────────────

export type ActionStatus = 'idle' | 'running' | 'done' | 'error'

export interface ActionState {
  status: ActionStatus
  label: string
  result: ActionResultPayload | null
  errorMessage: string | null
  /** Optional navigation hint shown as a CTA when done. */
  nextStep?: { view: string; label: string }
}

const IDLE: ActionState = {
  status: 'idle',
  label: '',
  result: null,
  errorMessage: null,
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Universal hook for async button actions.
 *
 * Usage:
 *   const { state, run, dismiss } = useActionRun()
 *
 *   <Button onClick={() => run('Fix dates', async () => {
 *     const r = await fixDateMismatches(projectId)
 *     return {
 *       kind: 'summary',
 *       success: true,
 *       items: [
 *         { label: 'Articles checked', value: String(r.checked) },
 *         { label: 'Dates fixed', value: String(r.date_mismatches) },
 *       ],
 *     }
 *   })}>Fix dates</Button>
 *
 *   <ActionDrawer state={state} onDismiss={dismiss} />
 */
export function useActionRun() {
  const [state, setState] = useState<ActionState>(IDLE)

  async function run(
    label: string,
    fn: () => Promise<ActionResultPayload>,
    nextStep?: ActionState['nextStep'],
  ) {
    setState({ status: 'running', label, result: null, errorMessage: null, nextStep })
    try {
      const result = await fn()
      setState({ status: 'done', label, result, errorMessage: null, nextStep })
    } catch (e: unknown) {
      setState({
        status: 'error',
        label,
        result: null,
        errorMessage: String(e),
        nextStep,
      })
    }
  }

  function dismiss() {
    setState(IDLE)
  }

  return { state, run, dismiss }
}
