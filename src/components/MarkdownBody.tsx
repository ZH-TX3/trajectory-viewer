// ── Rendered Markdown body ───────────────────────────────────────────────
//
// Turns source Markdown into the rendered DOM, and nothing else: no switch,
// no header, no scroll container. Callers frame it — `MarkdownPane` fills a
// panel, the Config Hub clips it to `max-h-24` behind a Preview button, the
// trajectory detail panel drops it under a tab.
//
// `md-body` (the styling hook in `styles.css`) and the link click handler are
// applied here rather than left to callers, because both have to be attached
// to this exact element — a `.md-body` on an ancestor would still style the
// output, but a click handler on one would miss every link below it.
//
// Sizing is deliberately left to the caller: `styles.css` expresses headings,
// code and tables in `em`, so the caller's font-size decides the scale.

import { cn } from '../lib/utils';
import { handleMarkdownClick, renderMarkdown } from '../utils/renderMarkdown';

export interface MarkdownBodyProps {
  /** Raw Markdown. */
  source: string;
  /** Classes for the wrapper: the font size, spacing and any height clamp. */
  className?: string;
}

/** Shown when there is nothing to render, so the pane doesn't collapse. */
const EMPTY = '(nothing to preview)';

export function MarkdownBody({ source, className }: MarkdownBodyProps) {
  const html = renderMarkdown(source);

  if (html === '') {
    return <div className={cn('md-body', className)}>{EMPTY}</div>;
  }
  return (
    <div
      className={cn('md-body', className)}
      onClick={handleMarkdownClick}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
