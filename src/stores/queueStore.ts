/**
 * Global Task Queue Store
 * 
 * Manages a cross-project queue of tasks for serial execution.
 */

import { create } from 'zustand';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { executeQueue, pauseQueue, resumeQueue, clearCompletedQueueItems } from '@/lib/tauri';
import { createLogger, LogTarget } from '@/lib/logging';
import type { StepProgress, QueueItem } from '@/lib/types';

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
    
    if (newItems.length === 0) {
      logger.debug('enqueue - no items to add');
      return;
    }
    
    set((state: QueueState) => {
      const deduped = newItems.filter(
        (na: QueueItem) => !state.items.some((p: QueueItem) => p.taskId === na.taskId)
      );
      
      logger.debug('enqueue - deduped', { toAdd: deduped.length, existing: state.items.length });
      
      if (deduped.length === 0) return state;
      
      // Ensure all items have pending status
      const itemsWithStatus = deduped.map(item => ({
        ...item,
        status: item.status || 'pending' as const,
      }));
      
      return { items: [...state.items, ...itemsWithStatus], isVisible: true };
    });
    
    const { isRunning, start } = get();
    if (!isRunning) {
      logger.info('enqueue - auto-starting queue');
      void start();
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
    set((state: QueueState) => ({
      items: state.items.filter((i: QueueItem) => !(i.taskId === taskId && i.status === 'pending')),
    }));
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
    
    const { items, isRunning, setupEventListeners } = get();
    
    if (items.length === 0 || isRunning) {
      logger.debug('start - early return', { reason: items.length === 0 ? 'no items' : 'already running' });
      return;
    }
    
    const pendingItems = items.filter((i: QueueItem) => i.status === 'pending');
    logger.info('start - pending items to execute', { count: pendingItems.length });
    
    if (pendingItems.length === 0) {
      logger.debug('start - no pending items');
      return;
    }
    
    await setupEventListeners();
    set({ isRunning: true, isPaused: false });
    
    try {
      logger.info('start - calling executeQueue()');
      await executeQueue(pendingItems);
      logger.info('start - executeQueue() returned');
    } catch (error) {
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
    const { onTaskStarted, onTaskCompleted, onTaskFailed, onFollowUpCreated, cleanupEventListeners } = get();
    
    cleanupEventListeners();
    const unlisteners: UnlistenFn[] = [];
    
    logger.debug('setupEventListeners - registering listeners');
    
    const unlistenStarted = await listen<QueueProgressEvent>('queue:task-started', (event) => {
      logger.event('queue:task-started', { taskId: event.payload.taskId });
      onTaskStarted(event.payload);
    });
    unlisteners.push(unlistenStarted);
    
    const unlistenCompleted = await listen<QueueProgressEvent>('queue:task-completed', (event) => {
      logger.event('queue:task-completed', { taskId: event.payload.taskId, type: event.payload.eventType });
      if (event.payload.eventType === 'completed') {
        onTaskCompleted(event.payload);
      } else {
        onTaskFailed(event.payload);
      }
    });
    unlisteners.push(unlistenCompleted);
    
    const unlistenFailed = await listen<QueueProgressEvent>('queue:task-failed', (event) => {
      logger.event('queue:task-failed', { taskId: event.payload.taskId, error: event.payload.payload.error });
      onTaskFailed(event.payload);
    });
    unlisteners.push(unlistenFailed);
    
    const unlistenFollowUp = await listen<FollowUpCreatedEvent>('queue:follow-up-created', (event) => {
      logger.event('queue:follow-up-created', { taskId: event.payload.taskId, type: event.payload.taskType });
      onFollowUpCreated(event.payload);
    });
    unlisteners.push(unlistenFollowUp);
    
    const unlistenFinished = await listen('queue:finished', () => {
      logger.event('queue:finished');
      cleanupEventListeners();
      set({ isRunning: false });
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
    logger.entry('onTaskStarted', { taskId: event.taskId, title: event.payload.title });
    set((state: QueueState) => ({
      items: state.items.map((i: QueueItem) =>
        i.taskId === event.taskId ? { ...i, status: 'running' as const } : i
      ),
    }));
    logger.stateChange('task status', 'pending/queued', 'running');
    logger.exit('onTaskStarted');
  },

  onTaskCompleted: (event: QueueProgressEvent) => {
    logger.entry('onTaskCompleted', { taskId: event.taskId });
    set((state: QueueState) => ({
      items: state.items.map((i: QueueItem) =>
        i.taskId === event.taskId ? { ...i, status: 'completed' as const } : i
      ),
    }));
    logger.stateChange('task status', 'running', 'completed');
    logger.exit('onTaskCompleted');
  },

  onTaskFailed: (event: QueueProgressEvent) => {
    logger.error('onTaskFailed', { taskId: event.taskId, error: event.payload.error });
    set((state: QueueState) => ({
      items: state.items.map((i: QueueItem) =>
        i.taskId === event.taskId ? { ...i, status: 'failed' as const, error: event.payload.error } : i
      ),
      isPaused: true,
    }));
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
