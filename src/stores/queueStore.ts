/**
 * Global Task Queue Store
 * 
 * Manages a cross-project queue of tasks for serial execution.
 */

import { create } from 'zustand';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { executeQueue, markTasksQueued, markTasksTodo, pauseQueue, resumeQueue, clearCompletedQueueItems } from '@/lib/tauri';
import { createLogger, LogTarget } from '@/lib/logging';
import type { StepProgress, QueueItem, ExecutionResult } from '@/lib/types';

const logger = createLogger(LogTarget.QUEUE);

export interface QueueProgressEvent {
  eventType: 'started' | 'step_progress' | 'completed' | 'failed';
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
    follow_up_tasks?: { id: string; task_type: string; title: string; status: string; execution_mode: string; priority: string }[];
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
  items: QueueItem[];
  isRunning: boolean;
  isPaused: boolean;
  isVisible: boolean;
  unlisteners: UnlistenFn[];
  
  enqueue: (items: QueueItem[]) => void;
  enqueueNext: (items: QueueItem[]) => void;
  removeItem: (taskId: string) => void;
  clearCompleted: () => Promise<void>;
  close: () => void;
  start: () => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  show: () => void;
  
  setupEventListeners: () => Promise<void>;
  cleanupEventListeners: () => void;
  onTaskStarted: (event: QueueProgressEvent) => void;
  onTaskCompleted: (event: QueueProgressEvent) => void;
  onTaskFailed: (event: QueueProgressEvent) => void;
  onFollowUpCreated: (event: FollowUpCreatedEvent) => void;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  items: [],
  isRunning: false,
  isPaused: false,
  isVisible: false,
  unlisteners: [],

  enqueue: (newItems: QueueItem[]) => {
    logger.entry('enqueue', { count: newItems.length });
    console.log('[QueueStore] enqueue called with', newItems.length, 'items');
    
    if (newItems.length === 0) {
      logger.debug('enqueue - no items to add');
      console.log('[QueueStore] enqueue - no items, returning');
      return;
    }
    
    let addedTaskIds: string[] = [];
    
    set((state: QueueState) => {
      const deduped = newItems.filter(
        (na: QueueItem) => !state.items.some((p: QueueItem) => p.taskId === na.taskId)
      );
      
      logger.debug('enqueue - deduped', { toAdd: deduped.length, existing: state.items.length });
      console.log('[QueueStore] enqueue - deduped:', deduped.length, 'items to add');
      
      if (deduped.length === 0) return state;
      
      // Track which tasks were actually added
      addedTaskIds = deduped.map(item => item.taskId);
      
      // Ensure all items have pending status
      const itemsWithStatus = deduped.map(item => ({
        ...item,
        status: item.status || 'pending' as const,
      }));
      
      return { items: [...state.items, ...itemsWithStatus], isVisible: true };
    });
    
    // Mark added tasks as queued in the database
    if (addedTaskIds.length > 0) {
      console.log('[QueueStore] enqueue - marking tasks as queued:', addedTaskIds);
      markTasksQueued(addedTaskIds).catch(err => {
        console.error('[QueueStore] enqueue - failed to mark tasks as queued:', err);
      });
    }
    
    const { isRunning, start } = get();
    console.log('[QueueStore] enqueue - isRunning:', isRunning);
    if (!isRunning) {
      logger.info('enqueue - auto-starting queue');
      console.log('[QueueStore] enqueue - auto-starting queue...');
      void start();
      console.log('[QueueStore] enqueue - start() called');
    } else {
      console.log('[QueueStore] enqueue - already running, not starting');
    }
    
    logger.exit('enqueue');
  },

  enqueueNext: (newItems: QueueItem[]) => {
    logger.entry('enqueueNext', { count: newItems.length });
    
    if (newItems.length === 0) return;
    
    set((state: QueueState) => {
      const deduped = newItems.filter(
        (na: QueueItem) => !state.items.some((p: QueueItem) => p.taskId === na.taskId)
      );
      
      if (deduped.length === 0) return state;
      
      // Ensure all items have pending status
      const itemsWithStatus = deduped.map(item => ({
        ...item,
        status: item.status || 'pending' as const,
      }));
      
      const firstPendingIndex = state.items.findIndex((i: QueueItem) => i.status === 'pending');
      const insertAt = firstPendingIndex >= 0 ? firstPendingIndex : state.items.length;
      
      const newItemsList = [...state.items];
      newItemsList.splice(insertAt, 0, ...itemsWithStatus);
      
      logger.debug('enqueueNext - inserted at', { index: insertAt });
      return { items: newItemsList, isVisible: true };
    });
    
    const { isRunning, start } = get();
    if (!isRunning) void start();
    
    logger.exit('enqueueNext');
  },

