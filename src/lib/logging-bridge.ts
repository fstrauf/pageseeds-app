/**
 * Frontend-to-Backend Logging Bridge
 * 
 * Captures all frontend logs and sends them to the Rust backend for persistent storage.
 * This enables full observability of the application for debugging and agentic analysis.
 */

import { invoke } from '@tauri-apps/api/core';
import { nanoid } from 'nanoid';

// Generate a session ID for grouping logs
const SESSION_ID = nanoid();

// Log buffer for batching
const LOG_BUFFER: LogEntry[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

export interface LogEntry {
  timestamp: string;
  level: 'debug' | 'info' | 'warn' | 'error';
  component: string;
  message: string;
  metadata?: Record<string, unknown>;
  session_id: string;
}

interface LogEntryInput {
  timestamp: string;
  level: string;
  component: string;
  message: string;
  metadata?: Record<string, unknown>;
  session_id: string;
}

/**
 * Submit a single log entry to the backend
 */
export async function submitLog(entry: LogEntryInput): Promise<number | null> {
  try {
    const id = await invoke<number>('submit_log', { entry });
    return id;
  } catch (error) {
    console.error('Failed to submit log:', error);
    return null;
  }
}

/**
 * Submit multiple logs in batch
 */
export async function submitLogsBatch(entries: LogEntryInput[]): Promise<number[]> {
  if (entries.length === 0) return [];
  
  try {
    const ids = await invoke<number[]>('submit_logs_batch', { entries });
    return ids;
  } catch (error) {
    console.error('Failed to submit logs batch:', error);
    return [];
  }
}

/**
 * Queue a log entry for batch submission
 */
export function queueLog(
  level: LogEntry['level'],
  component: string,
  message: string,
  metadata?: Record<string, unknown>
): void {
  const entry: LogEntry = {
    timestamp: new Date().toISOString(),
    level,
    component,
    message,
    metadata,
    session_id: SESSION_ID,
  };
  
  LOG_BUFFER.push(entry);
  
  // Schedule flush if not already scheduled
  if (!flushTimer) {
    flushTimer = setTimeout(() => {
      flushLogs();
    }, 500); // Batch logs every 500ms
  }
  
  // Also log to console for immediate visibility
  const consoleMessage = `[${component}] ${message}`;
  switch (level) {
    case 'debug':
      console.debug(consoleMessage, metadata || '');
      break;
    case 'info':
      console.info(consoleMessage, metadata || '');
      break;
    case 'warn':
      console.warn(consoleMessage, metadata || '');
      break;
    case 'error':
      console.error(consoleMessage, metadata || '');
      break;
  }
}

/**
 * Flush all queued logs to the backend
 */
export async function flushLogs(): Promise<void> {
  if (flushTimer) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  
  if (LOG_BUFFER.length === 0) return;
  
  const logsToSend = [...LOG_BUFFER];
  LOG_BUFFER.length = 0; // Clear buffer
  
  await submitLogsBatch(logsToSend);
}

/**
 * Create a logger instance for a specific component
 */
export function createLogger(component: string) {
  return {
    debug: (message: string, metadata?: Record<string, unknown>) => {
      queueLog('debug', component, message, metadata);
    },
    info: (message: string, metadata?: Record<string, unknown>) => {
      queueLog('info', component, message, metadata);
    },
    warn: (message: string, metadata?: Record<string, unknown>) => {
      queueLog('warn', component, message, metadata);
    },
    error: (message: string, metadata?: Record<string, unknown>) => {
      queueLog('error', component, message, metadata);
    },
    // Legacy API compatibility
    entry: (method: string, metadata?: Record<string, unknown>) => {
      queueLog('debug', component, `→ ${method}`, metadata);
    },
    exit: (method: string) => {
      queueLog('debug', component, `← ${method}`);
    },
    stateChange: (what: string, from: unknown, to: unknown) => {
      queueLog('debug', component, `State: ${what}`, { from, to });
    },
    event: (event: string, metadata?: Record<string, unknown>) => {
      queueLog('info', component, `Event: ${event}`, metadata);
    },
  };
}

/**
 * Query logs from the backend
 */
export async function queryLogs(filters: {
  level?: string;
  source?: string;
  component?: string;
  sessionId?: string;
  searchQuery?: string;
}, limit = 100, offset = 0): Promise<Array<{
  id?: number;
  timestamp: string;
  level: string;
  source: string;
  component: string;
  message: string;
  metadata?: Record<string, unknown>;
  session_id: string;
}>> {
  try {
    const logs = await invoke<Array<{
      id?: number;
      timestamp: string;
      level: string;
      source: string;
      component: string;
      message: string;
      metadata?: Record<string, unknown>;
      session_id: string;
    }>>('query_logs', { 
      filters: {
        level: filters.level,
        source: filters.source,
        component: filters.component,
        session_id: filters.sessionId,
        search_query: filters.searchQuery,
      },
      limit,
      offset,
    });
    return logs;
  } catch (error) {
    console.error('Failed to query logs:', error);
    return [];
  }
}

/**
 * Get recent logs from memory (fast)
 */
export async function getRecentLogs(limit = 100): Promise<Array<{
  id?: number;
  timestamp: string;
  level: string;
  source: string;
  component: string;
  message: string;
  metadata?: Record<string, unknown>;
  session_id: string;
}>> {
  try {
    const logs = await invoke<Array<{
      id?: number;
      timestamp: string;
      level: string;
      source: string;
      component: string;
      message: string;
      metadata?: Record<string, unknown>;
      session_id: string;
    }>>('get_recent_logs', { limit });
    return logs;
  } catch (error) {
    console.error('Failed to get recent logs:', error);
    return [];
  }
}

/**
 * Get log statistics
 */
export async function getLogStats(): Promise<{
  total_count: number;
  error_count: number;
  warn_count: number;
  info_count: number;
  debug_count: number;
  frontend_count: number;
  backend_count: number;
  agent_count: number;
} | null> {
  try {
    const stats = await invoke<{
      total_count: number;
      error_count: number;
      warn_count: number;
      info_count: number;
      debug_count: number;
      frontend_count: number;
      backend_count: number;
      agent_count: number;
    }>('get_log_stats');
    return stats;
  } catch (error) {
    console.error('Failed to get log stats:', error);
    return null;
  }
}

/**
 * Export logs to JSON
 */
export async function exportLogs(filters?: {
  level?: string;
  source?: string;
  component?: string;
  sessionId?: string;
  searchQuery?: string;
}): Promise<string | null> {
  try {
    const json = await invoke<string>('export_logs', {
      filters: filters || {},
    });
    return json;
  } catch (error) {
    console.error('Failed to export logs:', error);
    return null;
  }
}

/**
 * Clear old logs
 */
export async function clearOldLogs(daysToKeep: number): Promise<number> {
  try {
    const count = await invoke<number>('clear_old_logs', { daysToKeep });
    return count;
  } catch (error) {
    console.error('Failed to clear old logs:', error);
    return 0;
  }
}

// Auto-flush on page unload
window.addEventListener('beforeunload', () => {
  flushLogs();
});

// Export session ID for reference
export { SESSION_ID };
