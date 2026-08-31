// ── Trajectory Cell ───────────────────────────────────────────────────────
//
// Renders one trajectory record row with kind icon, label, text, and duration.

import React from 'react';
import type { TrajectoryCellProps } from '../utils/layout';
import { KIND_LABEL } from '../utils/format';
import { cn } from '../lib/utils';

// ── Inline SVG Icons ──────────────────────────────────────────────────────

function ToolWrenchIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14 3.3a3.8 3.8 0 0 1-4.8 4.8l-5.1 5.1a1.6 1.6 0 1 1-2.3-2.3l5.1-5.1A3.8 3.8 0 0 1 11.7 1l-2.3 2.3 2.3 2.3L14 3.3Z" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="8" cy="5.5" r="3" />
      <path d="M2 14c0-3.3 2.7-6 6-6s6 2.7 6 6" />
    </svg>
  );
}

function SparkleIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 1l1.5 3.5L13 6l-3.5 1.5L8 11l-1.5-3.5L3 6l3.5-1.5L8 1z" />
    </svg>
  );
}

function CompactedIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="2" y="2" width="12" height="3" rx="1" />
      <rect x="2" y="7" width="12" height="3" rx="1" />
      <rect x="2" y="12" width="12" height="3" rx="1" />
    </svg>
  );
}

const KIND_ICON: Record<string, React.ReactNode> = {
  system: null,
  user: <UserIcon />,
  context: null,
  compacted: <CompactedIcon />,
  message: <SparkleIcon />,
  tool: <ToolWrenchIcon />,
  subtool: <ToolWrenchIcon />,
};

const KIND_COLOR: Record<string, string> = {
  system: 'text-gray-500',
  user: 'text-blue-600 dark:text-blue-400',
  context: 'text-green-600 dark:text-green-400',
  compacted: 'text-gray-400',
  message: 'text-violet-600 dark:text-violet-400',
  tool: 'text-amber-600 dark:text-amber-400',
  subtool: 'text-amber-500 dark:text-amber-300',
};

// Label anchor: every row right-aligns its icon+label toward the content
// column. KIND_INDENT is the gap between the label's right edge and the
// content column — the same value for one level (so USER / ASSISTANT end at
// the same line), and smaller for deeper levels (tools hug the text).
const KIND_INDENT: Record<string, number> = {
  system: 4,
  user: 4,
  context: 4,
  message: 4,
  tool: 4,
  subtool: 4,
};

interface TrajectoryCellPropsExtra extends TrajectoryCellProps {
  onClick?: () => void;
  onDoubleClickTurn?: (turn: number) => void;
  selected?: boolean;
  searchMatch?: boolean;
  /** "inside" / "outside" when a timeline range is active (DSH data-timeline-focus) */
  timelineFocus?: 'inside' | 'outside' | undefined;
  /** First visible row of a turn — draws a separator + "#N" label. */
  turnStart?: boolean;
  /** Row belongs to the currently selected turn — draws the left rail. */
  activeTurn?: boolean;
  /** This row is the start of a request/step — draws the request dot. */
  requestNumber?: number;
}

export function TrajectoryCell({
  recordId,
  kind,
  text,
  turn,
  timeSeconds,
  result,
  resultPreviewMarkdown,
  isError,
  toolName,
  opensTurn,
  onClick,
  onDoubleClickTurn,
  selected,
  searchMatch,
  timelineFocus,
  turnStart = false,
  activeTurn = false,
  requestNumber,
}: TrajectoryCellPropsExtra) {
  const label = KIND_LABEL[kind] ?? kind.toUpperCase();
  const icon = KIND_ICON[kind];
  const colorClass = KIND_COLOR[kind] ?? 'text-gray-500';
  const isSummary = recordId?.startsWith('summary-turn-') === true;

  const handleDoubleClick = (e: React.MouseEvent) => {
    if (isSummary || !turnStart || turn == null || onDoubleClickTurn === undefined) return;
    e.preventDefault();
    onDoubleClickTurn(turn);
  };

  return (
    <tr
      data-kind={kind}
      data-turn-start={opensTurn ? '' : undefined}
      data-selected={selected ? '' : undefined}
      data-error={isError ? '' : undefined}
      data-timeline-focus={timelineFocus}
      onClick={onClick}
      onDoubleClick={onClick ? handleDoubleClick : undefined}
      className={cn(
        'group relative h-full border-t border-border/15 transition-colors cursor-pointer',
        turnStart && 'border-t-border/40',
        selected && 'bg-blue-100 dark:bg-blue-800/40',
        !selected && 'hover:bg-blue-50 dark:hover:bg-blue-900/20',
        searchMatch && 'bg-yellow-100 dark:bg-yellow-800/30',
      )}
    >
      {/* Event column */}
      <td className="py-0.5 px-2 align-middle w-24">
        {activeTurn && (
          <span className="absolute left-0 top-0 bottom-0 w-[2px] bg-blue-400/25 rounded-r pointer-events-none" />
        )}
        {turnStart && turn != null && (
          <span className="absolute left-2 top-[1px] text-[8px] font-mono text-muted-foreground/60 leading-none pointer-events-none">
            T{turn}
          </span>
        )}
        {/* ASSISTANT request dot — centered on the row's top border line */}
        {kind === 'message' && requestNumber !== undefined && (
          <span
            title={`Request ${requestNumber}`}
            className="absolute left-1.5 top-0 -translate-y-1/2 size-1.5 rounded-full bg-gray-400/70 group-hover:bg-blue-500 group-data-[selected]:bg-blue-500 dark:bg-gray-500 dark:group-hover:bg-blue-400 dark:group-data-[selected]:bg-blue-400 pointer-events-none"
            aria-hidden
          />
        )}
        <div className="flex items-center justify-end gap-1 text-xs pt-1">
          {isSummary ? (
            <span className="text-muted-foreground/50">…</span>
          ) : (
            <span
              style={{ width: 100, paddingRight: KIND_INDENT[kind] ?? 0 }}
              className={cn('flex items-center justify-end gap-0.5 shrink-0 min-w-0 font-normal', colorClass)}
            >
              {icon && <span className="shrink-0">{icon}</span>}
              <span className="truncate min-w-0">{label}</span>
            </span>
          )}
        </div>
      </td>

      {/* Content column */}
      <td className="py-0.5 px-2 align-middle min-w-0">
        <div className="flex items-center gap-2 min-w-0">
          {toolName && (
            <span className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] font-mono bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300">
              {toolName}
            </span>
          )}

          <span className={cn('text-xs truncate min-w-0', isSummary ? 'text-muted-foreground italic' : isError ? 'text-red-600 dark:text-red-400' : 'text-foreground/80')}>
            {text}
          </span>

          {(result || resultPreviewMarkdown) && (
            <span className="shrink-0 text-[10px] text-muted-foreground truncate max-w-[200px] ml-auto">
              → {resultPreviewMarkdown ?? result}
            </span>
          )}

          {timeSeconds != null && (
            <span className="shrink-0 text-[10px] text-muted-foreground font-mono ml-auto">
              {timeSeconds >= 1
                ? `${timeSeconds.toFixed(1)}s`
                : `${Math.round(timeSeconds * 1000)}ms`}
            </span>
          )}
        </div>
      </td>
    </tr>
  );
}