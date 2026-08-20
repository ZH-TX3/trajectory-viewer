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
  user: 'text-emerald-600 dark:text-emerald-400',
  context: 'text-green-600 dark:text-green-400',
  compacted: 'text-gray-400',
  message: 'text-violet-600 dark:text-violet-400',
  tool: 'text-amber-600 dark:text-amber-400',
  subtool: 'text-amber-500 dark:text-amber-300',
};

// Horizontal indent per record kind — tools nest below the assistant message.
const KIND_INDENT: Record<string, number> = {
  system: 8,
  user: 8,
  context: 8,
  message: 28,
  tool: 44,
  subtool: 56,
};

interface TrajectoryCellPropsExtra extends TrajectoryCellProps {
  onClick?: () => void;
  selected?: boolean;
  searchMatch?: boolean;
  /** "inside" / "outside" when a timeline range is active (DSH data-timeline-focus) */
  timelineFocus?: 'inside' | 'outside' | undefined;
}

export function TrajectoryCell({
  index,
  kind,
  text,
  timeSeconds,
  result,
  resultPreviewMarkdown,
  isError,
  toolName,
  opensTurn,
  onClick,
  selected,
  searchMatch,
  timelineFocus,
}: TrajectoryCellPropsExtra) {
  const label = KIND_LABEL[kind] ?? kind.toUpperCase();
  const icon = KIND_ICON[kind];
  const colorClass = KIND_COLOR[kind] ?? 'text-gray-500';

  return (
    <tr
      data-kind={kind}
      data-turn-start={opensTurn ? '' : undefined}
      data-selected={selected ? '' : undefined}
      data-error={isError ? '' : undefined}
      data-timeline-focus={timelineFocus}
      onClick={onClick}
      className={cn(
        'border-b border-border/40 transition-colors cursor-pointer',
        selected && 'bg-blue-100 dark:bg-blue-800/40 ring-2 ring-blue-400/60 dark:ring-blue-500/60 ring-inset',
        !selected && 'hover:bg-blue-50 dark:hover:bg-blue-900/20',
        searchMatch && 'bg-yellow-100 dark:bg-yellow-800/30',
      )}
    >
      {/* Event column */}
      <td className="py-0.5 px-2 align-middle w-24">
        <div className="flex items-center gap-1 text-xs">
          <span className="text-muted-foreground font-mono text-[10px] w-6 text-right shrink-0">
            #{index}
          </span>
          <span
            className={cn('inline-flex items-center gap-0.5 shrink-0 font-normal', colorClass)}
            style={{ paddingLeft: KIND_INDENT[kind] ?? 0 }}
          >
            {icon && <span className="shrink-0">{icon}</span>}
            <span className="truncate max-w-[5rem]">{label}</span>
          </span>
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

          <span className={cn('text-xs truncate min-w-0', isError ? 'text-red-600 dark:text-red-400' : 'text-foreground/80')}>
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