  removeItem: (taskId: string) => {
    logger.entry('removeItem', { taskId });
    
    const wasPending = get().items.find(i => i.taskId === taskId)?.status === 'pending';
    
    set((state: QueueState) => ({
      items: state.items.filter((i: QueueItem) => !(i.taskId === taskId && i.status === 'pending')),
    }));
    
    // Reset task status back to todo in the database
    if (wasPending) {
      console.log('[QueueStore] removeItem - resetting task to todo:', taskId);
      markTasksTodo([taskId]).catch(err => {
        console.error('[QueueStore] removeItem - failed to reset task status:', err);
      });
    }
    
    logger.exit('removeItem');
  },

  clearCompleted: async () => {
    logger.entry('clearCompleted');
    try {
      await clearCompletedQueueItems();
      set((state: QueueState) => ({
        items: state.items.filter((i: QueueItem) => i.status === 'pending' || i.status === 'running'),
      }));
      logger.info('clearCompleted - success');
    } catch (error) {
      logger.error('clearCompleted - failed', { error: String(error) });
    }
    logger.exit('clearCompleted');
  },

  close: () => {
    logger.entry('close');
    const { isRunning, cleanupEventListeners } = get();
    if (isRunning) {
      logger.warn('close - cannot close while running');
      return;
    }
    cleanupEventListeners();
    set({ items: [], isVisible: false, isPaused: false });
    logger.exit('close');
  },

  show: () => {
    set({ isVisible: true });
  },

  start: async () => {
    logger.entry('start', { itemCount: get().items.length, isRunning: get().isRunning });
    console.log('[QueueStore] start() called');
    
    const { items, isRunning, setupEventListeners } = get();
    console.log('[QueueStore] start - items:', items.length, 'isRunning:', isRunning);
    
    if (items.length === 0 || isRunning) {
      logger.debug('start - early return', { reason: items.length === 0 ? 'no items' : 'already running' });
      console.log('[QueueStore] start - early return, reason:', items.length === 0 ? 'no items' : 'already running');
      return;
    }
    
    const pendingItems = items.filter((i: QueueItem) => i.status === 'pending');
    logger.info('start - pending items to execute', { count: pendingItems.length });
    console.log('[QueueStore] start - pending items:', pendingItems.length);
    
    if (pendingItems.length === 0) {
      logger.debug('start - no pending items');
      console.log('[QueueStore] start - no pending items, returning');
      return;
    }
    
    console.log('[QueueStore] start - setting up event listeners...');

    // Set isRunning BEFORE await to close the race window where concurrent
    // start() calls could both pass the isRunning check.
    set({ isRunning: true, isPaused: false });

    await setupEventListeners();
    console.log('[QueueStore] start - event listeners ready');
    console.log('[QueueStore] start - isRunning set to true');
    
    try {
      logger.info('start - calling executeQueue()');
      console.log('[QueueStore] start - calling executeQueue() with', pendingItems.length, 'items');
      await executeQueue(pendingItems);
      console.log('[QueueStore] start - executeQueue() returned successfully');
      logger.info('start - executeQueue() returned');
    } catch (error) {
      console.error('[QueueStore] start - executeQueue() failed:', error);
      logger.error('start - executeQueue() failed', { error: String(error) });
      set({ isRunning: false });
    }
    
    logger.exit('start');
  },

  pause: async () => {
    logger.entry('pause');
    try {
      await pauseQueue();
      set({ isPaused: true });
      logger.info('pause - success');
    } catch (error) {
      logger.error('pause - failed', { error: String(error) });
    }
    logger.exit('pause');
  },

  resume: async () => {
    logger.entry('resume');
    try {
      await resumeQueue();
      set({ isPaused: false });
      logger.info('resume - success');
    } catch (error) {
      logger.error('resume - failed', { error: String(error) });
    }
    logger.exit('resume');
  },

