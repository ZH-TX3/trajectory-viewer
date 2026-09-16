// ── Search result grouping ───────────────────────────────────────────────
//
// A flat hit list is hard to read when one conversation matches many times.
// Collapse hits into one entry per conversation so the result list answers
// "which conversation was this in?" first, with the matches nested under it.

import type { SearchHit, SearchSessionGroup } from '../types';

/** Collapse hits into one group per conversation, newest match first. */
export function groupHitsBySession(hits: readonly SearchHit[]): SearchSessionGroup[] {
  const byKey = new Map<string, SearchSessionGroup>();

  for (const hit of hits) {
    const ts = hit.ts ?? null;
    const existing = byKey.get(hit.sessionKey);
    if (existing === undefined) {
      byKey.set(hit.sessionKey, {
        sessionKey: hit.sessionKey,
        provider: hit.provider,
        sessionId: hit.sessionId,
        title: hit.title,
        latestTs: ts,
        hits: [hit],
      });
      continue;
    }
    existing.hits.push(hit);
    // Track the newest match so groups sort by most recent activity.
    if (ts !== null && (existing.latestTs === null || ts > existing.latestTs)) {
      existing.latestTs = ts;
    }
    // Some providers leave the title blank; adopt a sibling's if we have none.
    if (existing.title === '' && hit.title !== '') {
      existing.title = hit.title;
    }
  }

  // Within a group, show newest matches first too.
  for (const group of byKey.values()) {
    group.hits.sort((a, b) => (b.ts ?? 0) - (a.ts ?? 0));
  }

  return [...byKey.values()].sort((a, b) => (b.latestTs ?? 0) - (a.latestTs ?? 0));
}
