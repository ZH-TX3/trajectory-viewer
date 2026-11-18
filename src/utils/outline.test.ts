import { describe, expect, it } from 'vitest';

import { deriveOutline, OUTLINE_PREVIEW_CHARS } from './outline';
import type { SessionMessage } from '../types';

const msg = (role: string, content: string): SessionMessage => ({ role, content });

describe('deriveOutline', () => {
  // The rail's numbers have to match the rows, so the index is the original
  // position in the message list, not a position among the user turns.
  it('keeps only the turns the reader typed, at their original index', () => {
    const items = deriveOutline([
      msg('user', 'first question'),
      msg('assistant', 'answer'),
      msg('tool', 'tool output'),
      msg('user', 'second question'),
    ]);
    expect(items.map((item) => item.index)).toEqual([0, 3]);
    expect(items.map((item) => item.preview)).toEqual(['first question', 'second question']);
  });

  it('skips injected context', () => {
    const items = deriveOutline([
      msg('user', 'real question'),
      msg('user', '<system-reminder>noise</system-reminder>'),
      msg('user', '<task-notification>agent done</task-notification>'),
    ]);
    expect(items.map((item) => item.index)).toEqual([0]);
  });

  it('collapses whitespace so a multi-line prompt stays one entry', () => {
    const [item] = deriveOutline([msg('user', 'line one\n\n   line  two\t')]);
    expect(item.preview).toBe('line one line two');
  });

  it('truncates a long turn', () => {
    const [item] = deriveOutline([msg('user', 'x'.repeat(OUTLINE_PREVIEW_CHARS + 50))]);
    expect(item.preview).toHaveLength(OUTLINE_PREVIEW_CHARS);
  });

  it('is empty when there is nothing worth navigating', () => {
    expect(deriveOutline([])).toEqual([]);
    expect(deriveOutline([msg('assistant', 'only prose')])).toEqual([]);
  });

  // The Trajectory tab locates a record by time, so the entry has to carry it.
  it('carries the turn timestamp for the trajectory jump', () => {
    const [item] = deriveOutline([{ role: 'user', content: 'hi', ts: 1_700_000_000_000 }]);
    expect(item.ts).toBe(1_700_000_000_000);
    expect(deriveOutline([msg('user', 'no ts')])[0].ts).toBeNull();
  });
});
