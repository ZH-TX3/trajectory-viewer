// ── Config Hub: MCP server panel ─────────────────────────────────────────
//
// The unified MCP store: a list of servers, each with per-tool enable toggles
// (brand-icon chips), plus add / edit / import-from-tools actions. Saving or
// toggling writes into each enabled tool's live config.

import { useCallback, useEffect, useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Zap,
} from 'lucide-react';
import { api } from '../../api';
import type { McpEditorState, McpServer, McpTestResult } from '../../types';
import {
  TOOL_ACTIVE_CLASSES,
  editorToMcp,
  mcpToEditor,
} from '../../utils/configHub';
import { cn } from '../../lib/utils';
import { McpEditorModal } from './McpEditorModal';
import { ToolBadge } from '../icons/BrandIcons';

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

const MCP_TOOLS = ['claude', 'codex', 'opencode'];
const TOOL_LABELS: Record<string, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};

export function McpPanel() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [editor, setEditor] = useState<McpEditorState | null>(null);
  const [pendingDelete, setPendingDelete] = useState<McpServer | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [testing, setTesting] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, McpTestResult>>({});
  const [elapsed, setElapsed] = useState(0);

  // Tick while a test runs so the user can see it's still working — a stdio
  // server may take a while on first run (npx downloads the package).
  useEffect(() => {
    if (testing === null) {
      setElapsed(0);
      return;
    }
    const timer = setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => clearInterval(timer);
  }, [testing]);

  const toggleExpanded = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleTest = useCallback(async (id: string) => {
    setTesting(id);
    try {
      const result = await api.configHubMcpTest(id);
      setTestResults((prev) => ({ ...prev, [id]: result }));
    } catch (err) {
      setTestResults((prev) => ({
        ...prev,
        [id]: { ok: false, latencyMs: 0, message: String(err) },
      }));
    } finally {
      setTesting(null);
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      setServers(await api.configHubMcpList());
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to load MCP servers: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleToggle = useCallback(
    async (id: string, toolId: string, enabled: boolean) => {
      setBusyKey(`${id}@${toolId}`);
      setNotice(null);
      try {
        await api.configHubMcpToggle(id, toolId, enabled);
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusyKey(null);
      }
    },
    [refresh],
  );

  const openAdd = useCallback(() => {
    setEditor(
      mcpToEditor({
        id: '',
        name: '',
        server: { type: 'stdio', command: 'npx', args: ['-y', ''] },
        apps: { claude: false, codex: false, opencode: false },
      }),
    );
  }, []);

  const openEdit = useCallback((server: McpServer) => {
    setEditor(mcpToEditor(server));
  }, []);

  const handleSave = useCallback(
    async (state: McpEditorState) => {
      const id = state.id.trim() || state.name.trim();
      if (!id) return;
      setBusyKey(`save:${id}`);
      setNotice(null);
      try {
        await api.configHubMcpUpsert(
          id,
          state.name.trim() || id,
          state.description.trim() || null,
          editorToMcp(state),
          state.apps,
        );
        setNotice({ type: 'ok', text: `Saved ${id}` });
        setEditor(null);
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusyKey(null);
      }
    },
    [refresh],
  );

  const handleDelete = useCallback(async () => {
    if (!pendingDelete) return;
    setBusyKey(`delete:${pendingDelete.id}`);
    setNotice(null);
    try {
      await api.configHubMcpDelete(pendingDelete.id);
      setNotice({ type: 'ok', text: `Deleted ${pendingDelete.id}` });
      setPendingDelete(null);
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: String(err) });
    } finally {
      setBusyKey(null);
    }
  }, [pendingDelete, refresh]);

  const handleImport = useCallback(async () => {
    setBusyKey('import');
    setNotice(null);
    try {
      const count = await api.configHubMcpImport();
      setNotice({
        type: 'ok',
        text: count > 0 ? `Imported ${count} server(s)` : 'Nothing new to import',
      });
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: String(err) });
    } finally {
      setBusyKey(null);
    }
  }, [refresh]);

  return (
    <div>
      {/* Header row */}
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-[hsl(var(--border))]">
        <span className="text-[11px] text-muted-foreground">
          {servers.length} MCP server{servers.length === 1 ? '' : 's'} in the unified store
        </span>
        <div className="flex-1" />
        <button
          onClick={() => void handleImport()}
          disabled={busyKey !== null}
          title="Import from installed tools"
          className="inline-flex items-center gap-1 px-2 py-1 rounded text-[11px] border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
        >
          {busyKey === 'import' ? (
            <Loader2 className="size-3 animate-spin" />
          ) : (
            <Download className="size-3" />
          )}
          Import
        </button>
        <button
          onClick={() => void refresh()}
          disabled={loading}
          title="Rescan"
          className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-50"
        >
          <RefreshCw className={cn('size-3', loading && 'animate-spin')} />
        </button>
        <button
          onClick={openAdd}
          disabled={busyKey !== null}
          className="inline-flex items-center gap-1 px-2 py-1 rounded text-[11px] bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50"
        >
          <Plus className="size-3" />
          Add
        </button>
      </div>

      {notice && (
        <div
          className={cn(
            'mb-3 px-3 py-2 rounded-lg text-[11px] leading-relaxed break-all',
            notice.type === 'ok'
              ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
              : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
          )}
        >
          {notice.text}
        </div>
      )}

      {/* List */}
      <div className="divide-y divide-border/40">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin mr-2 inline" />
            Loading…
          </div>
        ) : servers.length === 0 ? (
          <div className="p-4 text-xs text-muted-foreground text-center">
            No MCP servers yet. Add one, or import from your installed tools.
          </div>
        ) : (
          servers.map((server) => {
            const enabledCount = MCP_TOOLS.filter((t) => server.apps[t]).length;
            const isOpen = expanded.has(server.id);
            return (
              <div key={server.id}>
                <div className="px-4 py-2 flex items-center gap-2">
                  <button
                    onClick={() => toggleExpanded(server.id)}
                    title={isOpen ? 'Hide JSON' : 'Show JSON'}
                    className="p-0.5 rounded text-muted-foreground hover:text-foreground shrink-0"
                  >
                    {isOpen ? (
                      <ChevronDown className="size-3" />
                    ) : (
                      <ChevronRight className="size-3" />
                    )}
                  </button>
                  <span className="text-[9px] uppercase tracking-wide rounded px-1 py-0.5 border shrink-0 border-sky-500/40 text-sky-600 dark:text-sky-400">
                    MCP
                  </span>

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs font-medium truncate">
                        {server.name || server.id}
                      </span>
                      {enabledCount > 0 ? (
                        <span className="text-[9px] text-muted-foreground border border-border/40 rounded px-1">
                          {enabledCount}/{MCP_TOOLS.length} tools
                        </span>
                      ) : (
                        <span className="text-[9px] text-muted-foreground border border-border/40 rounded px-1">
                          not enabled
                        </span>
                      )}
                      <span className="text-[9px] uppercase tracking-wide text-muted-foreground/70 border border-border/40 rounded px-1">
                        {server.server.type ?? 'stdio'}
                      </span>
                    </div>
                    <div className="text-[10px] text-muted-foreground font-mono truncate">
                      {describeSpec(server)}
                    </div>
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    {MCP_TOOLS.map((toolId) => {
                      const enabled = server.apps[toolId] ?? false;
                      const busy = busyKey === `${server.id}@${toolId}`;
                      return (
                        <button
                          key={toolId}
                          onClick={() => handleToggle(server.id, toolId, !enabled)}
                          disabled={busy || busyKey !== null}
                          aria-pressed={enabled}
                          title={`${TOOL_LABELS[toolId]}: ${enabled ? 'enabled' : 'disabled'}`}
                          className={cn(
                            'w-7 h-7 rounded-lg flex items-center justify-center transition-all disabled:opacity-50 shrink-0',
                            enabled
                              ? TOOL_ACTIVE_CLASSES[toolId] ?? 'bg-muted/40 text-foreground'
                              : 'opacity-40 hover:opacity-80',
                          )}
                        >
                          {busy ? (
                            <Loader2 className="size-3.5 animate-spin" />
                          ) : (
                            <ToolBadge toolId={toolId} className="size-3.5" />
                          )}
                        </button>
                      );
                    })}
                    {/* Row actions, grouped in a border so they read as one
                        cluster distinct from the per-tool enable switches. */}
                    <div className="ml-1 flex items-center gap-0.5 rounded-md border border-[hsl(var(--border))] p-0.5 shrink-0">
                      <button
                        onClick={() => void handleTest(server.id)}
                        disabled={testing !== null}
                        title="Test connection (runs a real MCP handshake)"
                        className="p-1 rounded text-muted-foreground hover:text-amber-600 hover:bg-amber-500/10 transition-colors disabled:opacity-50"
                      >
                        {testing === server.id ? (
                          <Loader2 className="size-3 animate-spin" />
                        ) : (
                          <Zap className="size-3" />
                        )}
                      </button>
                      <button
                        onClick={() => openEdit(server)}
                        title="Edit"
                        className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
                      >
                        <Pencil className="size-3" />
                      </button>
                      <button
                        onClick={() => setPendingDelete(server)}
                        title="Delete"
                        className="p-1 rounded text-red-500 hover:bg-red-500/10 transition-colors"
                      >
                        <Trash2 className="size-3" />
                      </button>
                    </div>
                  </div>
                </div>

                {testing === server.id && (
                  <div className="mx-4 mb-2 px-2 py-1 rounded text-[10px] bg-blue-50 text-blue-700 dark:bg-blue-900/20 dark:text-blue-300 flex items-center gap-2">
                    <Loader2 className="size-3 animate-spin shrink-0" />
                    <span>
                      Testing handshake… {elapsed}s
                      {elapsed >= 3 && (
                        <span className="opacity-70">
                          {' '}
                          (a stdio server may download its package on first run)
                        </span>
                      )}
                    </span>
                  </div>
                )}

                {testing !== server.id && testResults[server.id] && (
                  <div
                    className={cn(
                      'mx-4 mb-2 px-2 py-1 rounded text-[10px] break-all',
                      testResults[server.id].ok
                        ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
                        : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
                    )}
                  >
                    {testResults[server.id].ok ? '✓ ' : '✗ '}
                    {testResults[server.id].ok && testResults[server.id].serverName
                      ? `${testResults[server.id].serverName}${
                          testResults[server.id].serverVersion
                            ? ` v${testResults[server.id].serverVersion}`
                            : ''
                        } — `
                      : ''}
                    {testResults[server.id].message}
                    {testResults[server.id].latencyMs > 0 &&
                      ` (${testResults[server.id].latencyMs} ms)`}
                  </div>
                )}

                {isOpen && (
                  <pre className="mx-4 mb-2 text-[10px] leading-relaxed font-mono whitespace-pre-wrap break-all rounded bg-muted/30 p-2 max-h-64 overflow-auto">
                    {JSON.stringify(server.server, null, 2)}
                  </pre>
                )}
              </div>
            );
          })
        )}
      </div>

      {editor && (
        <McpEditorModal
          editor={editor}
          busy={busyKey !== null}
          onSave={handleSave}
          onCancel={() => setEditor(null)}
        />
      )}

      {pendingDelete && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
          <div className="w-full max-w-md rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg">
            <div className="text-sm font-medium mb-2">
              Delete MCP server “{pendingDelete.name || pendingDelete.id}”?
            </div>
            <p className="text-[11px] text-muted-foreground leading-relaxed mb-3">
              It will be removed from the unified store and from every tool that had it
              enabled.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPendingDelete(null)}
                disabled={busyKey !== null}
                className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={() => void handleDelete()}
                disabled={busyKey !== null}
                className="px-3 py-1 rounded text-xs bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-60"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function describeSpec(server: McpServer): string {
  const spec = server.server;
  if ((spec.type ?? 'stdio') === 'stdio') {
    const cmd = (spec.command as string) ?? '';
    const args = (spec.args as string[]) ?? [];
    return [cmd, ...args].join(' ');
  }
  return (spec.url as string) ?? 'http/sse server';
}