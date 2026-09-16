// ── Snippet highlighting ─────────────────────────────────────────────────
//
// Splits a snippet into plain and matched segments so the result list can
// render the hit without dangerouslySetInnerHTML.

export interface SnippetSegment {
  text: string;
  /** True for the matched region, which the UI renders emphasised. */
  match: boolean;
}

/**
 * Split `snippet` into segments around the match described by
 * `matchStart`/`matchLen` (character offsets, as produced by the backend).
 *
 * Returns a single non-match segment when there is nothing to highlight, so
 * callers can always render the result uniformly.
 */
export function highlightSnippet(
  snippet: string,
  matchStart: number | null | undefined,
  matchLen: number,
): SnippetSegment[] {
  if (snippet === '') return [];
  if (
    matchStart == null ||
    matchLen <= 0 ||
    matchStart < 0 ||
    matchStart + matchLen > snippet.length
  ) {
    return [{ text: snippet, match: false }];
  }

  const segments: SnippetSegment[] = [];
  if (matchStart > 0) {
    segments.push({ text: snippet.slice(0, matchStart), match: false });
  }
  segments.push({ text: snippet.slice(matchStart, matchStart + matchLen), match: true });
  const rest = snippet.slice(matchStart + matchLen);
  if (rest !== '') {
    segments.push({ text: rest, match: false });
  }
  return segments;
}
