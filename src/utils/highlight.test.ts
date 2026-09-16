import { describe, expect, it } from 'vitest';
import { highlightSnippet } from './highlight';
import { groupHitsBySession } from './searchGroups';
import type { SearchHit } from '../types';

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    sessionKey: 'claude::s1',
    provider: 'claude',
    sessionId: 's1',
    title: 'Session',
    role: 'user',
    ts: 1000,
    snippet: 'the snippet',
    matchStart: 0,
    matchLen: 3,
    ...overrides,
  };
}

describe('highlightSnippet', () => {
  it('splits around the match', () => {
    const segs = highlightSnippet('前白屏后', 1, 2);
    expect(segs).toEqual([
      { text: '前', match: false },
      { text: '白屏', match: true },
      { text: '后', match: false },
    ]);
  });

  it('handles a match at the very start', () => {
    const segs = highlightSnippet('白屏后续', 0, 2);
    expect(segs[0]).toEqual({ text: '白屏', match: true });
    expect(segs).toHaveLength(2);
  });

  it('handles a match at the very end', () => {
    const segs = highlightSnippet('前缀白屏', 2, 2);
    expect(segs[segs.length - 1]).toEqual({ text: '白屏', match: true });
  });

  it('returns one plain segment when there is nothing to highlight', () => {
    expect(highlightSnippet('text', null, 0)).toEqual([{ text: 'text', match: false }]);
    expect(highlightSnippet('text', undefined, 0)).toEqual([{ text: 'text', match: false }]);
  });

  it('ignores offsets that fall outside the snippet', () => {
    // Defensive: a stale offset must not produce a broken render.
    expect(highlightSnippet('short', 99, 5)).toEqual([{ text: 'short', match: false }]);
    expect(highlightSnippet('short', -1, 2)).toEqual([{ text: 'short', match: false }]);
    expect(highlightSnippet('short', 3, 99)).toEqual([{ text: 'short', match: false }]);
  });

  it('handles an empty snippet', () => {
    expect(highlightSnippet('', 0, 0)).toEqual([]);
  });

  it('reassembles to the original text', () => {
    const segs = highlightSnippet('前缀白屏后缀', 2, 2);
    expect(segs.map((s) => s.text).join('')).toBe('前缀白屏后缀');
  });
});

describe('groupHitsBySession', () => {
  it('collapses hits from one session into a single group', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'claude::s1', ts: 100 }),
      hit({ sessionKey: 'claude::s1', ts: 300 }),
      hit({ sessionKey: 'claude::s2', ts: 200 }),
    ]);

    expect(groups).toHaveLength(2);
    const s1 = groups.find((g) => g.sessionKey === 'claude::s1')!;
    expect(s1.hits).toHaveLength(2);
  });

  it('orders groups by their newest match', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'a', ts: 100 }),
      hit({ sessionKey: 'b', ts: 900 }),
      hit({ sessionKey: 'c', ts: 500 }),
    ]);
    expect(groups.map((g) => g.sessionKey)).toEqual(['b', 'c', 'a']);
  });

  it('orders hits inside a group newest first', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'a', ts: 100 }),
      hit({ sessionKey: 'a', ts: 900 }),
      hit({ sessionKey: 'a', ts: 500 }),
    ]);
    expect(groups[0].hits.map((h) => h.ts)).toEqual([900, 500, 100]);
  });

  it('handles missing timestamps without crashing the sort', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'a', ts: null }),
      hit({ sessionKey: 'b', ts: 500 }),
    ]);
    expect(groups.map((g) => g.sessionKey)).toEqual(['b', 'a']);
    expect(groups.find((g) => g.sessionKey === 'a')!.latestTs).toBeNull();
  });

  it('adopts a sibling title when one is blank', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'a', title: '' }),
      hit({ sessionKey: 'a', title: 'Real Title' }),
    ]);
    expect(groups[0].title).toBe('Real Title');
  });

  it('keeps provider and session id from the hits', () => {
    const groups = groupHitsBySession([
      hit({ sessionKey: 'opencode::ses_1', provider: 'opencode', sessionId: 'ses_1' }),
    ]);
    expect(groups[0].provider).toBe('opencode');
    expect(groups[0].sessionId).toBe('ses_1');
  });

  it('returns nothing for no hits', () => {
    expect(groupHitsBySession([])).toEqual([]);
  });
});
