// ── Config Hub: MCP server editor modal ──────────────────────────────────
//
// Single form for adding or editing one MCP server in the unified store. The
// fields switch with the transport type: stdio gets command/args/env,
// http/sse get url/headers. Per-tool toggles decide where it gets written.

import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { McpEditorState, McpServerType } from '../../types';
import { TOOL_ACTIVE_CLASSES } from '../../utils/configHub';
import { cn } from '../../lib/utils';
import { ToolBadge } from '../icons/BrandIcons';

interface McpEditorModalProps {
  editor: McpEditorState;
  busy: boolean;
  onSave: (editor: McpEditorState) => void;
  onCancel: () => void;
}

const TYPES: Array<{ value: McpServerType; label: string }> = [
  { value: 'stdio', label: 'stdio' },
  { value: 'http', label: 'http' },
  { value: 'sse', label: 'sse' },
];

const MCP_TOOLS = ['claude', 'codex', 'opencode'];

export function McpEditorModal({ editor, busy, onSave, onCancel }: McpEditorModalProps) {
  const [draft, setDraft] = useState<McpEditorState>(editor);
  const isNew = !editor.id && !editor.name;

  const edit = (patch: Partial<McpEditorState>) => setDraft((e) => ({ ...e, ...patch }));

  const canSave = isNew ? draft.name.trim().length > 0 : draft.id.trim().length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-lg rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg overflow-y-auto max-h-[92vh]">
        <div className="text-sm font-medium mb-3">
          {isNew ? 'Add MCP server' : `Edit MCP server ${draft.name || draft.id}`}
        </div>

        <div className="space-y-3">
          {/* Name / id */}
          <div className="grid grid-cols-2 gap-3">
            <label className="block">
              <span className="text-[10px] text-muted-foreground">Name</span>
              <input
                value={draft.name}
                onChange={(e) => edit({ name: e.target.value })}
                placeholder={isNew ? 'server-id' : draft.id}
                className="mt-0.5 w-full text-xs bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
              />
            </label>
            <label className="block">
              <span className="text-[10px] text-muted-foreground">
                {isNew ? 'ID (save-as key)' : 'ID (unchanged)'}
              </span>
              <input
                value={draft.id}
                onChange={(e) => edit({ id: e.target.value })}
                disabled={!isNew}
                placeholder="unique-id"
                className="mt-0.5 w-full text-xs bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring disabled:opacity-50"
              />
            </label>
          </div>

          <label className="block">
            <span className="text-[10px] text-muted-foreground">Description (optional)</span>
            <input
              value={draft.description}
              onChange={(e) => edit({ description: e.target.value })}
              className="mt-0.5 w-full text-xs bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </label>

          {/* Transport type */}
          <div>
            <span className="text-[10px] text-muted-foreground">Transport</span>
            <div className="mt-0.5 flex items-center gap-0.5 rounded-md border border-border/60 bg-muted/30 p-0.5 w-fit">
              {TYPES.map((t) => (
                <button
                  key={t.value}
                  onClick={() => edit({ type: t.value })}
                  aria-pressed={draft.type === t.value}
                  className={cn(
                    'px-2.5 py-0.5 rounded text-[10px] transition-colors',
                    draft.type === t.value
                      ? 'bg-background text-foreground font-medium shadow-sm ring-1 ring-border'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {t.label}
                </button>
              ))}
            </div>
          </div>

          {/* stdio fields */}
          {draft.type === 'stdio' && (
            <>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">Command</span>
                <input
                  value={draft.command}
                  onChange={(e) => edit({ command: e.target.value })}
                  placeholder="npx"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Args (space-separated)
                </span>
                <input
                  value={draft.args}
                  onChange={(e) => edit({ args: e.target.value })}
                  placeholder="-y @modelcontextprotocol/server-time"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Env (KEY=VALUE, one per line)
                </span>
                <textarea
                  value={draft.env}
                  onChange={(e) => edit({ env: e.target.value })}
                  rows={2}
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring resize-y"
                />
              </label>
            </>
          )}

          {/* http / sse fields */}
          {draft.type !== 'stdio' && (
            <>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">URL</span>
                <input
                  value={draft.url}
                  onChange={(e) => edit({ url: e.target.value })}
                  placeholder="https://example.com/mcp"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Headers (KEY=VALUE, one per line)
                </span>
                <textarea
                  value={draft.headers}
                  onChange={(e) => edit({ headers: e.target.value })}
                  rows={2}
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-border/60 rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring resize-y"
                />
              </label>
            </>
          )}

          {/* per-tool toggles */}
          <div>
            <span className="text-[10px] text-muted-foreground">Enabled for</span>
            <div className="mt-1 flex items-center gap-1 flex-wrap">
              {MCP_TOOLS.map((toolId) => {
                const enabled = draft.apps[toolId] ?? false;
                return (
                  <button
                    key={toolId}
                    onClick={() => edit({ apps: { ...draft.apps, [toolId]: !enabled } })}
                    aria-pressed={enabled}
                    className={cn(
                      'h-7 px-2 rounded-lg flex items-center justify-center transition-all',
                      enabled
                        ? TOOL_ACTIVE_CLASSES[toolId] ?? 'bg-muted/40 text-foreground'
                        : 'opacity-40 hover:opacity-80',
                    )}
                  >
                    <ToolBadge toolId={toolId} label={toolLabel(toolId)} className="size-3.5" />
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <button
            onClick={onCancel}
            disabled={busy}
            className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={() => onSave(draft)}
            disabled={busy || !canSave}
            className="inline-flex items-center gap-1.5 px-3 py-1 rounded text-xs bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-60"
          >
            {busy && <Loader2 className="size-3 animate-spin" />}
            Save
          </button>
        </div>
      </div>
    </div>
  );
}

function toolLabel(toolId: string): string {
  return toolId === 'claude'
    ? 'Claude Code'
    : toolId === 'codex'
      ? 'Codex'
      : toolId === 'opencode'
        ? 'OpenCode'
        : toolId;
}