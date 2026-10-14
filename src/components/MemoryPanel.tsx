// ── Project memory ───────────────────────────────────────────────────────
//
// Claude keeps per-project memory as Markdown beside the session files:
//
//   ~/.claude/projects/{project}/memory/MEMORY.md
//   ~/.claude/projects/{project}/memory/{name}.md
//
// In the sidebar this mirrors that layout — the project group gains a
// `memory/` node listing the documents, above its sessions. Clicking one opens
// it in the right panel, the same way clicking a session opens its messages.
//
// The node is rendered only when the directory exists: most projects never
// write one.

import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, FileText, FolderOpen, Star } from 'lucide-react';
import { api } from '../api';
import { cn } from '../lib/utils';
import type { MemoryDoc } from '../types';

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
}

/**
 * The `memory/` node for one project group.
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
      {/* The memory/ directory row */}
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
        <span className="text-[11px] font-medium truncate">memory/</span>
        <span className="text-[10px] opacity-60 ml-auto">{docs.length}</span>
      </div>

      {open && docs.map((doc) => {
        const isSelected = doc.fileName === selectedFileName;
        return (
          <div
            key={doc.fileName}
            onClick={() => onOpen({ doc, projectPath })}
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
                <span className="text-[10px] truncate">
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
 * Render a memory body in the right panel.
 *
 * Deliberately not a Markdown renderer: memory files are prose with headings
 * and lists, and the app has no Markdown dependency. Headings, list markers
 * and blank lines are enough to make one readable, and anything else stays
 * literal rather than being silently dropped.
 */
export function MemoryViewer({
  memory,
  fromSessionId,
}: {
  memory: OpenMemory;
  fromSessionId?: string | null;
}) {
  const { doc, projectPath } = memory;
  const fromThisSession =
    fromSessionId != null && doc.originSessionId != null && doc.originSessionId === fromSessionId;

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

      <div className="flex-1 overflow-auto px-4 py-3">
        <div className="space-y-1 max-w-3xl">
          {doc.content.split('\n').map((line, i) => {
            const trimmed = line.trim();
            if (trimmed === '') return <div key={i} className="h-2" />;

            const heading = trimmed.match(/^(#{1,6})\s+(.*)$/);
            if (heading) {
              const level = heading[1].length;
              return (
                <div
                  key={i}
                  className={cn(
                    'font-semibold text-foreground',
                    level === 1 ? 'text-sm mt-2' : level === 2 ? 'text-xs mt-1.5' : 'text-[11px] mt-1',
                  )}
                >
                  {heading[2]}
                </div>
              );
            }

            const bullet = trimmed.match(/^([-*]|\d+\.)\s+(.*)$/);
            if (bullet) {
              return (
                <div key={i} className="flex gap-1.5 text-xs text-foreground/85 leading-relaxed">
                  <span className="text-muted-foreground/60 shrink-0">{bullet[1]}</span>
                  <span className="whitespace-pre-wrap break-words min-w-0">{bullet[2]}</span>
                </div>
              );
            }

            return (
              <div key={i} className="text-xs text-foreground/85 whitespace-pre-wrap break-words leading-relaxed">
                {line}
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
