// ── Markdown pane ────────────────────────────────────────────────────────
//
// The shared shell for showing a Markdown document. Everywhere the app shows
// one (a project memory doc, a Config Hub prompt, the trajectory detail
// preview) this renders the two views and the switch between them:
//
//   • **Source** is the default — the file's literal text, the way it was
//     written. Nothing is interpreted, so a `#` in the middle of a line is a
//     `#`.
//   • **Preview** renders the same text as Markdown.
//
// The choice is per-pane state (not shared, not persisted): switching one
// document leaves the others alone.
//
// Rendering itself lives in `utils/renderMarkdown.ts`.

import { useState } from 'react';
import { Eye, FileText } from 'lucide-react';
import { cn } from '../lib/utils';
import { MarkdownBody } from './MarkdownBody';

/**
 * Size for a document body.
 *
 * The app's panels run at `text-xs`/`text-[10px]`, and the rendered
 * Markdown sizes its own headings, code and tables in `em` — so the parent's
 * font size is what actually decides how large the document reads. Keep this
 * in step with the panel the pane is dropped into via `className`.
 */
const BODY_SIZE = 'text-[11px] leading-relaxed';

export interface MarkdownPaneProps {
  /** Raw Markdown, exactly as it is stored on disk. */
  source: string;
  /** Label for the source view, e.g. `MEMORY.md` or `CLAUDE.md`. */
  label?: string;
  /** Extra classes on the scroll container. */
  className?: string;
}

/**
 * A Markdown document with a Source / Preview switch.
 *
 * `source` is shown first on purpose: this is a viewer for files whose exact
 * text matters (frontmatter, indentation, code), and rendering them by default
 * would hide all three.
 */
export function MarkdownPane({ source, label, className }: MarkdownPaneProps) {
  const [previewing, setPreviewing] = useState(false);

  return (
    <div className={cn('flex flex-col min-h-0', className)}>
      <div className="flex items-center gap-1 shrink-0">
        <span className="text-[10px] font-medium text-muted-foreground truncate">
          {label}
        </span>
        <button
          type="button"
          onClick={() => setPreviewing((v) => !v)}
          aria-pressed={previewing}
          title={previewing ? 'Show source' : 'Preview Markdown'}
          className={cn(
            'ml-auto inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] transition-colors border',
            previewing
              ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 border-blue-300 dark:border-blue-700'
              : 'text-muted-foreground hover:text-foreground hover:bg-muted/40 border-transparent',
          )}
        >
          {previewing ? <FileText className="size-3" /> : <Eye className="size-3" />}
          {previewing ? 'Source' : 'Preview'}
        </button>
      </div>

      {previewing ? (
        <MarkdownBody
          source={source}
          // The pane fills whatever it is given, and its own body size is the
          // right one — there is no surrounding panel to match.
          className={cn('md-body mt-1 flex-1 overflow-auto', BODY_SIZE)}
        />
      ) : (
        <pre
          className={cn(
            BODY_SIZE,
            'mt-1 flex-1 overflow-auto font-mono whitespace-pre-wrap break-words',
            'rounded bg-muted/30 p-2 text-foreground/80',
          )}
        >
          {source === '' ? '(empty)' : source}
        </pre>
      )}
    </div>
  );
}
