// ── Semantic tool-result view ─────────────────────────────────────────────
//
// A tool record already carries everything needed to read its outcome without
// guessing: `toolArgs` is the call's `input` object serialized, and `result`
// is what the tool returned. This module turns that pair into a description
// the UI can render as a diff, a terminal block, or a numbered file — instead
// of dropping a JSON blob on the reader.

/** One line of a diff. `text` never includes the marker. */
export interface DiffRow {
  marker: ' ' | '-' | '+';
  text: string;
}

export type ToolResultView =
  | {
      kind: 'diff';
      filePath: string | null;
      replaceAll: boolean;
      rows: DiffRow[];
      truncated: boolean;
    }
  | {
      kind: 'write';
      filePath: string | null;
      rows: DiffRow[];
      truncated: boolean;
    }
  | {
      kind: 'terminal';
      command: string;
      description: string | null;
      output: string;
    }
  | {
      kind: 'file';
      filePath: string | null;
      content: string;
    };

/** Claude Code's editing tools, whose arguments hold the before/after text. */
const EDIT_TOOLS = new Set(['Edit', 'MultiEdit']);
const WRITE_TOOLS = new Set(['Write']);

/** Lines of unchanged context kept on each side of a replacement. */
const CONTEXT_LINES = 2;
/** A single Edit can rewrite a whole file; the panel stays readable instead. */
const MAX_DIFF_ROWS = 400;

export interface ToolResultInput {
  toolName?: string;
  toolArgs?: string;
  result?: string;
  isError?: boolean;
}

/**
 * Describe a tool record as something richer than text, or `null` to leave it
 * to the generic renderer.
 */
export function describeToolResult(input: ToolResultInput): ToolResultView | null {
  const { toolName, result } = input;
  if (toolName == null) return null;
  const args = parseArgs(input.toolArgs);
  if (args === null) return null;
  const output = result ?? '';

  if (EDIT_TOOLS.has(toolName)) {
    const before = stringField(args, 'old_string');
    const after = stringField(args, 'new_string');
    if (before === null || after === null) return null;
    return {
      kind: 'diff',
      filePath: stringField(args, 'file_path'),
      replaceAll: args.replace_all === true,
      ...diffRows(before, after),
    };
  }

  if (WRITE_TOOLS.has(toolName)) {
    const content = stringField(args, 'content');
    if (content === null) return null;
    // A write has no "before", so every line is an addition.
    return { kind: 'write', filePath: stringField(args, 'file_path'), ...diffRows('', content) };
  }

  if (toolName === 'Read') {
    if (output.trim() === '') return null;
    return { kind: 'file', filePath: stringField(args, 'file_path'), content: output };
  }

  if (toolName === 'Bash') {
    const command = stringField(args, 'command');
    if (command === null) return null;
    return {
      kind: 'terminal',
      command,
      description: stringField(args, 'description'),
      output,
    };
  }

  return null;
}

/**
 * Turn a before/after pair into rows, trimming the unchanged head and tail
 * down to a little context. An `Edit` replaces one contiguous region, so the
 * difference is exactly that region — no alignment pass is needed.
 */
function diffRows(before: string, after: string): { rows: DiffRow[]; truncated: boolean } {
  const oldLines = before === '' ? [] : before.split('\n');
  const newLines = after === '' ? [] : after.split('\n');

  let head = 0;
  while (head < oldLines.length && head < newLines.length && oldLines[head] === newLines[head]) {
    head += 1;
  }
  let tail = 0;
  while (
    tail < oldLines.length - head &&
    tail < newLines.length - head &&
    oldLines[oldLines.length - 1 - tail] === newLines[newLines.length - 1 - tail]
  ) {
    tail += 1;
  }

  const rows: DiffRow[] = [];
  for (let i = Math.max(0, head - CONTEXT_LINES); i < head; i += 1) {
    rows.push({ marker: ' ', text: oldLines[i] });
  }
  for (let i = head; i < oldLines.length - tail; i += 1) {
    rows.push({ marker: '-', text: oldLines[i] });
  }
  for (let i = head; i < newLines.length - tail; i += 1) {
    rows.push({ marker: '+', text: newLines[i] });
  }
  for (let i = 0; i < Math.min(CONTEXT_LINES, tail); i += 1) {
    rows.push({ marker: ' ', text: oldLines[oldLines.length - tail + i] });
  }

  const truncated = rows.length > MAX_DIFF_ROWS;
  return { rows: truncated ? rows.slice(0, MAX_DIFF_ROWS) : rows, truncated };
}

function parseArgs(toolArgs: string | undefined): Record<string, unknown> | null {
  if (toolArgs == null || toolArgs.trim() === '') return null;
  try {
    const parsed: unknown = JSON.parse(toolArgs);
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return null;
    return parsed as Record<string, unknown>;
  } catch {
    return null;
  }
}

function stringField(args: Record<string, unknown>, key: string): string | null {
  const value = args[key];
  return typeof value === 'string' ? value : null;
}
