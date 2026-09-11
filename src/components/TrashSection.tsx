// ── Trash Section ────────────────────────────────────────────────────────
//
// Sessions deleted from the browser are moved here (restorable), not erased.
// Shows every trashed session with restore / permanently-delete, plus an
// empty-trash action. Entries older than 30 days are auto-purged on startup.

import { useCallback, useEffect, useState } from 'react';
import { Loader2, RotateCcw, Trash2, ShieldOff } from 'lucide-react';
import { api } from '../api';
import type { TrashEntry } from '../types';
import { cn } from '../lib/utils';

const PROVIDER_LABELS: Record<string, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  dsh: 'DSH',
  opencode: 'OpenCode',
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

export function TrashSection() {
  const [entries, setEntries] = useState<TrashEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [working, setWorking] = useState<'restore' | 'delete' | 'empty' | null>(null);
  const [confirm, setConfirm] = useState<{ kind: 'restore' | 'delete' | 'empty'; id?: string } | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);

  const refresh = useCallback(async () => {
    try {
      setEntries(await api.listTrash());
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to load trash: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const doRestore = useCallback(
    async (id: string) => {
      setWorking('restore');
      setNotice(null);
      try {
        const manifest = await api.restoreTrashEntry(id);
        setNotice({ type: 'ok', text: `Restored ${manifest.title || manifest.sessionId} → ${manifest.providerId}` });
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: `Restore failed: ${String(err)}` });
      } finally {
        setWorking(null);
        setConfirm(null);
      }
    },
    [refresh],
  );

  const doDelete = useCallback(
    async (id: string) => {
      setWorking('delete');
      setNotice(null);
      try {
        await api.deleteTrashEntry(id);
        setNotice({ type: 'ok', text: 'Deleted permanently' });
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: `Delete failed: ${String(err)}` });
      } finally {
        setWorking(null);
        setConfirm(null);
      }
    },
    [refresh],
  );

  const doEmpty = useCallback(async () => {
    setWorking('empty');
    setNotice(null);
    try {
      const count = await api.emptyTrash();
      setNotice({ type: 'ok', text: `Emptied ${count} trash entrie(s)` });
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: `Empty failed: ${String(err)}` });
    } finally {
      setWorking(null);
      setConfirm(null);
    }
  }, [refresh]);

  return (
    <div>
      {/* Header + empty / refresh */}
      <div className="flex items-center gap-2 px-4 py-3 border-t border-border/40">
        <span className="text-[11px] text-muted-foreground">
          {entries.length} trashed {entries.length === 1 ? 'session' : 'sessions'} · auto-purged after 30 days
        </span>
        <div className="flex-1" />
        <button
          onClick={() => entries.length > 0 && setConfirm({ kind: 'empty' })}
          disabled={working !== null || entries.length === 0}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] border border-red-500/40 text-red-600 dark:text-red-400 hover:bg-red-500/10 transition-colors disabled:opacity-50"
        >
          <Trash2 className="size-3" />
          Empty trash
        </button>
      </div>

      {/* Entries */}
      <div className="border-t border-border/40 divide-y divide-border/40">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin mr-2 inline" />Loading trash…
          </div>
        ) : entries.length === 0 ? (
          <div className="p-4 text-xs text-muted-foreground text-center">Trash is empty.</div>
        ) : (
          entries.map((entry) => (
            <div key={entry.id} className="px-4 py-2 flex items-center gap-3">
              <ShieldOff className="size-3 text-muted-foreground/50 shrink-0" />
              <div className="flex-1 min-w-0">
                <div className="text-xs truncate">{entry.title || entry.sessionId}</div>
                <div className="text-[10px] text-muted-foreground truncate">
                  {PROVIDER_LABELS[entry.providerId] ?? entry.providerId} ·{' '}
                  {new Date(entry.deletedAt).toLocaleString()} · {formatBytes(entry.sizeBytes)}
                </div>
              </div>
              <div className="shrink-0 flex items-center gap-0.5">
                <button
                  onClick={() => setConfirm({ kind: 'restore', id: entry.id })}
                  disabled={working !== null}
                  title="Restore"
                  className="p-1 rounded text-blue-600 dark:text-blue-400 hover:bg-blue-500/10 transition-colors disabled:opacity-50"
                >
                  <RotateCcw className="size-3" />
                </button>
                <button
                  onClick={() => setConfirm({ kind: 'delete', id: entry.id })}
                  disabled={working !== null}
                  title="Delete permanently"
                  className="p-1 rounded text-red-500 hover:text-red-600 hover:bg-red-500/10 transition-colors disabled:opacity-50"
                >
                  <Trash2 className="size-3" />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Confirm */}
      {confirm && (
        <div className="mx-4 my-3 rounded-lg border border-amber-300/60 bg-amber-50 dark:bg-amber-900/15 p-3">
          <div className="text-[11px] text-amber-800 dark:text-amber-200 leading-relaxed">
            <div className="font-medium mb-1">
              {confirm.kind === 'restore'
                ? 'Restore this trashed session?'
                : confirm.kind === 'empty'
                  ? 'Empty the trash?'
                  : 'Delete permanently?'}
            </div>
            <div className="opacity-80">
              {confirm.kind === 'restore'
                ? 'It will be put back at its original location.'
                : confirm.kind === 'empty'
                  ? 'Every trashed session will be erased for good.'
                  : 'This trashed session will be erased for good.'}
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-2">
            <button
              onClick={() => setConfirm(null)}
              disabled={working !== null}
              className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={() =>
                confirm.kind === 'restore'
                  ? doRestore(confirm.id!)
                  : confirm.kind === 'empty'
                    ? doEmpty()
                    : doDelete(confirm.id!)
              }
              disabled={working !== null}
              className={cn(
                'px-3 py-1 rounded text-xs text-white transition-colors disabled:opacity-60',
                confirm.kind === 'restore' ? 'bg-amber-500 hover:bg-amber-600' : 'bg-red-600 hover:bg-red-700',
              )}
            >
              {working !== null ? 'Working…' : confirm.kind === 'restore' ? 'Restore' : 'Delete'}
            </button>
          </div>
        </div>
      )}

      {/* Notice */}
      {notice && (
        <div
          className={cn(
            'mx-4 mb-3 px-3 py-2 rounded-lg text-[11px] leading-relaxed break-all',
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