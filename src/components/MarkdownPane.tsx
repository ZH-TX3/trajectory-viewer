// ── Markdown pane ────────────────────────────────────────────────────────
//
// The shared shell for showing a Markdown document. Everywhere the app shows
// one (a project memory doc, a Config Hub prompt) this renders the source
// view, the rendered view, and the controls for switching between them:
//
//   • **Source** is the default — the file's literal text, the way it was
//     written. Nothing is interpreted, so a `#` in the middle of a line is a
//     `#`.
//   • **Preview** renders the same text as Markdown.
//
// The choice is per-pane state (not shared, not persisted): switching one
// document leaves the others alone.
//
// Editing is opt-in via `editor`. The controls sit beside Preview rather than
// on a row of their own, so the button order reads the same everywhere:
// `Preview | Edit` then, once editing, `Preview | format? | Save | Revert |
// Cancel`.
//
// Rendering and the edit/save state machine live outside, in
// `utils/renderMarkdown.ts` and `utils/useMarkdownEditor.ts`.

import { useState } from 'react';
import { Check, Eye, FileText, Loader2, Pencil, RotateCcw, X } from 'lucide-react';
import { cn } from '../lib/utils';
import type { MarkdownEditor } from '../utils/useMarkdownEditor';
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
  /** The file's saved content. */
  source: string;
  /**
   * When given, an Edit button joins Preview and the source becomes editable.
   * Save / Revert / Cancel appear in their place while editing.
   */
  editor?: MarkdownEditor;
  /**
   * Rendered while editing, beside Save. Used by the Config Hub for its
   * "format JSON" action, which only applies to JSON.
   */
  editAction?: { label: string; onClick: () => void } | null;
  /** Classes for the root: how the pane takes its place in the surrounding layout. */
  className?: string;
  /** How much room each view of the document gets. */
  heights?: MarkdownHeights;
}

/** The three views of a document, each with its own height. */
export interface MarkdownHeights {
  /**
   * The source view — the default one, and the only place a height limit
   * belongs.
   *
   * A cap and the `overflow-auto` that acts on it have to sit on the same
   * element. A `max-h-*` on the root instead leaves the body at its natural
   * height (nothing to scroll), so the content spills out over whatever is
   * below the pane.
   *
   * `flex-1` fills a parent of known height (the memory viewer); `max-h-*` caps
   * a parent that sizes to its content (a Config Hub row).
   */
  source?: string;
  /**
   * Preview. Source is what most visits are for, so it stays compact; a render
   * is something you ask for, and it gets room to be read in.
   */
  preview?: string;
  /**
   * Editing. Colouring the text is not the same job as scanning it, and a cap
   * that keeps a list row tidy leaves far too little room to type in.
   */
  editor?: string;
}

/**
 * The height class for one view.
 *
 * Kept as a function rather than three inline ternaries so the policy is
 * stated once and every view can be checked on its own. Anything unset falls
 * back to the source height — that is what lets the memory viewer pass a
 * single `flex-1` and have all three views fill the panel.
 */
export function markdownHeight(
  view: 'source' | 'preview' | 'editor',
  heights: MarkdownHeights | undefined,
): string {
  if (heights == null) return '';
  return heights[view] ?? heights.source ?? '';
}

/**
 * A Markdown document with a Source / Preview switch, and optional editing.
 *
 * `source` is shown first on purpose: this is a viewer for files whose exact
 * text matters (frontmatter, indentation, code), and rendering them by default
 * would hide all three.
 */
export function MarkdownPane({
  source,
  editor,
  editAction = null,
  className,
  heights,
}: MarkdownPaneProps) {
  const [previewing, setPreviewing] = useState(false);
  // Held in a local so TypeScript can narrow it: `editor` is optional, but every
  // use below runs only when it exists.
  const active = editor?.editing === true ? editor : null;
  // A draft in progress is what the Preview shows — otherwise there would be
  // no way to check a change before saving it.
  const text = active != null ? active.draft : source;

  // "Edit" means "show me the editor", so it always leaves the preview behind.
  // Preview and Edit are two views of the same text; leaving `previewing` set
  // would keep rendering the body and make the buttons say "Save" while the
  // pane still looked unedited.
  const startEdit = () => {
    editor?.startEdit();
    setPreviewing(false);
  };

  // The body's height follows the view it is in, and the constraint arrives
  // together with the `overflow-auto` that consumes it, so no view can be
  // bounded without a way to scroll.
  //
  // Previewing a draft counts as editing: checking a long change in a
  // reading-sized window means scrolling a few lines at a time, which is the
  // problem the preview exists to avoid.
  const view = active != null ? 'editor' : previewing ? 'preview' : 'source';
  const bodyBox = cn(
    'mt-1 min-h-0 overflow-auto rounded bg-muted/30 p-2',
    markdownHeight(view, heights),
  );

  return (
    <div className={cn('flex min-h-0 flex-col', className)}>
      <div className="flex shrink-0 items-center gap-1">
        <div className="ml-auto flex items-center gap-1">
          {active != null && editAction != null && (
            <button
              onClick={editAction.onClick}
              disabled={active.saving}
              title={editAction.label}
              className="text-[10px] text-blue-600 dark:text-blue-400 hover:underline disabled:opacity-50"
            >
              {editAction.label}
            </button>
          )}

          {active != null ? (
            <>
              <button
                onClick={() => void active.save()}
                disabled={active.saving || !active.dirty}
                title="Save"
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50"
              >
                {active.saving ? <Loader2 className="size-3 animate-spin" /> : <Check className="size-3" />}
                Save
              </button>
              <button
                onClick={active.revert}
                disabled={active.saving || !active.dirty}
                title="Revert changes"
                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-40"
              >
                <RotateCcw className="size-3" />
              </button>
              <button
                onClick={active.cancel}
                disabled={active.saving}
                title="Cancel"
                className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-40"
              >
                <X className="size-3" />
              </button>
            </>
          ) : (
            editor != null && (
              <button
                onClick={startEdit}
                title="Edit"
                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] transition-colors border border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/40"
              >
                <Pencil className="size-3" />
                Edit
              </button>
            )
          )}

          <button
            type="button"
            onClick={() => setPreviewing((v) => !v)}
            aria-pressed={previewing}
            title={previewing ? 'Show source' : active != null ? 'Preview draft' : 'Preview Markdown'}
            className={cn(
              'inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] transition-colors border',
              previewing
                ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 border-blue-300 dark:border-blue-700'
                : 'border-transparent text-blue-600 dark:text-blue-400 hover:underline',
            )}
          >
            {previewing ? <FileText className="size-3" /> : <Eye className="size-3" />}
            {previewing ? 'Source' : 'Preview'}
          </button>
        </div>
      </div>

      {previewing ? (
        <MarkdownBody
          source={text}
          // The pane fills whatever it is given, and its own body size is the
          // right one — there is no surrounding panel to match.
          className={cn('md-body', BODY_SIZE, bodyBox)}
        />
      ) : active != null ? (
        <textarea
          value={active.draft}
          onChange={(e) => active.setDraft(e.target.value)}
          spellCheck={false}
          className={cn(BODY_SIZE, bodyBox, 'bg-transparent resize-y',
            'border border-[hsl(var(--border))] focus:outline-none focus:ring-1 focus:ring-blue-400')}
        />
      ) : (
        <pre className={cn(BODY_SIZE, bodyBox, 'font-mono whitespace-pre-wrap break-words text-foreground/80')}>
          {text === '' ? '(empty)' : text}
        </pre>
      )}
    </div>
  );
}
