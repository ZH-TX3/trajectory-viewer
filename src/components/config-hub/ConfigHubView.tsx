// ── Config Hub: main view ────────────────────────────────────────────────
//
// Four tabs manage the tool configuration: Prompts & config (read-only),
// Skills, Agents and MCP. Skills and agents live once in `~/.agents` and are
// linked into each tool; MCP servers live in a unified store and are merged
// into each tool's own config.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Loader2, RefreshCw } from 'lucide-react';
import { api } from '../../api';
import type { ConfigResource, HubSnapshot, ImportPreview, ToolInfo } from '../../types';
import { filterResources, hasDrift, presentKinds } from '../../utils/configHub';
import { cn } from '../../lib/utils';
import { ResourceList } from './ResourceList';
import { ImportPreviewDialog } from './ImportPreviewDialog';
import { DeleteConfirmDialog } from './DeleteConfirmDialog';
import { ConfigReadonlyPanel } from './ConfigReadonlyPanel';
import { McpPanel } from './McpPanel';

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

type HubTab = 'prompts' | 'skill' | 'mcp' | 'agent';

const TABS: Array<{ value: HubTab; label: string }> = [
  { value: 'prompts', label: 'Prompts & config' },
  { value: 'skill', label: 'Skills' },
  { value: 'mcp', label: 'MCP' },
  { value: 'agent', label: 'Agents' },
];