  setupEventListeners: async () => {
    logger.entry('setupEventListeners');
    console.log('[QueueStore] Setting up event listeners...');
    const { onTaskStarted, onTaskCompleted, onTaskFailed, onFollowUpCreated, cleanupEventListeners } = get();
    
    cleanupEventListeners();
    const unlisteners: UnlistenFn[] = [];
    
    logger.debug('setupEventListeners - registering listeners');
    console.log('[QueueStore] Registering Tauri event listeners...');
    
    const unlistenStarted = await listen<QueueProgressEvent>('queue:task-started', (event) => {
      // event.payload is the QueueProgressEvent (Tauri wraps it)
      const payload = event.payload;
      console.log('[QueueStore] Received queue:task-started', payload);
      logger.event('queue:task-started', { taskId: payload.taskId });
      onTaskStarted(payload);
    });
    unlisteners.push(unlistenStarted);
    
    const unlistenCompleted = await listen<QueueProgressEvent>('queue:task-completed', (event) => {
      const payload = event.payload;
      console.log('[QueueStore] Received queue:task-completed', payload);
      logger.event('queue:task-completed', { taskId: payload.taskId, type: payload.eventType });
      if (payload.eventType === 'completed') {
        onTaskCompleted(payload);
      } else {
        onTaskFailed(payload);
      }
    });
    unlisteners.push(unlistenCompleted);
    
    const unlistenFailed = await listen<QueueProgressEvent>('queue:task-failed', (event) => {
      const payload = event.payload;
      logger.event('queue:task-failed', { taskId: payload.taskId, error: payload.payload?.error });
      onTaskFailed(payload);
    });
    unlisteners.push(unlistenFailed);
    
    const unlistenFollowUp = await listen<FollowUpCreatedEvent>('queue:follow-up-created', (event) => {
      const payload = event.payload;
      logger.event('queue:follow-up-created', { taskId: payload.taskId, type: payload.taskType });
      onFollowUpCreated(payload);
    });
    unlisteners.push(unlistenFollowUp);
    
    const unlistenFinished = await listen('queue:finished', () => {
      logger.event('queue:finished');
      console.log('[QueueStore] Queue finished event received');
      cleanupEventListeners();
      set({ isRunning: false });
      
      // Check if there are any pending items that were added while queue was running
      const { items, start } = get();
      const pendingItems = items.filter((i: QueueItem) => i.status === 'pending');
      if (pendingItems.length > 0) {
        console.log('[QueueStore] Found', pendingItems.length, 'pending items after queue finished, restarting...');
        logger.info('queue:finished - restarting for pending items', { count: pendingItems.length });
        void start();
      }
    });
    unlisteners.push(unlistenFinished);
    
    set({ unlisteners });
    logger.info('setupEventListeners - all listeners registered', { count: unlisteners.length });
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

  onTaskStarted: (event: QueueProgressEvent) => {
    console.log('[QueueStore] Task started:', event.taskId);
    logger.entry('onTaskStarted', { taskId: event.taskId, title: event.payload.title });
    set((state: QueueState) => {
      const idx = state.items.findIndex((i: QueueItem) => i.taskId === event.taskId);
      if (idx === -1 || state.items[idx].status === 'running') {
        return state;
      }
      const next = [...state.items];
      next[idx] = { ...next[idx], status: 'running' as const };
      return { items: next };
    });
    logger.stateChange('task status', 'pending/queued', 'running');
    logger.exit('onTaskStarted');
  },

  onTaskCompleted: (event: QueueProgressEvent) => {
    logger.entry('onTaskCompleted', { taskId: event.taskId });
    console.log('[QueueStore] Task completed:', event.taskId);
    set((state: QueueState) => {
      const idx = state.items.findIndex((i: QueueItem) => i.taskId === event.taskId);
      if (idx === -1) {
        return state;
      }
      const current = state.items[idx];
      const newResult = event.payload?.follow_up_tasks
        ? {
            follow_up_tasks: event.payload.follow_up_tasks,
            started_at: event.payload.started_at,
            finished_at: event.payload.finished_at,
          } as unknown as ExecutionResult
        : undefined;
      // Idempotent: skip if already completed with same result
      if (
        current.status === 'completed' &&
        JSON.stringify(current.result) === JSON.stringify(newResult)
      ) {
        return state;
      }
      const next = [...state.items];
      next[idx] = { ...current, status: 'completed' as const, result: newResult };
      return { items: next };
    });
    logger.stateChange('task status', 'running', 'completed');
    logger.exit('onTaskCompleted');
  },

  onTaskFailed: (event: QueueProgressEvent) => {
    logger.error('onTaskFailed', { taskId: event.taskId, error: event.payload.error });
    set((state: QueueState) => {
      const idx = state.items.findIndex((i: QueueItem) => i.taskId === event.taskId);
      if (idx === -1 || state.items[idx].status === 'failed') {
        return { isPaused: true };
      }
      const next = [...state.items];
      next[idx] = { ...next[idx], status: 'failed' as const, error: event.payload.error };
      return { items: next, isPaused: true };
    });
    logger.stateChange('task status', 'running', 'failed');
    logger.stateChange('queue paused', false, true);
  },

  onFollowUpCreated: (event: FollowUpCreatedEvent) => {
    logger.entry('onFollowUpCreated', { taskId: event.taskId, mode: event.executionMode });
    
    if (event.executionMode !== 'automatic' && event.executionMode !== 'batchable') {
      logger.debug('onFollowUpCreated - skipping (not auto-queueable)', { mode: event.executionMode });
      return;
    }
    
    set((state: QueueState) => {
      if (state.items.some((i: QueueItem) => i.taskId === event.taskId)) {
        logger.debug('onFollowUpCreated - already in queue');
        return state;
      }
      
      const projectItem = state.items.find((i: QueueItem) => i.projectId === event.projectId);
      const projectName = projectItem?.projectName || 'Auto-created';
      
      logger.info('onFollowUpCreated - adding follow-up to queue');
      return {
        items: [...state.items, {
          taskId: event.taskId,
          projectId: event.projectId,
          projectName,
          title: event.title,
          taskType: event.taskType,
          status: 'pending' as const,
        }],
      };
    });
    logger.exit('onFollowUpCreated');
  },
}));

// HMR cleanup: remove orphaned Tauri event listeners when this module is hot-replaced
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    useQueueStore.getState().cleanupEventListeners();
  });
}
