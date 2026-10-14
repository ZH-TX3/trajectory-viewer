// ── Config Hub: prompts & config panel ───────────────────────────────────
//
// Each tool's system prompt and config files. Files can be viewed and edited
// in place; saving goes through the backend, which only accepts paths the tool
// itself declares. A tool that isn't installed shows a plain notice instead of
// an empty box.

import { useCallback, useState } from 'react';
import { ChevronDown, ChevronRight, Pencil } from 'lucide-react';
import { MarkdownPane } from '../MarkdownPane';
import type { ConfigFileContent, ToolConfigs } from '../../types';
import { api } from '../../api';
import { cn } from '../../lib/utils';
import { useMarkdownEditor } from '../../utils/useMarkdownEditor';
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
  // A long file is clipped until expanded. Previewing a Markdown file expands
  // it on its own — a rendered document cut off after three lines reads as a
  // rendering bug rather than a clip — so for Markdown the expanded state is
  // simply derived rather than stored.
  const [expanded, setExpanded] = useState(false);
  const showExpanded = expanded;

  const editor = useMarkdownEditor({
    original,
    save: (content) => api.configHubSaveConfig(toolId, file.path, content),
    onSaved,
  });

  // Opening in VS Code is a user action with a real failure mode (no VS Code
  // installed, the file is locked), and it happens outside the edit flow, so it
  // keeps its own error slot rather than borrowing the editor's.
  const [openError, setOpenError] = useState<string | null>(null);
  const openInEditor = useCallback(async () => {
    try {
      await api.configHubOpenConfig(toolId, file.path);
      setOpenError(null);
    } catch (err) {
      setOpenError(String(err));
    }
  }, [toolId, file.path]);

  // JSON files get a format action; other formats (md/toml/yaml) have no
  // generic reformatter, so the button is hidden for them.
  const isJson = file.format === 'json';
  // A Markdown prompt (CLAUDE.md / AGENTS.md) can be previewed; the
  // other formats have no renderer here, so they stay raw.
  const isMarkdown = file.format === 'markdown';
  const formatDraft = useCallback(() => {
    try {
      editor.setDraft(JSON.stringify(JSON.parse(editor.draft), null, 2));
      editor.setError(null);
    } catch (err) {
      editor.setError(`Invalid JSON: ${String(err)}`);
    }
  }, [editor.draft, editor.setError, editor.setDraft]);

  return (
    // `text-[10px]` is the panel's base font size, and everything inside
    // inherits it — including a Markdown preview, whose headings and code are
    // sized in `em`.
    <div className="px-3 py-2 text-[10px]">
      <div className="flex items-center gap-2 mb-1">
        <span className="text-[11px] font-medium">{file.label}</span>
        <StatusBadge exists={file.exists} dirty={editor.dirty} editing={editor.editing} />

        <button
          onClick={() => void openInEditor()}
          title="Open in VS Code"
          className="ml-auto text-[10px] text-blue-600 dark:text-blue-400 hover:underline"
        >
          {file.path}
        </button>
      </div>

      {editor.error != null && (
        <div className="mb-1 px-2 py-1 rounded text-[10px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all">
          {editor.error}
        </div>
      )}

      {openError != null && (
        <div className="mb-1 px-2 py-1 rounded text-[10px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all">
          {openError}
        </div>
      )}

      {isMarkdown ? (
        /* A Markdown file is both previewable and editable, so it goes through
           the shared pane — one place decides where Preview and Edit sit, and
           the same buttons appear for a memory document. */
        <MarkdownPane
          source={original}
          editor={editor}
          editAction={
            editor.editing && isJson ? { label: 'format', onClick: formatDraft } : null
          }
          // One row per file, so the default stays compact: the whole config
          // list should be scannable, and asking for the render or the editor
          // is what buys more room. The editor matches this panel's other
          // formats, which use `h-[70vh]`.
          heights={{
            source: 'max-h-24',
            preview: 'max-h-96',
            editor: 'h-[70vh] min-h-64',
          }}
        />
      ) : (
        <>
          <div className="flex items-center gap-1 mb-1">
            {file.exists && original.length > 400 && (
              <button
                onClick={() => setExpanded((v) => !v)}
                className="text-[10px] text-blue-600 dark:text-blue-400 hover:underline"
              >
                {showExpanded ? 'collapse' : 'expand'}
              </button>
            )}
            <button
              onClick={editor.startEdit}
              title="Edit"
              className="ml-auto p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
            >
              <Pencil className="size-3" />
            </button>
          </div>

          {editor.editing ? (
            <textarea
              value={editor.draft}
              onChange={(e) => editor.setDraft(e.target.value)}
              spellCheck={false}
              className="w-full text-[10px] leading-relaxed font-mono bg-transparent border border-[hsl(var(--border))] rounded p-2 h-[70vh] min-h-64 resize-y focus:outline-none focus:ring-1 focus:ring-blue-400"
            />
          ) : file.exists ? (
            <pre
              className={cn(
                'text-[10px] leading-relaxed whitespace-pre-wrap break-all rounded bg-muted/30 p-2 overflow-auto',
                showExpanded ? 'max-h-96' : 'max-h-24',
              )}
            >
              {original || '(empty)'}
            </pre>
          ) : (
            <div className="text-[10px] text-muted-foreground">
              File does not exist. Click edit to create it.
            </div>
          )}
        </>
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
