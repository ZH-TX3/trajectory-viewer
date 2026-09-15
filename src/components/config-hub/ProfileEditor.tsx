// ── Config Hub: profile editor ───────────────────────────────────────────
//
// Create or edit a profile's contents directly: a grid of resources × tools
// where each cell is a checkbox. Starting from the live state (or an existing
// profile) is the usual path, but every cell can be set by hand — this is how
// a profile is authored, not just snapshotted.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { api } from '../../api';
import type { McpServer, ProfileDetail, ToolInfo } from '../../types';
import { KIND_LABELS, linkableTools } from '../../utils/configHub';
import { cn } from '../../lib/utils';
import { ToolBadge } from '../icons/BrandIcons';

interface ProfileEditorProps {
  /** Existing profile name, or null to create a new one. */
  profileName: string | null;
  tools: ToolInfo[];
  /** Resource id → display name, from the current scan. */
  resources: Array<{ id: string; name: string; kind: 'skill' | 'agent' }>;
  mcpServers: McpServer[];
  onSaved: () => void;
  onCancel: () => void;
}

/** toolId → enabled, for one resource. */
type ToolFlags = Record<string, boolean>;

export function ProfileEditor({
  profileName,
  tools,
  resources,
  mcpServers,
  onSaved,
  onCancel,
}: ProfileEditorProps) {
  const [name, setName] = useState(profileName ?? '');
  const [resourceFlags, setResourceFlags] = useState<Record<string, ToolFlags>>({});
  const [mcpFlags, setMcpFlags] = useState<Record<string, ToolFlags>>({});
  const [loading, setLoading] = useState(profileName !== null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Seed: an existing profile's entries, else the live state.
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        if (profileName) {
          const detail: ProfileDetail = await api.configHubProfileDetail(profileName);
          if (cancelled) return;
          setResourceFlags(entriesToFlags(detail.resources));
          setMcpFlags(entriesToFlags(detail.mcpServers));
        } else {
          // New profile starts from the live state, so the common case is
          // "tweak what's there".
          const live = await api.configHubSnapshot();
          if (cancelled) return;
          const res: Record<string, ToolFlags> = {};
          for (const r of live.resources) {
            res[r.id] = Object.fromEntries(
              Object.entries(r.apps).map(([tool, state]) => [
                tool,
                state === 'linked' || state === 'copied',
              ]),
            );
          }
          setResourceFlags(res);
          const mcp = await api.configHubMcpList();
          if (cancelled) return;
          setMcpFlags(
            Object.fromEntries(mcp.map((s) => [s.id, { ...s.apps }])),
          );
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [profileName]);

  const isNew = profileName === null;

  const toggleResource = useCallback((id: string, toolId: string) => {
    setResourceFlags((prev) => ({
      ...prev,
      [id]: { ...prev[id], [toolId]: !prev[id]?.[toolId] },
    }));
  }, []);

  const toggleMcp = useCallback((id: string, toolId: string) => {
    setMcpFlags((prev) => ({
      ...prev,
      [id]: { ...prev[id], [toolId]: !prev[id]?.[toolId] },
    }));
  }, []);

  const save = useCallback(async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Profile name is required');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.configHubUpsertProfile(
        trimmed,
        flagsToEntries(resourceFlags),
        flagsToEntries(mcpFlags),
      );
      onSaved();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, [name, resourceFlags, mcpFlags, onSaved]);

  const enabledCount = useMemo(
    () => Object.values(resourceFlags).reduce((n, f) => n + countOn(f), 0),
    [resourceFlags],
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-3xl rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg flex flex-col max-h-[92vh]">
        <div className="text-sm font-medium mb-3">
          {isNew ? 'New profile' : `Edit profile “${profileName}”`}
        </div>

        <label className="block mb-3">
          <span className="text-[10px] text-muted-foreground">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={!isNew}
            placeholder="e.g. frontend"
            className="mt-0.5 w-full text-xs bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400 disabled:opacity-50"
          />
        </label>

        <div className="flex items-center gap-2 mb-2">
          <span className="text-[10px] text-muted-foreground">
            {enabledCount} enablement{enabledCount === 1 ? '' : 's'} selected
          </span>
          <span className="text-[10px] text-muted-foreground/70">
            · tick a tool for each resource the profile should turn on
          </span>
        </div>

        {error && (
          <div className="mb-2 px-3 py-2 rounded-lg text-[11px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all">
            {error}
          </div>
        )}

        <div className="flex-1 overflow-auto rounded border border-[hsl(var(--border))]">
          {loading ? (
            <div className="p-4 text-xs text-muted-foreground">
              <Loader2 className="size-3 animate-spin mr-2 inline" />
              Loading…
            </div>
          ) : (
            <>
              {/* Skills / agents */}
              <div className="px-3 py-1.5 text-[9px] uppercase tracking-wide text-muted-foreground bg-muted/30 border-b border-[hsl(var(--border))]">
                Skills &amp; agents
              </div>
              {resources.length === 0 ? (
                <div className="px-3 py-2 text-[11px] text-muted-foreground">
                  No skills or agents found.
                </div>
              ) : (
                resources.map((resource) => {
                  const rowTools = linkableTools(tools, resource.kind);
                  const flags = resourceFlags[resource.id] ?? {};
                  return (
                    <div
                      key={resource.id}
                      className="px-3 py-1.5 flex items-center gap-2 border-b border-[hsl(var(--border))] last:border-b-0"
                    >
                      <span className="text-[9px] uppercase tracking-wide rounded px-1 border border-border/40 text-muted-foreground shrink-0">
                        {KIND_LABELS[resource.kind]}
                      </span>
                      <span className="text-xs truncate flex-1 min-w-0">{resource.name}</span>
                      <div className="flex items-center gap-1 shrink-0">
                        {rowTools.map((tool) => (
                          <ToolCheck
                            key={tool.id}
                            toolId={tool.id}
                            checked={flags[tool.id] ?? false}
                            onToggle={() => toggleResource(resource.id, tool.id)}
                          />
                        ))}
                      </div>
                    </div>
                  );
                })
              )}

              {/* MCP servers */}
              <div className="px-3 py-1.5 text-[9px] uppercase tracking-wide text-muted-foreground bg-muted/30 border-y border-[hsl(var(--border))]">
                MCP servers
              </div>
              {mcpServers.length === 0 ? (
                <div className="px-3 py-2 text-[11px] text-muted-foreground">
                  No MCP servers in the store.
                </div>
              ) : (
                mcpServers.map((server) => {
                  const flags = mcpFlags[server.id] ?? {};
                  return (
                    <div
                      key={server.id}
                      className="px-3 py-1.5 flex items-center gap-2 border-b border-[hsl(var(--border))] last:border-b-0"
                    >
                      <span className="text-[9px] uppercase tracking-wide rounded px-1 border border-sky-500/40 text-sky-600 dark:text-sky-400 shrink-0">
                        MCP
                      </span>
                      <span className="text-xs truncate flex-1 min-w-0">
                        {server.name || server.id}
                      </span>
                      <div className="flex items-center gap-1 shrink-0">
                        {MCP_TOOL_IDS.map((toolId) => (
                          <ToolCheck
                            key={toolId}
                            toolId={toolId}
                            checked={flags[toolId] ?? false}
                            onToggle={() => toggleMcp(server.id, toolId)}
                          />
                        ))}
                      </div>
                    </div>
                  );
                })
              )}
            </>
          )}
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <button
            onClick={onCancel}
            disabled={saving}
            className="px-3 py-1 rounded text-xs border border-[hsl(var(--border))] hover:bg-muted/40 transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={() => void save()}
            disabled={saving || loading || !name.trim()}
            className="inline-flex items-center gap-1.5 px-3 py-1 rounded text-xs bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-60"
          >
            {saving && <Loader2 className="size-3 animate-spin" />}
            Save profile
          </button>
        </div>
      </div>
    </div>
  );
}

