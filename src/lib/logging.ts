/**
 * PageSeeds Logging Module
 * 
 * Provides consistent, AI-friendly logging for the frontend.
 * All logs are sent to the Rust backend and written to the log file
 * with the same format as backend logs.
 * 
 * Log Format:
 * [YYYY-MM-DD HH:MM:SS][LEVEL][target] message
 * 
 * Log Location:
 * macOS: ~/Library/Logs/com.pageseeds.app/pageseeds_YYYY-MM-DD.log
 * Windows: %APPDATA%\PageSeeds\logs\pageseeds_YYYY-MM-DD.log
 * Linux: ~/.local/share/PageSeeds/logs/pageseeds_YYYY-MM-DD.log
 */

import { trace, debug, info, warn, error } from '@tauri-apps/plugin-log';

// Log targets for categorization
export const LogTarget = {
  QUEUE: 'pageseeds::queue',
  TASKS: 'pageseeds::tasks',
  UI: 'pageseeds::ui',
  API: 'pageseeds::api',
  RENDER: 'pageseeds::render',
  STATE: 'pageseeds::state',
  EVENT: 'pageseeds::event',
} as const;

type LogTargetType = typeof LogTarget[keyof typeof LogTarget];

// Get current timestamp in the same format as Rust
function getTimestamp(): string {
  const now = new Date();
  return now.toISOString().replace('T', ' ').slice(0, 19);
}

// Format a log message with context
function formatMessage(target: LogTargetType, message: string, context?: Record<string, unknown>): string {
  let msg = `[${target}] ${message}`;
  if (context && Object.keys(context).length > 0) {
    try {
      msg += ` | context=${JSON.stringify(context)}`;
    } catch {
      msg += ` | context=[unserializable]`;
    }
  }
  return msg;
}

/**
 * Log at TRACE level (very detailed debugging)
 */
export function logTrace(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  const msg = formatMessage(target, message, context);
  console.trace(`[${getTimestamp()}][TRACE]${msg}`);
  trace(msg).catch(() => {}); // Ignore errors from logging
}

/**
 * Log at DEBUG level (detailed debugging)
 */
export function logDebug(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  const msg = formatMessage(target, message, context);
  console.debug(`[${getTimestamp()}][DEBUG]${msg}`);
  debug(msg).catch(() => {});
}

/**
 * Log at INFO level (general information)
 */
export function logInfo(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  const msg = formatMessage(target, message, context);
  console.info(`[${getTimestamp()}][INFO]${msg}`);
  info(msg).catch(() => {});
}

/**
 * Log at WARN level (warnings)
 */
export function logWarn(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  const msg = formatMessage(target, message, context);
  console.warn(`[${getTimestamp()}][WARN]${msg}`);
  warn(msg).catch(() => {});
}

/**
 * Log at ERROR level (errors)
 */
export function logError(target: LogTargetType, message: string, context?: Record<string, unknown>): void {
  const msg = formatMessage(target, message, context);
  console.error(`[${getTimestamp()}][ERROR]${msg}`);
  error(msg).catch(() => {});
}

/**
 * Log a function entry with arguments
 */
export function logEntry<T extends Record<string, unknown>>(
  target: LogTargetType, 
  functionName: string, 
  args?: T
): void {
  logDebug(target, `→ ${functionName}()`, args);
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
  logDebug(target, `← ${functionName}()`, context);
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
  logDebug(target, `State change: ${stateName}`, {
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
  logDebug(target, `Event received: ${eventName}`, payload ? { payload } : undefined);
}

/**
 * Create a scoped logger for a specific component/module
 */
export function createLogger(target: LogTargetType) {
  return {
    trace: (msg: string, ctx?: Record<string, unknown>) => logTrace(target, msg, ctx),
    debug: (msg: string, ctx?: Record<string, unknown>) => logDebug(target, msg, ctx),
    info: (msg: string, ctx?: Record<string, unknown>) => logInfo(target, msg, ctx),
    warn: (msg: string, ctx?: Record<string, unknown>) => logWarn(target, msg, ctx),
    error: (msg: string, ctx?: Record<string, unknown>) => logError(target, msg, ctx),
    entry: <T extends Record<string, unknown>>(fn: string, args?: T) => logEntry(target, fn, args),
    exit: (fn: string, result?: unknown) => logExit(target, fn, result),
    stateChange: <T>(name: string, old: T, new_: T) => logStateChange(target, name, old, new_),
    event: <T>(name: string, payload?: T) => logEvent(target, name, payload),
  };
}

// Default logger instance
export default {
  trace: logTrace,
  debug: logDebug,
  info: logInfo,
  warn: logWarn,
  error: logError,
  entry: logEntry,
  exit: logExit,
  stateChange: logStateChange,
  event: logEvent,
  createLogger,
  LogTarget,
};
