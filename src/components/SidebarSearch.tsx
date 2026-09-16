// ── Sidebar cross-session search ─────────────────────────────────────────
//
// Search every indexed conversation and show the results grouped by
// conversation, so the answer to "where did I discuss this?" is a session
// title rather than a wall of loose matches. Clicking a group opens that
// session and seeds the in-session search with the same query.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Search, X, Loader2, MessageSquare } from 'lucide-react';
import { api } from '../api';
import { highlightSnippet } from '../utils/highlight';
import { groupHitsBySession } from '../utils/searchGroups';
import type { SearchHit, SearchSessionGroup } from '../types';
import { cn } from '../lib/utils';

const PROVIDER_LABELS: Record<string, string> = {
  claude: 'Claude',
  codex: 'Codex',
  dsh: 'DSH',
  opencode: 'OpenCode',
};

const DEBOUNCE_MS = 180;

interface SidebarSearchProps {
  /** Providers to search; empty means no filter. */
  providers: readonly string[];
  /** Controlled query — owned by the parent so ESC can clear it. */
  query: string;
  onQueryChange: (query: string) => void;
  /** Open a session and focus the matched message at `focusTs`. */
  onOpenResult: (group: SearchSessionGroup, focusTs: number | null) => void;
  /** Rendered when there is no query, so the sidebar can show its normal list. */
  children: React.ReactNode;
}

function formatTime(ts: number | null | undefined): string {
  if (!ts) return '';
  return new Date(ts).toLocaleString();
}

/** One matching message inside a conversation. */
function HitRow({ hit, onOpen }: { hit: SearchHit; onOpen: () => void }) {
  const segments = useMemo(
    () => highlightSnippet(hit.snippet, hit.matchStart, hit.matchLen),
    [hit.snippet, hit.matchStart, hit.matchLen],
  );

  return (
    <button
      onClick={onOpen}
      className="w-full text-left px-2 py-1.5 rounded hover:bg-muted/60 transition-colors"
    >
      <div className="flex items-center gap-1.5 mb-0.5">
        <span
          className={cn(
            'text-[9px] font-mono uppercase shrink-0',
            hit.role === 'user' ? 'text-emerald-600 dark:text-emerald-400'
              : hit.role === 'assistant' ? 'text-violet-600 dark:text-violet-400'
                : 'text-muted-foreground',
          )}
        >
          {hit.role}
        </span>
        {hit.ts != null && (
          <span className="text-[9px] text-muted-foreground/60 truncate">{formatTime(hit.ts)}</span>
        )}
      </div>
      <div className="text-[11px] text-foreground/75 leading-snug line-clamp-3 break-words">
        {segments.map((seg, i) =>
          seg.match ? (
            <mark key={i} className="bg-yellow-200 dark:bg-yellow-700/60 text-inherit rounded-sm px-0.5">
              {seg.text}
            </mark>
          ) : (
            <span key={i}>{seg.text}</span>
          ),
        )}
      </div>
    </button>
  );
}

