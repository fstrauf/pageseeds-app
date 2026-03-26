/**
 * Queue Runner Hook
 * 
 * Bridge between legacy context API and new Zustand store.
 */

import { useQueueStore } from '../stores/queueStore';
import type { QueueItem, RunnerItem } from '../lib/types';
import { useEffect } from 'react';
import { createLogger, LogTarget } from '../lib/logging';

const logger = createLogger(LogTarget.QUEUE);

export function useQueueRunner(onCompleted?: () => void) {
  logger.entry('useQueueRunner');
  const store = useQueueStore();

  logger.debug('render', { 
    isRunning: store.isRunning, 
    isPaused: store.isPaused, 
    itemCount: store.items.length,
    items: store.items.map(i => ({ id: i.taskId, status: i.status }))
  });

  useEffect(() => {
    logger.debug('useEffect - state changed', { 
      isRunning: store.isRunning, 
      items: store.items.length 
    });
    
    if (!store.isRunning && store.items.length > 0) {
      const allDone = store.items.every((i: QueueItem) => i.status === 'completed' || i.status === 'failed');
      logger.debug('useEffect - allDone check', { allDone });
      
      if (allDone) {
        logger.info('useEffect - queue complete, calling onCompleted');
        onCompleted?.();
      }
    }
  }, [store.isRunning, store.items, onCompleted]);

  const items: RunnerItem[] = store.items.map((item: QueueItem) => ({
    task: {
      id: item.taskId,
      title: item.title,
      type: item.taskType,
      projectId: item.projectId,
      projectName: item.projectName,
    },
    status: item.status === 'pending' 
      ? 'queued' 
      : item.status === 'completed' 
        ? 'done' 
        : item.status === 'running' 
          ? 'running' 
          : 'failed',
    error: item.error,
    liveSteps: [],
    result: item.result,
  }));

  logger.exit('useQueueRunner');

  return {
    items,
    isRunning: store.isRunning,
    isPaused: store.isPaused,
    isVisible: store.isVisible,

    enqueue: (newItems: QueueItem[]) => {
      logger.entry('enqueue', { count: newItems.length });
      console.log('[useQueueRunner] enqueue called with', newItems.length, 'items');
      const itemsWithStatus = newItems.map((i) => ({ ...i, status: ('pending' as const) }));
      console.log('[useQueueRunner] calling store.enqueue...');
      store.enqueue(itemsWithStatus);
      console.log('[useQueueRunner] store.enqueue returned');
      logger.exit('enqueue');
    },

    enqueueNext: (newItems: QueueItem[]) => {
      logger.entry('enqueueNext', { count: newItems.length });
      const itemsWithStatus = newItems.map((i) => ({ ...i, status: ('pending' as const) }));
      store.enqueueNext(itemsWithStatus);
      logger.exit('enqueueNext');
    },

    removeItem: (taskId: string) => {
      logger.entry('removeItem', { taskId });
      store.removeItem(taskId);
      logger.exit('removeItem');
    },

    pause: () => {
      logger.entry('pause');
      store.pause();
      logger.exit('pause');
    },

    resume: () => {
      logger.entry('resume');
      store.resume();
      logger.exit('resume');
    },

    close: () => {
      logger.entry('close');
      store.close();
      logger.exit('close');
    },
  };
}
