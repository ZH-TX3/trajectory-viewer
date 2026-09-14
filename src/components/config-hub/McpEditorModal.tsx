// ── Config Hub: MCP server editor modal ──────────────────────────────────
//
// Two ways to edit one server, like cc-switch: structured fields up top
// (transport type, command/args/env or url/headers) and the raw JSON spec
// below. The fields drive the JSON; editing the JSON re-parses it into the
// fields, so the two views stay in step. Per-tool toggles decide which tools'
// live config the server gets merged into.

import { useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { McpEditorState, McpServerType } from '../../types';
import {
  TOOL_ACTIVE_CLASSES,
  editorToJson,
  editorToMcp,
  jsonToEditor,
} from '../../utils/configHub';
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
const TOOL_LABELS: Record<string, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
};

export function McpEditorModal({ editor, busy, onSave, onCancel }: McpEditorModalProps) {
  const [draft, setDraft] = useState<McpEditorState>(editor);
  const [json, setJson] = useState(() => editorToJson(editor));
  const [jsonError, setJsonError] = useState<string | null>(null);
  const isNew = !editor.id && !editor.name;

  const editFields = (patch: Partial<McpEditorState>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    // Fields are the source of truth; keep the JSON view in sync.
    setJson(editorToJson(next));
    setJsonError(null);
  };

  const onJsonChange = (text: string) => {
    setJson(text);
    const result = jsonToEditor(text, draft);
    if ('error' in result) {
      setJsonError(result.error);
    } else {
      setJsonError(null);
      setDraft(result.editor);
    }
  };

  // Re-indent the JSON view; parse first so invalid text reports instead of
  // being silently reformatted.
  const formatJson = () => {
    const result = jsonToEditor(json, draft);
    if ('error' in result) {
      setJsonError(result.error);
      return;
    }
    setJsonError(null);
    setDraft(result.editor);
    setJson(JSON.stringify(editorToMcp(result.editor), null, 2));
  };

  // Format the JSON view on open.
  useEffect(() => {
    setJson(editorToJson(editor));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const canSave = (isNew ? draft.name.trim().length > 0 : draft.id.trim().length > 0) && !jsonError;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="w-full max-w-2xl rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg overflow-y-auto max-h-[92vh]">
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
                onChange={(e) => editFields({ name: e.target.value })}
                placeholder={isNew ? 'server-id' : draft.id}
                className="mt-0.5 w-full text-xs bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
              />
            </label>
            <label className="block">
              <span className="text-[10px] text-muted-foreground">
                {isNew ? 'ID (save-as key)' : 'ID (unchanged)'}
              </span>
              <input
                value={draft.id}
                onChange={(e) => editFields({ id: e.target.value })}
                disabled={!isNew}
                placeholder="unique-id"
                className="mt-0.5 w-full text-xs bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400 disabled:opacity-50"
              />
            </label>
          </div>

          <label className="block">
            <span className="text-[10px] text-muted-foreground">Description (optional)</span>
            <input
              value={draft.description}
              onChange={(e) => editFields({ description: e.target.value })}
              className="mt-0.5 w-full text-xs bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
            />
          </label>

          {/* Transport type */}
          <div>
            <span className="text-[10px] text-muted-foreground">Transport</span>
            <div className="mt-0.5 flex items-center gap-0.5 rounded-md border border-[hsl(var(--border))] bg-muted/30 p-0.5 w-fit">
              {TYPES.map((t) => (
                <button
                  key={t.value}
                  onClick={() => editFields({ type: t.value })}
                  aria-pressed={draft.type === t.value}
                  className={cn(
                    'px-2.5 py-0.5 rounded text-[10px] transition-colors',
                    draft.type === t.value
                      ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 font-medium shadow-sm ring-1 ring-blue-300 dark:ring-blue-700'
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
                  onChange={(e) => editFields({ command: e.target.value })}
                  placeholder="npx"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Args (comma-separated)
                </span>
                <input
                  value={draft.args}
                  onChange={(e) => editFields({ args: e.target.value })}
                  placeholder="-y, @modelcontextprotocol/server-time"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Env (KEY=VALUE, one per line)
                </span>
                <textarea
                  value={draft.env}
                  onChange={(e) => editFields({ env: e.target.value })}
                  rows={2}
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400 resize-y"
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
                  onChange={(e) => editFields({ url: e.target.value })}
                  placeholder="https://example.com/mcp"
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-muted-foreground">
                  Headers (KEY=VALUE, one per line)
                </span>
                <textarea
                  value={draft.headers}
                  onChange={(e) => editFields({ headers: e.target.value })}
                  rows={2}
                  className="mt-0.5 w-full text-xs font-mono bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400 resize-y"
                />
              </label>
            </>
          )}

          {/* Raw JSON view — same data, editable both ways */}
          <div>
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-muted-foreground">
                JSON (editing this updates the fields above)
              </span>
              <button
                onClick={formatJson}
                className="ml-auto text-[10px] text-blue-600 dark:text-blue-400 hover:underline"
              >
                format
              </button>
            </div>
            <textarea
              value={json}
              onChange={(e) => onJsonChange(e.target.value)}
              spellCheck={false}
              rows={8}
              className={cn(
                'mt-0.5 w-full text-[11px] leading-relaxed font-mono bg-transparent border rounded p-2 resize-y focus:outline-none focus:ring-1 focus:ring-blue-400',
                jsonError ? 'border-red-500/60' : 'border-[hsl(var(--border))]',
              )}
            />
            {jsonError && (
              <div className="mt-1 px-2 py-1 rounded text-[10px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all">
                {jsonError}
              </div>
            )}
          </div>

          {/* per-tool toggles */}
          <div>
            <span className="text-[10px] text-muted-foreground">Enabled for</span>
            <div className="mt-1 flex items-center gap-1 flex-wrap">
              {MCP_TOOLS.map((toolId) => {
                const enabled = draft.apps[toolId] ?? false;
                return (
                  <button
                    key={toolId}
                    onClick={() => editFields({ apps: { ...draft.apps, [toolId]: !enabled } })}
                    aria-pressed={enabled}
                    className={cn(
                      'h-7 px-2 rounded-lg flex items-center justify-center transition-all',
                      enabled
                        ? TOOL_ACTIVE_CLASSES[toolId] ?? 'bg-muted/40 text-foreground'
                        : 'opacity-40 hover:opacity-80',
                    )}
                  >
                    <ToolBadge toolId={toolId} label={TOOL_LABELS[toolId]} className="size-3.5" />
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
            className="px-3 py-1 rounded text-xs border border-[hsl(var(--border))] hover:bg-muted/40 transition-colors disabled:opacity-50"
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

/** Re-exported for callers that only need the canonical spec. */
export { editorToMcp };
