/**
 * Queue Runner Hook
 * 
 * Bridge between legacy context API and new Zustand store.
 */

import { useQueueStore } from '../stores/queueStore';
import type { QueueItem, RunnerItem } from '../lib/types';
import { useEffect, useMemo, useRef } from 'react';
import { createLogger, LogTarget } from '../lib/logging';

const logger = createLogger(LogTarget.QUEUE);

export function useQueueRunner(onCompleted?: () => void) {
  logger.entry('useQueueRunner');

  // Guard to prevent calling onCompleted multiple times for the same completion
  const completedRef = useRef(false);

  // Use selectors to avoid re-rendering on unrelated store changes
  const itemsRaw = useQueueStore(s => s.items);
  const isRunning = useQueueStore(s => s.isRunning);
  const isPaused = useQueueStore(s => s.isPaused);
  const isVisible = useQueueStore(s => s.isVisible);

  // Get action methods from store (stable references from Zustand)
  const enqueueStore = useQueueStore(s => s.enqueue);
  const enqueueNextStore = useQueueStore(s => s.enqueueNext);
  const removeItemStore = useQueueStore(s => s.removeItem);
  const pauseStore = useQueueStore(s => s.pause);
  const resumeStore = useQueueStore(s => s.resume);
  const closeStore = useQueueStore(s => s.close);

  logger.debug('render', {
    isRunning,
    isPaused,
    itemCount: itemsRaw.length,
    items: itemsRaw.map(i => ({ id: i.taskId, status: i.status })),
  });

  useEffect(() => {
    logger.debug('useEffect - state changed', {
      isRunning,
      items: itemsRaw.length,
    });

    if (!isRunning && itemsRaw.length > 0) {
      const allDone = itemsRaw.every((i: QueueItem) => i.status === 'completed' || i.status === 'failed');
      logger.debug('useEffect - allDone check', { allDone });

      if (allDone && !completedRef.current) {
        logger.info('useEffect - queue complete, calling onCompleted');
        completedRef.current = true;
        onCompleted?.();
      }
    }

    // Reset the guard when the queue becomes active again or is cleared
    if (isRunning || itemsRaw.length === 0) {
      completedRef.current = false;
    }
  }, [isRunning, itemsRaw, onCompleted]);

  const items: RunnerItem[] = useMemo(
    () =>
      itemsRaw.map((item: QueueItem) => ({
        task: {
          id: item.taskId,
          title: item.title,
          type: item.taskType,
          projectId: item.projectId,
          projectName: item.projectName,
        },
        status:
          item.status === 'pending'
            ? 'queued'
            : item.status === 'completed'
              ? 'done'
              : item.status === 'running'
                ? 'running'
                : 'failed',
        error: item.error,
        liveSteps: [],
        result: item.result,
      })),
    [itemsRaw],
  );

  logger.exit('useQueueRunner');

  return useMemo(
    () => ({
      items,
      isRunning,
      isPaused,
      isVisible,

      enqueue: (newItems: QueueItem[]) => {
        logger.entry('enqueue', { count: newItems.length });
        console.log('[useQueueRunner] enqueue called with', newItems.length, 'items');
        const itemsWithStatus = newItems.map((i) => ({ ...i, status: 'pending' as const }));
        console.log('[useQueueRunner] calling store.enqueue...');
        enqueueStore(itemsWithStatus);
        console.log('[useQueueRunner] store.enqueue returned');
        logger.exit('enqueue');
      },

      enqueueNext: (newItems: QueueItem[]) => {
        logger.entry('enqueueNext', { count: newItems.length });
        const itemsWithStatus = newItems.map((i) => ({ ...i, status: 'pending' as const }));
        enqueueNextStore(itemsWithStatus);
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
    }),
    [items, isRunning, isPaused, isVisible, enqueueStore, enqueueNextStore, removeItemStore, pauseStore, resumeStore, closeStore],
  );
}