export function ConfigHubView() {
  const [snapshot, setSnapshot] = useState<HubSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);

  const [tab, setTab] = useState<HubTab>('prompts');
  const [toolId, setToolId] = useState<string>('all');
  const [driftedOnly, setDriftedOnly] = useState(false);

  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [pendingDelete, setPendingDelete] = useState<ConfigResource | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.configHubSnapshot());
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to scan: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const tools = snapshot?.tools ?? [];
  const resources = snapshot?.resources ?? [];

  const linkTools = useMemo(
    () => tools.filter((t) => t.installed && t.linkableKinds.length > 0),
    [tools],
  );

  const inLinkTab = tab === 'skill' || tab === 'agent';

  const filtered = useMemo(
    () =>
      filterResources(resources, {
        kind: inLinkTab ? tab : 'all',
        toolId,
        driftedOnly,
      }),
    [resources, tab, inLinkTab, toolId, driftedOnly],
  );

  const driftCount = useMemo(() => resources.filter(hasDrift).length, [resources]);

  const handleToggle = useCallback(
    async (resource: ConfigResource, target: string, enabled: boolean) => {
      setBusyKey(`${resource.id}@${target}`);
      setNotice(null);
      try {
        await api.configHubToggle(resource.id, target, enabled);
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusyKey(null);
      }
    },
    [refresh],
  );

  const openImport = useCallback(async (resource: ConfigResource, target: string) => {
    setNotice(null);
    try {
      setImportPreview(await api.configHubImportPreview(resource.id, target));
    } catch (err) {
      setNotice({ type: 'error', text: String(err) });
    }
  }, []);

  const confirmImport = useCallback(
    async (overwrite: boolean) => {
      if (!importPreview) return;
      setBusyKey(`${importPreview.kind}:${importPreview.name}@${importPreview.toolId}`);
      try {
        await api.configHubImport(
          `${importPreview.kind}:${importPreview.name}`,
          importPreview.toolId,
          overwrite,
        );
        setNotice({
          type: 'ok',
          text: overwrite ? 'Replaced and linked' : 'Imported into the unified store',
        });
        setImportPreview(null);
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusyKey(null);
      }
    },
    [importPreview, refresh],
  );

  const confirmDelete = useCallback(async () => {
    if (!pendingDelete) return;
    setBusyKey(pendingDelete.id);
    try {
      await api.configHubDelete(pendingDelete.id);
      setNotice({ type: 'ok', text: `Moved ${pendingDelete.name} to the trash` });
      setPendingDelete(null);
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: String(err) });
    } finally {
      setBusyKey(null);
    }
  }, [pendingDelete, refresh]);

  const kinds = presentKinds(resources);

  return (
    <div className="h-full overflow-auto">
      <div className="max-w-4xl mx-auto px-4 py-4">
        {/* Header */}
        <div className="flex items-center gap-2 mb-1">
          <span className="text-[11px] text-muted-foreground">
            {resources.length} resources
            {driftCount > 0 && ` · ${driftCount} need importing`}
          </span>
          <button
            onClick={() => void refresh()}
            disabled={loading}
            title="Rescan"
            className="ml-auto p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-50"
          >
            <RefreshCw className={cn('size-3.5', loading && 'animate-spin')} />
          </button>
        </div>
        <p className="text-[11px] text-muted-foreground mb-3">
          {tab === 'prompts'
            ? 'Each tool’s system prompt and config files, shown read-only for now.'
            : tab === 'mcp'
              ? 'MCP servers are managed from a unified store and merged into each tool’s config.'
              : tab === 'agent'
                ? 'Agents live once in ~/.agents and are linked into the tools that support them.'
                : 'Skills live once in ~/.agents and are linked into each tool, so one edit reaches all of them.'}
        </p>

        {/* Tabs */}
        <div className="flex items-center gap-2 flex-wrap mb-3">
          <div className="flex items-center gap-0.5 rounded-md border border-border/60 bg-muted/30 p-0.5">
            {TABS.map((option) => (
              <button
                key={option.value}
                onClick={() => setTab(option.value)}
                aria-pressed={tab === option.value}
                className={cn(
                  'px-2.5 py-0.5 rounded text-[10px] transition-colors',
                  tab === option.value
                    ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 font-medium shadow-sm ring-1 ring-blue-300 dark:ring-blue-700'
                    : 'text-muted-foreground hover:text-foreground',
                )}
              >
                {option.label}
              </button>
            ))}
          </div>

          {inLinkTab && (
            <>
              <select
                value={toolId}
                onChange={(e) => setToolId(e.target.value)}
                className="text-[10px] bg-transparent border border-border/60 rounded px-1.5 py-1"
              >
                <option value="all">All tools</option>
                {linkTools.map((tool) => (
                  <option key={tool.id} value={tool.id}>
                    {tool.displayName}
                  </option>
                ))}
              </select>

              <button
                onClick={() => setDriftedOnly((v) => !v)}
                aria-pressed={driftedOnly}
                className={cn(
                  'flex items-center gap-1 px-2.5 py-1 rounded text-[10px] border transition-colors',
                  driftedOnly
                    ? 'border-amber-500/60 bg-amber-500/15 text-amber-700 dark:text-amber-400 font-medium'
                    : 'border-border/60 text-muted-foreground hover:text-foreground',
                )}
              >
                <AlertTriangle className="size-3" />
                Needs import only
              </button>
            </>
          )}
        </div>

        {/* Notice */}
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

        {/* Skills / Agents resource list */}
        {inLinkTab && (
          <div className="rounded-lg border border-border/40 overflow-hidden mb-6">
            {loading ? (
              <div className="p-4 text-xs text-muted-foreground">
                <Loader2 className="size-3 animate-spin mr-2 inline" />
                Scanning…
              </div>
            ) : (
              <ResourceList
                resources={filtered}
                tools={tools}
                busyKey={busyKey}
                onToggle={handleToggle}
                onImport={openImport}
                onDelete={setPendingDelete}
              />
            )}
          </div>
        )}

        {tab === 'mcp' && (
          <div className="rounded-lg border border-[hsl(var(--border))] overflow-hidden mb-6">
            <McpPanel />
          </div>
        )}

        {/* Prompts & config tab (read-only) */}
        {tab === 'prompts' && snapshot && <ConfigReadonlyPanel configs={snapshot.configs} />}

        {/* Not-installed warning */}
        {!loading && tools.some((t) => !t.installed) && (
          <div className="mb-4 flex items-start gap-2 text-[11px] text-muted-foreground">
            <AlertTriangle className="size-3 mt-0.5 shrink-0" />
            <span>
              Not installed:{' '}
              {tools
                .filter((t) => !t.installed)
                .map((t) => t.displayName)
                .join(', ')}
              . They are skipped by every scan and switch.
            </span>
          </div>
        )}

        {inLinkTab && kinds.length === 0 && !loading && (
          <div className="text-[11px] text-muted-foreground">
            No skills or agents found in <code className="font-mono">~/.agents</code> or any tool
            directory.
          </div>
        )}
      </div>

      {importPreview && (
        <ImportPreviewDialog
          preview={importPreview}
          busy={busyKey !== null}
          onConfirm={confirmImport}
          onCancel={() => setImportPreview(null)}
        />
      )}

      {pendingDelete && (
        <DeleteConfirmDialog
          resource={pendingDelete}
          linkedTools={linkedToolNames(pendingDelete, tools)}
          busy={busyKey !== null}
          onConfirm={confirmDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}

/** Names of the tools currently linking or copying a resource. */
function linkedToolNames(resource: ConfigResource, tools: ToolInfo[]): string[] {
  return tools
    .filter((tool) => {
      const state = resource.apps[tool.id];
      return state === 'linked' || state === 'copied';
    })
    .map((tool) => tool.displayName);
}
