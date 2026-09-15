// ── Config Hub: prompts & config panel ───────────────────────────────────
//
// Each tool's system prompt and config files. Files can be viewed and edited
// in place; saving goes through the backend, which only accepts paths the tool
// itself declares. A tool that isn't installed shows a plain notice instead of
// an empty box.

import { useCallback, useState } from 'react';
import { Check, ChevronDown, ChevronRight, Loader2, Pencil, RotateCcw, X } from 'lucide-react';
import type { ConfigFileContent, ToolConfigs } from '../../types';
import { api } from '../../api';
import { cn } from '../../lib/utils';
import { ToolIcon } from '../icons/BrandIcons';

interface ConfigPanelProps {
  configs: ToolConfigs[];
  onSaved: () => void;
}

export function ConfigReadonlyPanel({ configs, onSaved }: ConfigPanelProps) {
  return (
    <div className="space-y-2">
      <div className="text-[11px] text-muted-foreground">
        Each tool's system prompt and config files. Editing writes straight to the file.
      </div>
      {configs.map((tool) => (
        <ToolConfigCard key={tool.toolId} tool={tool} onSaved={onSaved} />
      ))}
    </div>
  );
}

function ToolConfigCard({ tool, onSaved }: { tool: ToolConfigs; onSaved: () => void }) {
  const [open, setOpen] = useState(false);
  const files: ConfigFileContent[] = [...(tool.prompt ? [tool.prompt] : []), ...tool.files];

  return (
    <div className="rounded-lg border border-[hsl(var(--border))]">
      <button
        onClick={() => setOpen((v) => !v)}
        disabled={!tool.installed}
        className="w-full flex items-center gap-2 px-3 py-2 text-left disabled:cursor-default"
      >
        {open ? (
          <ChevronDown className="size-3 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3 text-muted-foreground" />
        )}
        <ToolIcon toolId={tool.toolId} className="size-3.5" />
        <span className="text-xs font-medium">{tool.displayName}</span>
        <span className="ml-auto text-[10px] text-muted-foreground">
          {tool.installed
            ? `${files.filter((f) => f.exists).length}/${files.length} files`
            : 'not installed'}
        </span>
      </button>

      {open && (
        <div className="border-t border-[hsl(var(--border))] divide-y divide-[hsl(var(--border))]">
          {!tool.installed ? (
            <div className="px-3 py-2 text-[11px] text-muted-foreground">
              {tool.displayName} is not installed — nothing to show.
            </div>
          ) : (
            files.map((file) => (
              <FileBlock key={file.path} toolId={tool.toolId} file={file} onSaved={onSaved} />
            ))
          )}
        </div>
      )}
    </div>
  );
}

function FileBlock({
  toolId,
  file,
  onSaved,
}: {
  toolId: string;
  file: ConfigFileContent;
  onSaved: () => void;
}) {
  const original = file.content ?? '';
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(original);
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startEdit = useCallback(() => {
    setDraft(original);
    setError(null);
    setEditing(true);
  }, [original]);

  const cancel = useCallback(() => {
    setEditing(false);
    setDraft(original);
    setError(null);
  }, [original]);

  const save = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      await api.configHubSaveConfig(toolId, file.path, draft);
      setEditing(false);
      onSaved();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, [toolId, file.path, draft, onSaved]);

  const openInEditor = useCallback(async () => {
    setError(null);
    try {
      await api.configHubOpenConfig(toolId, file.path);
    } catch (err) {
      setError(String(err));
    }
  }, [toolId, file.path]);

  const dirty = draft !== original;

  // JSON files get a format action; other formats (md/toml/yaml) have no
  // generic reformatter, so the button is hidden for them.
  const isJson = file.format === 'json';
  const formatDraft = useCallback(() => {
    try {
      setDraft(JSON.stringify(JSON.parse(draft), null, 2));
      setError(null);
    } catch (err) {
      setError(`Invalid JSON: ${String(err)}`);
    }
  }, [draft]);

  return (
    <div className="px-3 py-2">
      <div className="flex items-center gap-2 mb-1">
        <span className="text-[11px] font-medium">{file.label}</span>
        <StatusBadge exists={file.exists} dirty={dirty} editing={editing} />

        <div className="ml-auto flex items-center gap-1">
          {editing ? (
            <>
              {isJson && (
                <button
                  onClick={formatDraft}
                  disabled={saving}
                  title="Format JSON"
                  className="text-[10px] text-blue-600 dark:text-blue-400 hover:underline disabled:opacity-50"
                >
                  format
                </button>
              )}
              <button
                onClick={() => void save()}
                disabled={saving || !dirty}
                title="Save"
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50"
              >
                {saving ? <Loader2 className="size-3 animate-spin" /> : <Check className="size-3" />}
                Save
              </button>
              <button
                onClick={() => setDraft(original)}
                disabled={saving || !dirty}
                title="Revert changes"
                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-40"
              >
                <RotateCcw className="size-3" />
              </button>
              <button
                onClick={cancel}
                disabled={saving}
                title="Cancel"
                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-40"
              >
                <X className="size-3" />
              </button>
            </>
          ) : (
            <>
              {file.exists && file.content && file.content.length > 400 && (
                <button
                  onClick={() => setExpanded((v) => !v)}
                  className="text-[10px] text-blue-600 dark:text-blue-400 hover:underline"
                >
                  {expanded ? 'collapse' : 'expand'}
                </button>
              )}
              <button
                onClick={startEdit}
                title="Edit"
                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
              >
                <Pencil className="size-3" />
              </button>
            </>
          )}
        </div>
      </div>

      <button
        onClick={() => void openInEditor()}
        title="Open in VS Code"
        className="block w-full text-left text-[10px] text-blue-600 dark:text-blue-400 font-mono break-all mb-1 hover:underline"
      >
        {file.path}
      </button>

      {error && (
        <div className="mb-1 px-2 py-1 rounded text-[10px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all">
          {error}
        </div>
      )}

      {editing ? (
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          spellCheck={false}
          className="w-full text-[10px] leading-relaxed font-mono bg-transparent border border-[hsl(var(--border))] rounded p-2 h-[70vh] min-h-64 resize-y focus:outline-none focus:ring-1 focus:ring-blue-400"
        />
      ) : file.exists ? (
        <pre
          className={cn(
            'text-[10px] leading-relaxed whitespace-pre-wrap break-all rounded bg-muted/30 p-2 overflow-auto',
            expanded ? 'max-h-96' : 'max-h-24',
          )}
        >
          {original || '(empty)'}
        </pre>
      ) : (
        <div className="text-[10px] text-muted-foreground">
          File does not exist. Click edit to create it.
        </div>
      )}
    </div>
  );
}

/** Small status chip: editing / unsaved / missing / editable. */
function StatusBadge({
  exists,
  dirty,
  editing,
}: {
  exists: boolean;
  dirty: boolean;
  editing: boolean;
}) {
  const label = editing ? (dirty ? 'unsaved' : 'editing') : exists ? 'editable' : 'missing';
  return (
    <span
      className={cn(
        'text-[9px] uppercase tracking-wide rounded px-1 py-0.5 border',
        editing
          ? dirty
            ? 'border-amber-500/50 text-amber-600 dark:text-amber-400'
            : 'border-blue-500/50 text-blue-600 dark:text-blue-400'
          : exists
            ? 'border-blue-500/40 text-blue-600 dark:text-blue-400'
            : 'border-border/50 text-muted-foreground',
      )}
    >
      {label}
    </span>
  );
}
