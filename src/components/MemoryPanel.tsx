// ── Project memory ───────────────────────────────────────────────────────
//
// Claude keeps per-project memory as Markdown beside the session files:
//
//   ~/.claude/projects/{project}/memory/MEMORY.md
//   ~/.claude/projects/{project}/memory/{name}.md
//
// In the sidebar this mirrors that layout — the project group gains a
// `memory` node listing the documents, above its sessions. Clicking one opens
// it in the right panel, the same way clicking a session opens its messages.
//
// The node is rendered only when the directory exists: most projects never
// write one.

import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, FileText, FolderOpen, Star } from 'lucide-react';
import { api } from '../api';
import { cn } from '../lib/utils';
import type { MemoryDoc } from '../types';
import { MarkdownPane } from './MarkdownPane';
import { useMarkdownEditor } from '../utils/useMarkdownEditor';

/** Colour per frontmatter `type`, matching the app's existing palette. */
const KIND_STYLES: Record<string, string> = {
  feedback: 'bg-cyan-100 dark:bg-cyan-900/30 text-cyan-700 dark:text-cyan-300',
  project: 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300',
  reference: 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300',
};

/** A memory document together with the project it came from. */
export interface OpenMemory {
  doc: MemoryDoc;
  /** The project's display path, for the viewer header. */
  projectPath: string;
  /**
   * Any session file of the project. The memory directory is a sibling of the
   * session files, so this is what the backend needs to find it again when the
   * document is saved.
   */
  sourcePath: string;
}

/**
 * The `memory` node for one project group, in the sidebar.
 *
 * A document's name is bold — the file name is in `text-muted-foreground`
 * below it, and without the weight the two lines read as one run-on row.
 *
 * `sourcePath` is any session file in the group — the memory directory is a
 * sibling of the session files, so they all resolve to the same one.
 */
export function MemoryTree({
  sourcePath,
  projectPath,
  selectedFileName,
  onOpen,
}: {
  sourcePath: string;
  projectPath: string;
  selectedFileName?: string | null;
  onOpen: (memory: OpenMemory) => void;
}) {
  const [docs, setDocs] = useState<MemoryDoc[]>([]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await api.getProjectMemories(sourcePath);
        if (!cancelled) setDocs(list);
      } catch {
        // No memory directory is the common case, not an error.
        if (!cancelled) setDocs([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sourcePath]);

  if (docs.length === 0) return null;

  return (
    <div>
      {/* The memory directory row */}
      <div
        onClick={() => setOpen((v) => !v)}
        className="group flex items-center gap-1.5 pl-5 pr-2 py-1 cursor-pointer hover:bg-muted/50 transition-colors"
      >
        {open ? (
          <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
        )}
        <FolderOpen className="size-3.5 shrink-0 text-amber-600 dark:text-amber-500" />
        <span className="text-[11px] font-medium truncate">memory</span>
        <span className="text-[10px] opacity-60 ml-auto">{docs.length}</span>
      </div>

      {open && docs.map((doc) => {
        const isSelected = doc.fileName === selectedFileName;
        return (
          <div
            key={doc.fileName}
            onClick={() => onOpen({ doc, projectPath, sourcePath })}
            title={doc.description ?? doc.fileName}
            className={cn(
              'flex items-start gap-1.5 pl-9 pr-2 py-1 cursor-pointer transition-colors',
              isSelected
                ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-900 dark:text-blue-100 border-l-[3px] border-blue-500 dark:border-blue-400'
                : 'hover:bg-muted/50 border-l-2 border-transparent',
            )}
          >
            {doc.isIndex ? (
              <Star className="size-3 mt-px shrink-0 text-amber-500" />
            ) : (
              <FileText className="size-3 mt-px shrink-0 text-muted-foreground" />
            )}
            <span className="flex-1 min-w-0">
              <span className="flex items-center gap-1 min-w-0">
                <span className="text-[10px] font-semibold truncate">
                  {doc.name ?? doc.fileName.replace(/\.md$/i, '')}
                </span>
                {doc.kind != null && (
                  <span
                    className={cn(
                      'shrink-0 px-1 rounded text-[8px] font-mono leading-4',
                      KIND_STYLES[doc.kind] ?? 'bg-muted text-muted-foreground',
                    )}
                  >
                    {doc.kind}
                  </span>
                )}
              </span>
              {doc.description != null && (
                <span className="block text-[9px] text-muted-foreground/60 truncate mt-px">
                  {doc.description}
                </span>
              )}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Render a memory document in the right panel.
 *
 * The body is the file's exact text behind a Source / Preview switch (see
 * `MarkdownPane`) — the raw Markdown is what the file actually stores, so it
 * is the default view and rendering is opt-in.
 *
 * Editing writes straight through: these are the agent's working notes, and a
 * save that asked first would interrupt whoever is updating them. Only the
 * body is sent — the frontmatter the sidebar sorts and labels by is the
 * backend's business.
 */
export function MemoryViewer({
  memory,
  fromSessionId,
  onSaved,
}: {
  memory: OpenMemory;
  fromSessionId?: string | null;
  /** Called after a successful save, with the body that was written. */
  onSaved?: (content: string) => void;
}) {
  const { doc, projectPath, sourcePath } = memory;
  const fromThisSession =
    fromSessionId != null && doc.originSessionId != null && doc.originSessionId === fromSessionId;

  // The document's own content is the source of truth. Keeping a copy in state
  // would go stale when the reader switches documents — the pane is remounted
  // per document for exactly that reason (see SessionBrowser's `key`), so the
  // copy would only ever hold the first one.
  const editor = useMarkdownEditor({
    original: doc.content,
    save: (content) => api.saveProjectMemory(sourcePath, doc.fileName, content),
    // The backend returns nothing, so hand back what was written for the
    // caller to adopt rather than reloading to find out.
    onSaved: onSaved,
  });

  return (
    <>
      {/* Header, mirroring the session header's shape */}
      <div className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-background/95">
        {doc.isIndex ? (
          <Star className="size-4 shrink-0 text-amber-500" />
        ) : (
          <FileText className="size-4 shrink-0 text-muted-foreground" />
        )}
        <span className="text-xs font-medium truncate">{doc.name ?? doc.fileName}</span>
        {doc.kind != null && (
          <span
            className={cn(
              'shrink-0 px-1.5 py-0.5 rounded text-[10px] font-mono',
              KIND_STYLES[doc.kind] ?? 'bg-muted text-muted-foreground',
            )}
          >
            {doc.kind}
          </span>
        )}
        {fromThisSession && (
          <span className="shrink-0 px-1.5 py-0.5 rounded text-[10px] bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300">
            this session
          </span>
        )}
        <span className="ml-auto shrink-0 text-[10px] text-muted-foreground/60 font-mono truncate max-w-[40%]">
          {projectPath}/memory/{doc.fileName}
        </span>
      </div>

      {doc.description != null && (
        <div className="px-4 py-1.5 border-b border-border/40 bg-muted/20 shrink-0">
          <span className="text-[11px] text-muted-foreground">{doc.description}</span>
        </div>
      )}

      {editor.error != null && (
        <div className="mx-4 mb-1 px-2 py-1 rounded text-[10px] bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300 break-all shrink-0">
          {editor.error}
        </div>
      )}

      <MarkdownPane
        className="flex-1 px-4 py-3"
        source={doc.content}
        editor={editor}
        // The panel's height is set by the app shell, so every view fills it —
        // one entry, and preview and editing inherit it.
        heights={{ source: 'flex-1' }}
      />
    </>
  );
}
