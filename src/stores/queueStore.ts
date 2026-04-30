/**
 * Global Task Queue Store — Backend Projection Cache
 *
 * The queue is owned by the backend (engine/queue.rs). This store is a
 * read-through cache that syncs via getQueueSnapshot and Tauri events.
 * Only UI preferences (expanded rows) live here permanently.
 */

import { create } from 'zustand';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  getQueueSnapshot,
  enqueueTasks,
  removeQueueItem,
  pauseQueue,
  resumeQueue,
  clearCompletedQueueItems,
  dismissQueue,
} from '@/lib/tauri';
import { createLogger, LogTarget } from '@/lib/logging';
import type { StepProgress, QueueSnapshot, EnqueueItem, EnqueueMode } from '@/lib/types';

const logger = createLogger(LogTarget.QUEUE);

export interface QueueProgressEvent {
  eventType: 'started' | 'step_progress' | 'completed' | 'failed' | 'skipped';
  taskId: string;
  projectId: string;
  payload: {
    index?: number;
    total?: number;
    title?: string;
    taskType?: string;
    message?: string;
    steps?: StepProgress[];
    followUpCount?: number;
    error?: string;
    retryable?: boolean;
    reason?: string;
    follow_up_tasks?: { id: string; task_type: string; title: string; status: string; run_policy: string; priority: string }[];
    started_at?: string;
    finished_at?: string;
  };
}

export interface FollowUpCreatedEvent {
  taskId: string;
  projectId: string;
  title: string;
  taskType: string;
  executionMode: string;
}

interface QueueState {
  snapshot: QueueSnapshot | null;
  isVisible: boolean;
  unlisteners: UnlistenFn[];

  // Optimistic state for immediate UI feedback
  isStarting: boolean;

  // UI preferences (persisted in this store)
  expandedTaskIds: Set<string>;

  // Actions
  sync: () => Promise<void>;
  enqueue: (items: EnqueueItem[], mode?: EnqueueMode) => Promise<void>;
  enqueueNext: (items: EnqueueItem[]) => Promise<void>;
  removeItem: (taskId: string) => Promise<void>;
  clearCompleted: () => Promise<void>;
  close: () => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  show: () => void;
  toggleExpanded: (taskId: string) => void;

  setupEventListeners: () => Promise<void>;
  cleanupEventListeners: () => void;
  applySnapshot: (snapshot: QueueSnapshot) => void;
}

