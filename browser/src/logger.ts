/**
 * Structured JSON logging for browser automation
 * Outputs machine-parseable logs for container environments
 */

interface LogContext {
  [key: string]: string | number | boolean | undefined;
}

function timestamp(): string {
  return new Date().toISOString();
}

function log(level: string, event: string, message: string, context?: LogContext): void {
  const entry = {
    timestamp: timestamp(),
    level,
    event,
    message,
    ...context,
  };
  console.log(JSON.stringify(entry));
}

export function logInfo(event: string, message: string, context?: LogContext): void {
  log('info', event, message, context);
}

export function logError(event: string, message: string, context?: LogContext): void {
  log('error', event, message, context);
}

export function logDebug(event: string, message: string, context?: LogContext): void {
  log('debug', event, message, context);
}

export function logWarn(event: string, message: string, context?: LogContext): void {
  log('warn', event, message, context);
}

/**
 * Timer utility for measuring operation duration
 * Returns a function that when called returns elapsed milliseconds
 */
export function timer(name: string): () => number {
  const start = Date.now();
  return () => Date.now() - start;
}
