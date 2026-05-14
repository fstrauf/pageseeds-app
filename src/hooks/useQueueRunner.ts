/**
 * Queue Runner Hook
 *
 * Bridge between the backend-owned queue and the TaskRunner UI.
 * Maps QueueSnapshot to the component props expected by TaskRunner.
 */

import { useQueueStore } from '../stores/queueStore';
import type { QueueItem, RunnerItem, ExecutionResult, EnqueueItem } from '../lib/types';
import { useEffect, useMemo, useRef } from 'react';
import { createLogger, LogTarget } from '../lib/logging';

const logger = createLogger(LogTarget.QUEUE);

function mapQueueItemToRunnerItem(item: QueueItem): RunnerItem {
  const statusMap: Record<string, RunnerItem['status']> = {
    pending: 'queued',
    running: 'running',
    completed: 'done',
    failed: 'failed',
    skipped: 'failed',
  };

  return {
    task: {
      id: item.task_id,
      title: item.title ?? undefined,
      type: item.task_type ?? undefined,
      projectId: item.project_id ?? undefined,
      projectName: item.project_name ?? undefined,
    },
    status: statusMap[item.status] ?? 'queued',
    error: item.error ?? undefined,
    liveSteps: [],
    result: item.result_json
      ? (JSON.parse(item.result_json) as ExecutionResult)
      : undefined,
  };
}

export function useQueueRunner(onCompleted?: () => void) {
  logger.entry('useQueueRunner');

  const completedRef = useRef(false);

  const snapshot = useQueueStore((s) => s.snapshot);
  const isVisible = useQueueStore((s) => s.isVisible);
  const isStarting = useQueueStore((s) => s.isStarting);
  const sync = useQueueStore((s) => s.sync);
  const enqueueStore = useQueueStore((s) => s.enqueue);
  const removeItemStore = useQueueStore((s) => s.removeItem);
  const pauseStore = useQueueStore((s) => s.pause);
  const resumeStore = useQueueStore((s) => s.resume);
  const closeStore = useQueueStore((s) => s.close);
  const clearCompletedStore = useQueueStore((s) => s.clearCompleted);
  const setupEventListeners = useQueueStore((s) => s.setupEventListeners);

  const isRunning = snapshot?.run?.status === 'running' || isStarting;
  const isPaused = snapshot?.run?.status === 'paused' && !isStarting;
  const itemsRaw = useMemo(() => snapshot?.items ?? [], [snapshot]);

  logger.debug('render', {
    isRunning,
    isPaused,
    itemCount: itemsRaw.length,
  });

  // Sync on mount and setup listeners
  useEffect(() => {
    logger.info('useQueueRunner - mounting, syncing and setting up listeners');
    void sync();
    void setupEventListeners();
    return () => {
      useQueueStore.getState().cleanupEventListeners();
    };
  }, [sync, setupEventListeners]);

  // Call onCompleted when queue finishes
  useEffect(() => {
    if (!isRunning && itemsRaw.length > 0) {
      const allDone = itemsRaw.every(
        (i: QueueItem) => i.status === 'completed' || i.status === 'failed' || i.status === 'skipped'
      );
      if (allDone && !completedRef.current) {
        completedRef.current = true;
        onCompleted?.();
      }
    }
    if (isRunning || itemsRaw.length === 0) {
      completedRef.current = false;
    }
  }, [isRunning, itemsRaw, onCompleted]);

  const items: RunnerItem[] = useMemo(
    () => itemsRaw.map(mapQueueItemToRunnerItem),
    [itemsRaw]
  );



  logger.exit('useQueueRunner');

  return useMemo(
    () => ({
      items,
      isRunning,
      isPaused,
      isVisible,
      isStarting,

      enqueue: (newItems: { taskId: string; projectId: string; title?: string; taskType?: string; projectName?: string }[]) => {
        logger.entry('enqueue', { count: newItems.length });
        const enqueueItems = newItems.map((i) => ({
          task_id: i.taskId,
          project_id: i.projectId,
          title: i.title ?? null,
          task_type: i.taskType ?? null,
          project_name: i.projectName ?? null,
        }));
        enqueueStore(enqueueItems as EnqueueItem[], 'append');
        logger.exit('enqueue');
      },

      enqueueNext: (newItems: { taskId: string; projectId: string; title?: string; taskType?: string; projectName?: string }[]) => {
        logger.entry('enqueueNext', { count: newItems.length });
        const enqueueItems = newItems.map((i) => ({
          task_id: i.taskId,
          project_id: i.projectId,
          title: i.title ?? null,
          task_type: i.taskType ?? null,
          project_name: i.projectName ?? null,
        }));
        enqueueStore(enqueueItems as EnqueueItem[], 'next');
        logger.exit('enqueueNext');
      },

      removeItem: (taskId: string) => {
        logger.entry('removeItem', { taskId });
        removeItemStore(taskId);
        logger.exit('removeItem');
      },

      pause: () => {
        logger.entry('pause');
        pauseStore();
        logger.exit('pause');
      },

      resume: () => {
        logger.entry('resume');
        resumeStore();
        logger.exit('resume');
      },

      close: () => {
        logger.entry('close');
        closeStore();
        logger.exit('close');
      },

      clearCompleted: () => {
        logger.entry('clearCompleted');
        clearCompletedStore();
        logger.exit('clearCompleted');
      },
    }),
    [items, isRunning, isPaused, isVisible, isStarting, enqueueStore, removeItemStore, pauseStore, resumeStore, closeStore, clearCompletedStore]
  );
}
