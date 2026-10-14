// ── Markdown rendering ───────────────────────────────────────────────────
//
// One Markdown renderer for the whole app. Everywhere Markdown is shown —
// a project memory document, a tool's prompt/config file in the Config Hub,
// the per-record preview in the trajectory detail panel — it goes through
// `renderMarkdown` + `handleMarkdownClick`.
//
// `marked` does the parsing; the two hooks below exist for looks and for
// navigation, not for sanitisation (the app's own author writes these files):
//
//   - `code` tints JSON bodies so a tool's arguments read at a glance, and
//     escapes everything else.
//   - `link` drops the href from the output and re-attaches it on click, so
//     a link cannot navigate the single-page webview away from the app.
//
// Everything else keeps marked's GFM defaults, and the result is injected with
// `dangerouslySetInnerHTML`.

import type { MouseEvent } from 'react';
import { Marked } from 'marked';
import {
  formatCodeBlock,
  JSON_KEY_COLOR,
  JSON_VALUE_COLOR,
  tokenizeJson,
} from './markdown';

/** Escapes the five HTML-significant characters. */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Schemes a memory doc or a prompt may legitimately point at. */
const SAFE_HREF = /^(https?:|mailto:|tel:)/;

/** Object keys one tint, their values another, punctuation neutral. */
function renderJson(text: string): string {
  return tokenizeJson(formatCodeBlock('json', text))
    .map((token) => {
      const body = escapeHtml(token.text);
      if (token.kind === 'key') return `<span style="color:${JSON_KEY_COLOR}">${body}</span>`;
      if (token.kind === 'value') return `<span style="color:${JSON_VALUE_COLOR}">${body}</span>`;
      return `<span class="md-plain">${body}</span>`;
    })
    .join('');
}

const marked = new Marked({
  gfm: true,
  // Markdown is prose here, not CommonMark's "one paragraph per blank line" —
  // a lone newline in a memory doc means a line break.
  breaks: true,
  renderer: {
    code({ text, lang }) {
      const language = (lang ?? '').split(/\s+/)[0].toLowerCase();
      const label = language === '' ? '' : ` data-lang="${escapeHtml(language)}"`;
      const body = language === 'json' ? renderJson(text) : `<span class="md-plain">${escapeHtml(text)}</span>`;
      return `<pre class="md-pre"${label}><code>${body}</code></pre>`;
    },
    link(token) {
      const label = this.parser.parseInline(token.tokens);
      const safe = SAFE_HREF.test(token.href ?? '');
      // The href travels in `data-`, not `href`, so a click has to opt in
      // (see `handleMarkdownClick`) instead of navigating the webview.
      return `<a class="md-link" data-href="${escapeHtml(token.href ?? '')}"${
        safe ? '' : ' data-unsafe="1"'
      }>${label}</a>`;
    },
  },
});

/**
 * Render Markdown to HTML.
 *
 * The source is either prose the project wrote itself (a memory doc, a
 * prompt) or a tool record this app generated, so plain GFM output is the
 * intent. Returns `''` for an empty or unparseable source — a malformed
 * document must not take the panel down, and the source view still shows the
 * raw text.
 */
export function renderMarkdown(source: string): string {
  if (source === '') return '';
  try {
    const html = marked.parse(source, { async: false });
    return typeof html === 'string' ? html : '';
  } catch {
    return '';
  }
}

/**
 * Click handling shared by every rendered document.
 *
 * `http(s)`/`mailto`/`tel` links open in the OS browser; anything else the
 * renderer flagged `data-unsafe` is dropped.
 */
export function handleMarkdownClick(event: MouseEvent<HTMLDivElement>): void {
  const anchor = (event.target as HTMLElement).closest('a');
  if (anchor === null) return;
  event.preventDefault();

  const href = anchor.dataset.href ?? '';
  if (anchor.dataset.unsafe === '1' || !SAFE_HREF.test(href)) return;
  window.open(href, '_blank', 'noopener,noreferrer');
}
