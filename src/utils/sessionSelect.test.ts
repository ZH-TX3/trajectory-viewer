import { describe, expect, it } from 'vitest';

/**
 * Regression tests for the session-list selection/expansion rules.
 *
 * These model the two effects in SessionBrowser that decide which group is
 * expanded and which session is selected. The old behaviour was:
 *   - auto-expand group #1 ran INSIDE the auto-select effect, so every click
 *     re-added it (collapsing group #1 never stuck);
 *   - a selection outside the filtered list was silently replaced with the
 *     FIRST session (click A, land on B).
 */

type S = { providerId: string; sessionId: string };
type G = { groupName: string; sessions: S[] };

const key = (s: S) => `${s.providerId}::${s.sessionId}`;

/** Current rule: clear the selection when it is not in the filtered list. */
function resolveSelection(
  grouped: G[],
  filtered: S[],
  selectedKey: string | null,
  pendingKey: string | null,
): string | null {
  if (grouped.length === 0) return null;
  // A cross-session jump's key is exempt for one pass, until the filter lands.
  if (pendingKey !== null) {
    if (pendingKey === selectedKey) return selectedKey;
    return selectedKey;
  }
  const exists = filtered.some((s) => key(s) === selectedKey);
  return exists ? selectedKey : null;
}

/** Current rule: expand group #1 once per filter, never on a click. */
function resolveExpansion(
  grouped: G[],
  filterKey: string,
  alreadyDoneFor: string | null,
  expanded: Set<string>,
): { expanded: Set<string>; doneFor: string } {
  if (grouped.length === 0) return { expanded, doneFor: alreadyDoneFor ?? '' };
  if (alreadyDoneFor === filterKey) return { expanded, doneFor: alreadyDoneFor };
  return { expanded: new Set(expanded).add(grouped[0].groupName), doneFor: filterKey };
}

const s = (p: string, id: string): S => ({ providerId: p, sessionId: id });
const groups: G[] = [
  { groupName: 'projA', sessions: [s('claude', 'a1'), s('claude', 'a2')] },
  { groupName: 'projB', sessions: [s('claude', 'b1')] },
];
const filtered = [...groups[0].sessions, ...groups[1].sessions];

describe('selection', () => {
  it('keeps a selection that is in the list', () => {
    expect(resolveSelection(groups, filtered, 'claude::a2', null)).toBe('claude::a2');
  });

  // Old behaviour returned 'claude::a1' here — clicking a session and landing
  // on a different one.
  it('clears a selection outside the list instead of jumping to the first', () => {
    expect(resolveSelection(groups, filtered, 'codex::z', null)).toBeNull();
  });

  it('clears when there are no groups', () => {
    expect(resolveSelection([], [], 'claude::a1', null)).toBeNull();
  });

  // A cross-session jump sets filter + key together; the key is exempt until it
  // matches, so it is not cleared on the in-between render.
  it('honours a pending cross-session jump', () => {
    expect(resolveSelection(groups, filtered, 'codex::z', 'codex::z')).toBe('codex::z');
  });
});

describe('expansion', () => {
  it('expands the first group for a new filter', () => {
    const out = resolveExpansion(groups, 'claude|claude', null, new Set());
    expect(out.expanded.has('projA')).toBe(true);
  });

  // The reported bug: collapsing group #1 used to be undone by every click.
  it('leaves a collapsed first group alone once the filter is done', () => {
    const out = resolveExpansion(groups, 'claude|claude', 'claude|claude', new Set());
    expect(out.expanded.has('projA')).toBe(false);
  });

  // A 30s refresh produces a new list but the same filter key, so it must not
  // re-expand what the user collapsed.
  it('does not re-expand on refresh (same filter, new list)', () => {
    const collapsed = new Set<string>();
    const refreshed: G[] = [
      { groupName: 'projA', sessions: [...groups[0].sessions, s('claude', 'a3')] },
      ...groups.slice(1),
    ];
    const out = resolveExpansion(refreshed, 'claude|claude', 'claude|claude', collapsed);
    expect(out.expanded.has('projA')).toBe(false);
  });

  it('re-expands when the filter actually changes', () => {
    const out = resolveExpansion(groups, 'codex|claude,codex', 'claude|claude', new Set());
    expect(out.expanded.has('projA')).toBe(true);
  });
});