const MCP_TOOL_IDS = ['claude', 'codex', 'opencode'];

/** One tool's checkbox, drawn as its brand icon. */
function ToolCheck({
  toolId,
  checked,
  onToggle,
}: {
  toolId: string;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      onClick={onToggle}
      aria-pressed={checked}
      title={toolId}
      className={cn(
        'w-6 h-6 rounded flex items-center justify-center transition-all border',
        checked
          ? 'border-blue-500/60 bg-blue-500/15'
          : 'border-[hsl(var(--border))] opacity-40 hover:opacity-70',
      )}
    >
      <ToolBadge toolId={toolId} className="size-3" />
    </button>
  );
}

function countOn(flags: ToolFlags): number {
  return Object.values(flags).filter(Boolean).length;
}

/** `{ id: ["claude"] }` → `{ id: { claude: true } }`. */
function entriesToFlags(entries: Record<string, string[]>): Record<string, ToolFlags> {
  return Object.fromEntries(
    Object.entries(entries).map(([id, tools]) => [
      id,
      Object.fromEntries(tools.map((t) => [t, true])),
    ]),
  );
}

/** `{ id: { claude: true } }` → `{ id: ["claude"] }` (off cells dropped). */
function flagsToEntries(flags: Record<string, ToolFlags>): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const [id, perTool] of Object.entries(flags)) {
    const on = Object.entries(perTool)
      .filter(([, v]) => v)
      .map(([tool]) => tool);
    if (on.length > 0) out[id] = on;
  }
  return out;
}
