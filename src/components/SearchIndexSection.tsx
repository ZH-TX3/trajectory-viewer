// ── Search index section (Settings → Advanced) ───────────────────────────
//
// Shows how much of the corpus is searchable and lets the user force a
// rebuild. The index is refreshed in the background on startup; this is for
// when it looks stale or the user just wants it current now.

import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Loader2, RefreshCw, Database } from 'lucide-react';
import { api } from '../api';
import type { SearchIndexProgress, SearchIndexStatus } from '../types';
import { cn } from '../lib/utils';

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

export function SearchIndexSection({ providers }: { providers: readonly string[] }) {
  const [status, setStatus] = useState<SearchIndexStatus | null>(null);
  const [progress, setProgress] = useState<SearchIndexProgress | null>(null);
  const [rebuilding, setRebuilding] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  const refreshStatus = useCallback(() => {
    api.searchIndexStatus().then(setStatus).catch(console.error);
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  // Progress events stream while a build runs.
  useEffect(() => {
    const unlisten = listen<SearchIndexProgress>('search-index-progress', (event) => {
      setProgress(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const rebuild = useCallback(
    async (force: boolean) => {
      setRebuilding(true);
      setNotice(null);
      try {
        const stats = (await api.reindexSearch([...providers], force)) as {
          indexedSessions: number;
          skippedSessions: number;
          totalDocs: number;
        };
        setNotice({
          type: 'ok',
          text: `Indexed ${stats.indexedSessions} session(s), ${stats.skippedSessions} unchanged, ${stats.totalDocs} messages searchable.`,
        });
        refreshStatus();
      } catch (err) {
        setNotice({ type: 'error', text: `Indexing failed: ${String(err)}` });
      } finally {
        setRebuilding(false);
        setProgress(null);
      }
    },
    [providers, refreshStatus],
  );

  const pct =
    progress !== null && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : null;

  return (
    <div>
      <div className="px-4 py-3 flex items-center gap-3">
        <Database className="size-3.5 text-muted-foreground shrink-0" />
        <div className="flex-1 min-w-0">
          <div className="text-xs">
            {status === null
              ? 'Checking index…'
              : status.exists
                ? `${status.totalDocs.toLocaleString()} messages across ${status.indexedSessions} conversations`
                : 'No index yet — it builds automatically in the background'}
          </div>
          {progress !== null && pct !== null && (
            <div className="mt-1">
              <div className="h-1 rounded bg-muted overflow-hidden">
                <div
                  className="h-full bg-blue-500 transition-all"
                  style={{ width: `${pct}%` }}
                />
              </div>
              <div className="text-[10px] text-muted-foreground mt-0.5 truncate">
                {pct}% · {progress.done}/{progress.total}
                {progress.current ? ` · ${progress.current}` : ''}
              </div>
            </div>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2 px-4 pb-3">
        <button
          onClick={() => rebuild(false)}
          disabled={rebuilding}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] font-medium bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-60"
        >
          {rebuilding ? <Loader2 className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
          {rebuilding ? 'Indexing…' : 'Refresh index'}
        </button>
        <button
          onClick={() => rebuild(true)}
          disabled={rebuilding}
          title="Reindex every session, ignoring what has already been indexed"
          className="px-2.5 py-1.5 rounded-md text-[11px] border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-60"
        >
          Rebuild from scratch
        </button>
      </div>

      {notice && (
        <div
          className={cn(
            'mx-4 mb-3 px-3 py-2 rounded-lg text-[11px] leading-relaxed break-words',
            notice.type === 'ok'
              ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
              : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
          )}
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}