export function SidebarSearch({ providers, query, onQueryChange, onOpenResult, children }: SidebarSearchProps) {
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Guards against an older query's response overwriting a newer one.
  const requestRef = useRef(0);

  const trimmed = query.trim();

  // When the query is cleared from outside (e.g. ESC), reset the result state
  // so the sidebar snaps back to the normal session list.
  useEffect(() => {
    if (trimmed === '') {
      setHits([]);
      setExpanded(new Set());
    }
  }, [trimmed]);

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);

    if (trimmed === '') {
      setHits([]);
      setError(null);
      setSearching(false);
      return;
    }

    setSearching(true);
    const requestId = ++requestRef.current;
    debounceRef.current = setTimeout(() => {
      api
        .searchSessions(trimmed, [...providers])
        .then((result) => {
          if (requestId !== requestRef.current) return; // stale response
          setHits(result);
          setError(null);
        })
        .catch((err) => {
          if (requestId !== requestRef.current) return;
          setError(String(err));
          setHits([]);
        })
        .finally(() => {
          if (requestId === requestRef.current) setSearching(false);
        });
    }, DEBOUNCE_MS);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [trimmed, providers]);

  const groups = useMemo(() => groupHitsBySession(hits), [hits]);
  const totalMatches = hits.length;

  const toggleExpanded = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const clear = useCallback(() => onQueryChange(''), [onQueryChange]);

  return (
    <div className="flex flex-col min-h-0 flex-1">
      {/* Search box */}
      <div className="px-2 py-1.5 border-b border-border/40 shrink-0">
        <div className="flex items-center gap-1 px-2 h-7 rounded-md bg-muted/40 border border-border/40">
          {searching ? (
            <Loader2 className="size-3 animate-spin text-muted-foreground shrink-0" />
          ) : (
            <Search className="size-3 text-muted-foreground shrink-0" />
          )}
          <input
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') clear();
            }}
            placeholder="Search all conversations"
            className="flex-1 min-w-0 bg-transparent text-[11px] outline-none placeholder:text-muted-foreground/50"
          />
          {query !== '' && (
            <button
              onClick={clear}
              title="Clear search"
              aria-label="Clear search"
              className="shrink-0 p-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/60"
            >
              <X className="size-3" />
            </button>
          )}
        </div>
      </div>

      {/* No query → the normal session list */}
      {trimmed === '' ? (
        <div className="flex-1 min-h-0 flex flex-col">{children}</div>
      ) : (
        <div className="flex-1 min-h-0 overflow-auto">
          {error !== null ? (
            <div className="p-3 text-[11px] text-red-600 dark:text-red-400 break-words">
              Search failed: {error}
            </div>
          ) : searching && hits.length === 0 ? (
            <div className="p-3 text-[11px] text-muted-foreground">Searching…</div>
          ) : groups.length === 0 ? (
            <div className="p-3 text-[11px] text-muted-foreground text-center">
              No matches for “{trimmed}”
            </div>
          ) : (
            <div className="py-1">
              <div className="px-2 py-1 text-[10px] text-muted-foreground">
                {totalMatches} match{totalMatches === 1 ? '' : 'es'} in {groups.length}{' '}
                conversation{groups.length === 1 ? '' : 's'}
              </div>

              {groups.map((group) => {
                const isOpen = expanded.has(group.sessionKey);
                return (
                  <div key={group.sessionKey} className="mb-0.5">
                    {/* Conversation header — the grouping the user asked for */}
                    <button
                      onClick={() => toggleExpanded(group.sessionKey)}
                      className="w-full text-left px-2 py-1.5 hover:bg-muted/50 transition-colors"
                    >
                      <div className="flex items-start gap-1.5">
                        <MessageSquare className="size-3 shrink-0 mt-0.5 text-muted-foreground/60" />
                        <div className="flex-1 min-w-0">
                          <div className="text-[11px] font-medium truncate">
                            {group.title || group.sessionId.slice(0, 12)}
                          </div>
                          <div className="text-[9px] text-muted-foreground truncate">
                            {PROVIDER_LABELS[group.provider] ?? group.provider}
                            {' · '}
                            {group.hits.length} match{group.hits.length === 1 ? '' : 'es'}
                            {group.latestTs != null ? ` · ${formatTime(group.latestTs)}` : ''}
                          </div>
                        </div>
                      </div>
                    </button>

                    {/* Matches inside this conversation */}
                    <div className="pl-3 pr-1 pb-1">
                      {(isOpen ? group.hits : group.hits.slice(0, 2)).map((hit, i) => (
                        <HitRow
                          key={`${hit.sessionKey}-${hit.ts ?? 0}-${i}`}
                          hit={hit}
                          onOpen={() => onOpenResult(group, hit.ts ?? null)}
                        />
                      ))}
                      {!isOpen && group.hits.length > 2 && (
                        <button
                          onClick={() => toggleExpanded(group.sessionKey)}
                          className="px-2 py-0.5 text-[10px] text-muted-foreground hover:text-foreground"
                        >
                          +{group.hits.length - 2} more
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