function snapshotToVisible(snapshot: QueueSnapshot | null): boolean {
  if (!snapshot) return false;
  if (!snapshot.run) return false;
  const runStatus = snapshot.run.status;
  // Show if run is active or has items
  if (runStatus === 'running' || runStatus === 'paused' || runStatus === 'idle') {
    return snapshot.items.length > 0;
  }
  // Show finished runs too until dismissed
  if ((runStatus === 'finished' || runStatus === 'failed') && snapshot.items.length > 0) {
    return true;
  }
  return false;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  snapshot: null,
  isVisible: false,
  unlisteners: [],
  isStarting: false,
  expandedTaskIds: new Set(),

  applySnapshot: (snapshot: QueueSnapshot) => {
    logger.debug('applySnapshot', { itemCount: snapshot.items.length, runStatus: snapshot.run?.status });
    const isVisible = snapshotToVisible(snapshot);
    // Clear optimistic starting state once backend confirms running or a terminal state.
    // Keep it only while the run is idle (runner still spawning) so we don't flicker.
    const runStatus = snapshot.run?.status;
    const shouldKeepStarting = runStatus === 'idle' && get().isStarting;
    set({ snapshot, isVisible, isStarting: shouldKeepStarting });
  },

  sync: async () => {
    logger.entry('sync');
    try {
      const snapshot = await getQueueSnapshot();
      get().applySnapshot(snapshot);
      logger.info('sync - success', { itemCount: snapshot.items.length });
    } catch (error) {
      logger.error('sync - failed', { error: String(error) });
    }
    logger.exit('sync');
  },

  enqueue: async (items: EnqueueItem[], mode: EnqueueMode = 'append') => {
    logger.entry('enqueue', { count: items.length, mode });
    set({ isStarting: true });
    try {
      const snapshot = await enqueueTasks(items, mode);
      get().applySnapshot(snapshot);
      logger.info('enqueue - success');
    } catch (error) {
      logger.error('enqueue - failed', { error: String(error) });
      set({ isStarting: false });
    }
    logger.exit('enqueue');
  },

  enqueueNext: async (items: EnqueueItem[]) => {
    logger.entry('enqueueNext', { count: items.length });
    set({ isStarting: true });
    try {
      const snapshot = await enqueueTasks(items, 'next');
      get().applySnapshot(snapshot);
      logger.info('enqueueNext - success');
    } catch (error) {
      logger.error('enqueueNext - failed', { error: String(error) });
      set({ isStarting: false });
    }
    logger.exit('enqueueNext');
  },

  removeItem: async (taskId: string) => {
    logger.entry('removeItem', { taskId });
    try {
      const snapshot = await removeQueueItem(taskId);
      get().applySnapshot(snapshot);
      logger.info('removeItem - success');
    } catch (error) {
      logger.error('removeItem - failed', { error: String(error) });
    }
    logger.exit('removeItem');
  },

  clearCompleted: async () => {
    logger.entry('clearCompleted');
    try {
      const snapshot = await clearCompletedQueueItems();
      get().applySnapshot(snapshot);
      logger.info('clearCompleted - success');
    } catch (error) {
      logger.error('clearCompleted - failed', { error: String(error) });
    }
    logger.exit('clearCompleted');
  },

  close: async () => {
    logger.entry('close');
    try {
      await dismissQueue();
      set({ snapshot: null, isVisible: false, expandedTaskIds: new Set() });
      get().cleanupEventListeners();
      logger.info('close - success');
    } catch (error) {
      logger.error('close - failed', { error: String(error) });
    }
    logger.exit('close');
  },

  show: () => {
    set({ isVisible: true });
  },

  pause: async () => {
    logger.entry('pause');
    try {
      const snapshot = await pauseQueue();
      get().applySnapshot(snapshot);
      logger.info('pause - success');
    } catch (error) {
      logger.error('pause - failed', { error: String(error) });
    }
    logger.exit('pause');
  },

  resume: async () => {
    logger.entry('resume');
    set({ isStarting: true });
    try {
      const snapshot = await resumeQueue();
      get().applySnapshot(snapshot);
      logger.info('resume - success');
    } catch (error) {
      logger.error('resume - failed', { error: String(error) });
      set({ isStarting: false });
    }
    logger.exit('resume');
  },

  toggleExpanded: (taskId: string) => {
    set((state) => {
      const next = new Set(state.expandedTaskIds);
      if (next.has(taskId)) {
        next.delete(taskId);
      } else {
        next.add(taskId);
      }
      return { expandedTaskIds: next };
    });
  },

  setupEventListeners: async () => {
    logger.entry('setupEventListeners');
    const { cleanupEventListeners, sync } = get();
    cleanupEventListeners();
    const unlisteners: UnlistenFn[] = [];

    const unlistenStarted = await listen<QueueProgressEvent>('queue:task-started', () => {
      sync();
    });
    unlisteners.push(unlistenStarted);

    const unlistenCompleted = await listen<QueueProgressEvent>('queue:task-completed', () => {
      sync();
    });
    unlisteners.push(unlistenCompleted);

    const unlistenFailed = await listen<QueueProgressEvent>('queue:task-failed', () => {
      sync();
    });
    unlisteners.push(unlistenFailed);

    const unlistenSkipped = await listen<QueueProgressEvent>('queue:task-skipped', () => {
      sync();
    });
    unlisteners.push(unlistenSkipped);

    const unlistenFollowUp = await listen<FollowUpCreatedEvent>('queue:follow-up-created', () => {
      sync();
    });
    unlisteners.push(unlistenFollowUp);

    const unlistenFinished = await listen<{ status?: string; reason?: string }>('queue:finished', () => {
      sync();
    });
    unlisteners.push(unlistenFinished);

    set({ unlisteners });
    logger.info('setupEventListeners - registered', { count: unlisteners.length });
    logger.exit('setupEventListeners');
  },

  cleanupEventListeners: () => {
    const { unlisteners } = get();
    if (unlisteners.length > 0) {
      logger.info('cleanupEventListeners - cleaning up', { count: unlisteners.length });
      unlisteners.forEach((unlisten: UnlistenFn) => unlisten());
      set({ unlisteners: [] });
    }
  },
}));

// HMR cleanup: remove orphaned Tauri event listeners when this module is hot-replaced
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    useQueueStore.getState().cleanupEventListeners();
  });
}
