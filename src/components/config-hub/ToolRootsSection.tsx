// ── Config Hub: tool config-root overrides ───────────────────────────────
//
// Every tool resolves its config directory from `$HOME` automatically. If a
// tool's config lives somewhere else, set an override here; every derived path
// (skills/agents/prompt/config files) in the hub follows it.

import { useCallback, useEffect, useState } from 'react';
import { Loader2, RotateCcw } from 'lucide-react';
import { api } from '../../api';
import type { ToolRootInfo } from '../../types';
import { cn } from '../../lib/utils';
import { ToolBadge } from '../icons/BrandIcons';

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

export function ToolRootsSection() {
  const [roots, setRoots] = useState<ToolRootInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [savingId, setSavingId] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);

  const refresh = useCallback(async () => {
    try {
      const items = await api.configHubToolRoots();
      setRoots(items);
      setDrafts(
        Object.fromEntries(items.map((i) => [i.toolId, i.overrideRoot ?? ''])),
      );
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to load tool roots: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = useCallback(
    async (toolId: string, dir: string) => {
      setSavingId(toolId);
      setNotice(null);
      try {
        await api.configHubSetToolRoot(toolId, dir.trim());
        setNotice({
          type: 'ok',
          text: dir.trim() ? `Overrode ${toolId}'s config root` : `Cleared ${toolId}'s override`,
        });
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setSavingId(null);
      }
    },
    [refresh],
  );

  return (
    <section className="rounded-xl border border-[hsl(var(--border))] overflow-hidden">
      <div className="px-4 py-3 border-b border-[hsl(var(--border))]">
        <h2 className="text-xs font-medium">Tool config roots</h2>
        <p className="text-[10px] text-muted-foreground mt-0.5">
          Each tool's config directory is auto-detected from <span className="font-mono">$HOME</span>.
          Set an override here if a tool's config lives somewhere else — the hub will follow it.
        </p>
      </div>

      {notice && (
        <div
          className={cn(
            'mx-4 mt-2 px-3 py-2 rounded-lg text-[11px] break-all',
            notice.type === 'ok'
              ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
              : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
          )}
        >
          {notice.text}
        </div>
      )}

      <div className="divide-y divide-[hsl(var(--border))]">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin mr-2 inline" />
            Loading…
          </div>
        ) : (
          roots.map((root) => {
            const draft = drafts[root.toolId] ?? '';
            const dirty = draft.trim() !== (root.overrideRoot ?? '');
            return (
              <div key={root.toolId} className="px-4 py-3">
                <div className="flex items-center gap-2 mb-1">
                  <ToolBadge toolId={root.toolId} className="size-3.5" />
                  <span className="text-xs font-medium">{root.displayName}</span>
                </div>
                <div className="flex items-center gap-2">
                  <input
                    value={draft}
                    onChange={(e) =>
                      setDrafts((d) => ({ ...d, [root.toolId]: e.target.value }))
                    }
                    placeholder={root.defaultRoot ?? 'not auto-detected'}
                    className="flex-1 min-w-0 text-[10px] font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
                  />
                  {dirty && (
                    <button
                      onClick={() => void save(root.toolId, draft)}
                      disabled={savingId !== null}
                      title="Apply override"
                      className="px-2 py-1 rounded text-[10px] bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50"
                    >
                      {savingId === root.toolId ? (
                        <Loader2 className="size-3 animate-spin" />
                      ) : (
                        'Apply'
                      )}
                    </button>
                  )}
                  {root.overrideRoot && (
                    <button
                      onClick={() => void save(root.toolId, '')}
                      disabled={savingId !== null}
                      title="Clear override (use auto-detected)"
                      className="p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-50"
                    >
                      <RotateCcw className="size-3" />
                    </button>
                  )}
                </div>
                {root.overrideRoot && (
                  <div className="mt-1 text-[9px] text-blue-600 dark:text-blue-400">
                    override: {root.overrideRoot}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}