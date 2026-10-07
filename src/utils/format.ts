// ── Trajectory Formatting Utilities ──────────────────────────────────────

/**
 * Format a duration in milliseconds to a human-readable string.
 */
export function formatDurationMs(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return '—';
  return `${Math.round(ms).toLocaleString()} ms`;
}

/**
 * Format an elapsed duration given in seconds.
 */
export function formatElapsedSeconds(seconds: number | null): string {
  return formatDurationMs(seconds === null ? null : seconds * 1000);
}

/**
 * Format a Unix epoch timestamp to a locale time string.
 */
export function formatTimestamp(ts: number | null | undefined): string {
  if (ts == null) return '—';
  try {
    return new Date(ts).toLocaleTimeString();
  } catch {
    return '—';
  }
}

/**
 * Format a Unix epoch timestamp to a full date-time string.
 */
export function formatDateTime(ts: number | null | undefined): string {
  if (ts == null) return '—';
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return '—';
  }
}

/**
 * Format a timestamp as HH:MM:SS.mmm (wall-clock time), DSH formatRecordedTime.
 */
export function formatRecordedTime(ts: number | null | undefined): string {
  if (ts == null || !Number.isFinite(ts)) return '—';
  const date = new Date(ts);
  const two = (v: number) => String(v).padStart(2, '0');
  const three = (v: number) => String(v).padStart(3, '0');
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}.${three(date.getMilliseconds())}`;
}

/**
 * Format a duration in seconds for the timeline tooltip.
 */
export function formatDurationSeconds(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return '—';
  if (seconds < 1) return `${Math.round(seconds * 1000)} ms`;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}m ${secs.toFixed(0)}s`;
}

/**
 * Format token count with thousands separator.
 */
export function formatTokenCount(count: number | null | undefined): string {
  if (count == null) return '—';
  return count.toLocaleString();
}

/**
 * Format throughput (tokens per second).
 */
export function formatThroughput(tokens: number | null, seconds: number | null): string {
  if (tokens == null || seconds == null || seconds <= 0) return '—';
  return `${(tokens / seconds).toFixed(1)} tok/s`;
}

/**
 * Build a compact one-line preview from Markdown text.
 */
export function trajectoryPreviewText(text: string): string {
  if (text == null) return '';
  const source = text.slice(0, 2048);
  const compact = source
    .replace(/```[\s\S]*?```/g, '[code]')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/[#*_~>|]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
  const preview = compact.slice(0, 512).trimEnd();
  return source.length < text.length || preview.length < compact.length
    ? `${preview}…`
    : preview;
}

// ── Subagent task notifications ──────────────────────────────────────────
//
// A finished subagent reports back on a `user` line holding a
// `<task-notification>` XML blob. Raw, it is 2–3KB of tags and temp-file
// paths; these pull out the parts worth showing.

export interface TaskNotification {
  agentId: string | null;
  status: string | null;
  summary: string | null;
  result: string | null;
  /** A run that ended badly — `failed` or `killed`. */
  isError: boolean;
}

/** Whether a message body is a subagent task notification. */
export function isTaskNotification(text: string | null | undefined): boolean {
  return text != null && text.trimStart().startsWith('<task-notification>');
}

/** Read `<tag>value</tag>` out of a task notification body. */
function xmlField(text: string, tag: string): string | null {
  const open = `<${tag}>`;
  const start = text.indexOf(open);
  if (start < 0) return null;
  const close = text.indexOf(`</${tag}>`, start + open.length);
  if (close < 0) return null;
  const value = text.slice(start + open.length, close).trim();
  return value === '' ? null : value;
}

/** Parse a `<task-notification>` body; `null` when the text is not one. */
export function parseTaskNotification(text: string): TaskNotification | null {
  if (!isTaskNotification(text)) return null;
  const status = xmlField(text, 'status');
  return {
    agentId: xmlField(text, 'task-id'),
    status,
    summary: xmlField(text, 'summary'),
    result: xmlField(text, 'result'),
    isError: status === 'failed' || status === 'killed',
  };
}

/**
 * CSS class name for the kind tag label.
 */
export const KIND_LABEL: Record<string, string> = {  system: 'SYSTEM',
  user: 'USER',
  context: 'CONTEXT',
  compacted: 'COMPACTED',
  message: 'ASSISTANT',
  tool: 'TOOL',
  subtool: 'SUBAGENT',
};