/**
 * PageSeeds Logging Module
 * 
 * Provides consistent, AI-friendly logging for the frontend.
 * All logs are sent to the Rust backend for persistent storage in SQLite.
 * This enables full observability for debugging and agentic analysis.
 * 
 * Log Location:
 * - macOS: ~/Library/Application Support/com.pageseeds.app/pageseeds.db (table: app_logs)
 * - Windows: %APPDATA%/PageSeeds/pageseeds.db
 * - Linux: ~/.local/share/PageSeeds/pageseeds.db
 */

import { queueLog, flushLogs, createLogger as createBridgeLogger } from './logging-bridge';

// Re-export the bridge functions
export { flushLogs, queryLogs, getRecentLogs, getLogStats, exportLogs, clearOldLogs } from './logging-bridge';
export type { LogEntry } from './logging-bridge';

// Log targets for categorization
export const LogTarget = {
  QUEUE: 'queue',
  TASKS: 'tasks',
  UI: 'ui',
  API: 'api',
  RENDER: 'render',
  STATE: 'state',
  EVENT: 'event',
  AGENT: 'agent',
  SYSTEM: 'system',
} as const;

type LogTargetType = typeof LogTarget[keyof typeof LogTarget];

// Map LogTarget to component name
function getComponent(target: LogTargetType): string {
  return `frontend::${target}`;
}

/**
 * Log at DEBUG level (detailed debugging)
 */
export function logDebug(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  queueLog('debug', getComponent(target), message, context);
}

/**
 * Log at INFO level (general information)
 */
export function logInfo(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  queueLog('info', getComponent(target), message, context);
}

/**
 * Log at WARN level (warnings)
 */
export function logWarn(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  queueLog('warn', getComponent(target), message, context);
}

/**
 * Log at ERROR level (errors)
 */
export function logError(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  queueLog('error', getComponent(target), message, context);
}

/**
 * Log a function entry with arguments
 */
export function logEntry<T extends Record<string, unknown>>(
  target: LogTargetType, 
  functionName: string, 
  args?: T
): void {
  queueLog('debug', getComponent(target), `→ ${functionName}()`, args);
}

/**
 * Log a function exit with result
 */
export function logExit(
  target: LogTargetType, 
  functionName: string, 
  result?: unknown
): void {
  const context = result !== undefined ? { result } : undefined;
  queueLog('debug', getComponent(target), `← ${functionName}()`, context);
}

/**
 * Log a state change
 */
export function logStateChange<T>(
  target: LogTargetType,
  stateName: string,
  oldValue: T,
  newValue: T
): void {
  queueLog('debug', getComponent(target), `State change: ${stateName}`, {
    old: oldValue,
    new: newValue,
  });
}

/**
 * Log an event receipt
 */
export function logEvent<T>(
  target: LogTargetType,
  eventName: string,
  payload?: T
): void {
  queueLog('debug', getComponent(target), `Event received: ${eventName}`, payload ? { payload } : undefined);
}

/**
 * Create a scoped logger for a specific component/module
 */
export function createLogger(target: LogTargetType) {
  const component = getComponent(target);
  const logger = createBridgeLogger(component);
  
  return {
    trace: (msg: string, ctx?: Record<string, unknown>) => {
      // trace level maps to debug for our system
      queueLog('debug', component, msg, ctx);
    },
    debug: (msg: string, ctx?: Record<string, unknown>) => {
      queueLog('debug', component, msg, ctx);
    },
    info: (msg: string, ctx?: Record<string, unknown>) => {
      queueLog('info', component, msg, ctx);
    },
    warn: (msg: string, ctx?: Record<string, unknown>) => {
      queueLog('warn', component, msg, ctx);
    },
    error: (msg: string, ctx?: Record<string, unknown>) => {
      queueLog('error', component, msg, ctx);
    },
    entry: <T extends Record<string, unknown>>(fn: string, args?: T) => {
      queueLog('debug', component, `→ ${fn}()`, args);
    },
    exit: (fn: string, result?: unknown) => {
      queueLog('debug', component, `← ${fn}()`, result !== undefined ? { result } : undefined);
    },
    stateChange: <T>(name: string, old: T, new_: T) => {
      queueLog('debug', component, `State change: ${name}`, { old, new: new_ });
    },
    event: <T>(name: string, payload?: T) => {
      queueLog('debug', component, `Event received: ${name}`, payload ? { payload } : undefined);
    },
  };
}

// Default logger instance
export default {
  debug: logDebug,
  info: logInfo,
  warn: logWarn,
  error: logError,
  entry: logEntry,
  exit: logExit,
  stateChange: logStateChange,
  event: logEvent,
  createLogger,
  flushLogs,
  LogTarget,
};

// Auto-flush logs when page unloads
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    flushLogs();
  });
  
  // Also flush every 5 seconds as a backup
  setInterval(() => {
    flushLogs();
  }, 5000);
}
