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

/**
 * CSS class name for the kind tag label.
 */
export const KIND_LABEL: Record<string, string> = {
  system: 'SYSTEM',
  user: 'USER',
  context: 'CONTEXT',
  compacted: 'COMPACTED',
  message: 'ASSISTANT',
  tool: 'TOOL',
  subtool: 'SUBTOOL',
};