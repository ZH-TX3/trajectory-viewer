// ── Minimal Markdown preview ─────────────────────────────────────────────
//
// The detail panel previews tool records as Markdown by convention:
// `**Bash**` followed by a fenced ```json block holding the arguments. Pulling
// in a full Markdown library for that one shape would be a heavy dependency,
// so this parses just the constructs the trajectory view actually produces —
// fenced code blocks and bold spans. Everything else stays literal text.

export interface MarkdownCodeBlock {
  type: 'code';
  lang: string;
  content: string;
}

export interface MarkdownTextBlock {
  type: 'text';
  content: string;
}

export type MarkdownBlock = MarkdownCodeBlock | MarkdownTextBlock;

export interface InlineSegment {
  text: string;
  bold: boolean;
}

/** Opening or closing fence; the captured group is the language tag. */
const FENCE = /^```([^\s`]*)\s*$/;

/**
 * Split Markdown into code and text blocks. An unterminated fence is kept as
 * literal text rather than swallowing the rest of the input.
 */
export function parseMarkdownBlocks(text: string): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  let buffer: string[] = [];
  // Text held back while a fence is open, so an unterminated fence can be
  // re-emitted together with its opening line as one literal text block.
  let pendingText: string[] = [];
  let inCode = false;
  let lang = '';

  const takeText = (): string | null => {
    const content = buffer.join('\n');
    buffer = [];
    return content === '' ? null : content;
  };

  for (const line of text.split('\n')) {
    const fence = line.trim().match(FENCE);
    if (fence !== null) {
      if (inCode) {
        const before = pendingText.join('\n');
        if (before !== '') blocks.push({ type: 'text', content: before });
        blocks.push({ type: 'code', lang, content: buffer.join('\n') });
        buffer = [];
        pendingText = [];
        inCode = false;
        lang = '';
      } else {
        pendingText = buffer;
        buffer = [];
        inCode = true;
        lang = fence[1].toLowerCase();
      }
      continue;
    }
    buffer.push(line);
  }

  if (inCode) {
    const before = pendingText.join('\n');
    const body = ['```' + lang, ...buffer].join('\n');
    const content = before === '' ? body : `${before}\n${body}`;
    blocks.push({ type: 'text', content });
  } else {
    const rest = takeText();
    if (rest !== null) blocks.push({ type: 'text', content: rest });
  }
  return blocks;
}

/**
 * Pretty-print a code block's body when it holds JSON, so a one-line argument
 * object renders as readable indented JSON. Non-JSON (or invalid JSON) is
 * returned unchanged.
 */
export function formatCodeBlock(lang: string, content: string): string {
  if (lang !== 'json') return content;
  try {
    return JSON.stringify(JSON.parse(content), null, 2);
  } catch {
    return content;
  }
}

/** Split a text line into plain and `**bold**` segments. */
export function parseInline(text: string): InlineSegment[] {
  const segments: InlineSegment[] = [];
  const pattern = /\*\*([^*]+)\*\*/g;
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > last) {
      segments.push({ text: text.slice(last, match.index), bold: false });
    }
    segments.push({ text: match[1], bold: true });
    last = match.index + match[0].length;
  }
  if (last < text.length) {
    segments.push({ text: text.slice(last), bold: false });
  }
  return segments;
}

// ── JSON colouring ───────────────────────────────────────────────────────
//
// Object keys and their values are tinted separately so a rendered argument
// block reads at a glance.

export type JsonTokenKind = 'key' | 'value' | 'plain';

export interface JsonToken {
  text: string;
  kind: JsonTokenKind;
}

/** Colours applied to keys / values. */
export const JSON_KEY_COLOR = '#881391';
export const JSON_VALUE_COLOR = '#c41a16';

const STRING = '"(?:\\\\.|[^"\\\\])*"';
/** Key first (a string followed by a colon), then any other string, then
 *  numbers and literals. Everything else — punctuation and whitespace — falls
 *  through to `plain`. */
const JSON_TOKEN = new RegExp(
  `(${STRING})(?=\\s*:)|(${STRING})|(-?\\d+(?:\\.\\d+)?(?:[eE][-+]?\\d+)?|true|false|null)`,
  'g',
);

/**
 * Tokenise JSON for colouring. Returns one plain token when the text is not
 * valid JSON, so the caller renders it verbatim.
 */
export function tokenizeJson(text: string): JsonToken[] {
  try {
    JSON.parse(text);
  } catch {
    return [{ text, kind: 'plain' }];
  }

  const tokens: JsonToken[] = [];
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = JSON_TOKEN.exec(text)) !== null) {
    if (match.index > last) {
      tokens.push({ text: text.slice(last, match.index), kind: 'plain' });
    }
    tokens.push({
      text: match[0],
      kind: match[1] !== undefined ? 'key' : 'value',
    });
    last = JSON_TOKEN.lastIndex;
  }
  if (last < text.length) {
    tokens.push({ text: text.slice(last), kind: 'plain' });
  }
  return tokens;
}
