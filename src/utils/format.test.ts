// ── Format Utility Tests ─────────────────────────────────────────────────
//
// trajectoryPreviewText is where the opencode null-content crash surfaced
// (null.slice()), so its null/empty handling is asserted explicitly.

import { describe, expect, it } from 'vitest';
import {
  formatDurationMs,
  formatDurationSeconds,
  formatRecordedTime,
  formatThroughput,
  formatTokenCount,
  KIND_LABEL,
  trajectoryPreviewText,
} from './format';

describe('trajectoryPreviewText', () => {
  // Regression: opencode assistant messages carry null content, and this
  // function used to call .slice() on it → white screen.
  it('returns an empty string for null/undefined input', () => {
    expect(trajectoryPreviewText(null as unknown as string)).toBe('');
    expect(trajectoryPreviewText(undefined as unknown as string)).toBe('');
  });

  it('returns an empty string for empty input', () => {
    expect(trajectoryPreviewText('')).toBe('');
    expect(trajectoryPreviewText('   \n  ')).toBe('');
  });

  it('collapses whitespace and strips markdown decoration', () => {
    expect(trajectoryPreviewText('# Title\n\nsome   text')).toBe('Title some text');
    expect(trajectoryPreviewText('**bold** and `code`')).toBe('bold and code');
  });

  it('replaces fenced code blocks with a placeholder', () => {
    const preview = trajectoryPreviewText('before\n```js\nconst a = 1;\n```\nafter');
    expect(preview).toContain('[code]');
    expect(preview).not.toContain('const a = 1');
  });

  it('truncates long input with an ellipsis', () => {
    const preview = trajectoryPreviewText('x'.repeat(5000));
    expect(preview.length).toBeLessThan(600);
    expect(preview.endsWith('…')).toBe(true);
  });
});

describe('duration formatting', () => {
  it('formats milliseconds and guards non-finite values', () => {
    expect(formatDurationMs(1500)).toContain('1,500');
    expect(formatDurationMs(null)).toBe('—');
    expect(formatDurationMs(Number.NaN)).toBe('—');
  });

  it('scales seconds into ms / s / m', () => {
    expect(formatDurationSeconds(0.5)).toBe('500 ms');
    expect(formatDurationSeconds(2.5)).toBe('2.5 s');
    expect(formatDurationSeconds(90)).toBe('1m 30s');
    expect(formatDurationSeconds(null)).toBe('—');
  });

  it('formats recorded timestamps as HH:MM:SS.mmm', () => {
    const out = formatRecordedTime(Date.UTC(2026, 0, 1, 0, 0, 0));
    expect(out).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(formatRecordedTime(null)).toBe('—');
    expect(formatRecordedTime(Number.NaN)).toBe('—');
  });
});

describe('token / throughput formatting', () => {
  it('formats token counts with separators and guards null', () => {
    expect(formatTokenCount(12345)).toBe('12,345');
    expect(formatTokenCount(null)).toBe('—');
    expect(formatTokenCount(undefined)).toBe('—');
  });

  it('computes throughput but guards zero/negative durations', () => {
    expect(formatThroughput(100, 2)).toBe('50.0 tok/s');
    expect(formatThroughput(100, 0)).toBe('—');
    expect(formatThroughput(null, 2)).toBe('—');
  });
});

describe('KIND_LABEL', () => {
  it('labels every trajectory cell kind', () => {
    for (const kind of ['system', 'user', 'context', 'compacted', 'message', 'tool', 'subtool']) {
      expect(KIND_LABEL[kind]).toBeTruthy();
    }
  });
});
