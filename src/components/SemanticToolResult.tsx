// ── Semantic tool result ──────────────────────────────────────────────────
//
// Renders what `describeToolResult` recognized: an edit as a diff, a shell
// call as a terminal block, a read as a numbered file. Anything it did not
// recognize is left to the caller's generic renderer.

import type { DiffRow, ToolResultView } from '../utils/toolResult';
import { cn } from '../lib/utils';

type DiffView = Extract<ToolResultView, { kind: 'diff' | 'write' }>;
type TerminalView = Extract<ToolResultView, { kind: 'terminal' }>;
type FileView = Extract<ToolResultView, { kind: 'file' }>;

export function SemanticToolResult({ view, isError }: { view: ToolResultView; isError: boolean }) {
  switch (view.kind) {
    case 'diff':
    case 'write':
      return <DiffResult view={view} />;
    case 'terminal':
      return <TerminalResult view={view} isError={isError} />;
    case 'file':
      return <FileResult view={view} />;
  }
}

function DiffResult({ view }: { view: DiffView }) {
  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
        <span className="font-mono truncate">{view.filePath ?? '(unknown file)'}</span>
        {view.kind === 'write' ? (
          <span className="shrink-0">new file contents</span>
        ) : (
          view.replaceAll && <span className="shrink-0">every occurrence</span>
        )}
      </div>
      <div className="rounded border border-border/40 overflow-hidden font-mono text-[10px] leading-[1.6]">
        {view.rows.map((row, index) => (
          <DiffLine key={index} row={row} />
        ))}
      </div>
      {view.truncated && (
        <p className="text-[10px] text-muted-foreground">…diff truncated</p>
      )}
    </div>
  );
}

function DiffLine({ row }: { row: DiffRow }) {
  return (
    <div
      data-diff-marker={row.marker}
      className={cn(
        'flex',
        row.marker === '-' && 'bg-red-500/10',
        row.marker === '+' && 'bg-emerald-500/10',
      )}
    >
      <span className="w-4 shrink-0 select-none text-center text-muted-foreground/60">
        {row.marker}
      </span>
      <span className="flex-1 whitespace-pre-wrap break-all pr-2 text-foreground/80">
        {row.text === '' ? ' ' : row.text}
      </span>
    </div>
  );
}

function TerminalResult({ view, isError }: { view: TerminalView; isError: boolean }) {
  return (
    <div className="space-y-1">
      <div className="rounded border border-border/40 bg-muted/30 px-2 py-1 text-[10px]">
        <div className="font-mono whitespace-pre-wrap break-all text-foreground/80">
          <span className="select-none text-muted-foreground">$ </span>
          {view.command}
        </div>
        {view.description !== null && (
          <div className="text-muted-foreground">{view.description}</div>
        )}
      </div>
      <pre
        className={cn(
          'rounded border px-2 py-1 font-mono text-[10px] whitespace-pre-wrap break-all leading-relaxed',
          isError
            ? 'border-red-500/40 bg-red-500/5 text-red-600 dark:text-red-400'
            : 'border-border/40 text-foreground/80',
        )}
      >
        {view.output.trim() === '' ? '(no output)' : view.output}
      </pre>
    </div>
  );
}

function FileResult({ view }: { view: FileView }) {
  return (
    <div className="space-y-1">
      {view.filePath !== null && (
        <div className="font-mono text-[10px] text-muted-foreground truncate">{view.filePath}</div>
      )}
      {/* Claude's Read already numbers every line, so this stays verbatim. */}
      <pre className="rounded border border-border/40 px-2 py-1 font-mono text-[10px] whitespace-pre-wrap break-all leading-relaxed text-foreground/80">
        {view.content}
      </pre>
    </div>
  );
}
