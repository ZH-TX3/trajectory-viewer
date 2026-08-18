// ── Trajectory Timeline ──────────────────────────────────────────────────
//
// Chrome-Network-style overview timeline with three lanes (Input / Model / Tools).
// Supports range selection via drag.

import React, { useMemo, useRef, useState, useCallback } from 'react';
import type { TrajectoryTurnModel } from '../utils/layout';
import { formatDurationSeconds, formatTimestamp } from '../utils/format';
import { cn } from '../lib/utils';

interface TrajectoryTimelineProps {
  turns: readonly TrajectoryTurnModel[];
  actualDuration: boolean;
  searchMatchIndexes?: ReadonlySet<number> | null;
  onRangeChange?: (range: { start: number; end: number } | null) => void;
  onRecordSelect?: (index: number) => void;
}

interface TimelineSpan {
  index: number;
  kind: string;
  start: number;
  width: number;
  lane: number;
  toolName?: string;
  text: string;
  startedAt: number | null;
  durationMs: number | null;
  ttftMs?: number | null;
  isError?: boolean;
}

export function TrajectoryTimeline({
  turns,
  actualDuration,
  searchMatchIndexes = null,
  onRangeChange,
  onRecordSelect,
}: TrajectoryTimelineProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [selection, setSelection] = useState<{ start: number; end: number } | null>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const dragStartRef = useRef<number>(0);

  // Collect every cell in layout order
  const cells = useMemo(() => {
    const result: Array<{
      index: number;
      kind: string;
      text: string;
      toolName?: string;
      startedAt: number | null;
      timeSeconds: number | null;
      ttftMs?: number | null;
      isError?: boolean;
    }> = [];

    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          result.push({
            index: cell.index,
            kind: cell.kind,
            text: cell.text,
            toolName: cell.toolName,
            startedAt: cell.startedAt ?? null,
            timeSeconds: cell.timeSeconds,
            ttftMs: cell.assistantMetrics?.firstTokenTime != null && cell.startedAt != null
              ? cell.assistantMetrics.firstTokenTime - cell.startedAt
              : null,
            isError: cell.isError,
          });
        }
      }
    }

    return result;
  }, [turns]);

  // Build positioned spans
  const spans = useMemo(() => {
    const result: TimelineSpan[] = [];
    if (cells.length === 0) return result;

    const totalCells = cells.length;

    let minTime = Number.POSITIVE_INFINITY;
    let maxTime = Number.NEGATIVE_INFINITY;

    for (const cell of cells) {
      if (cell.startedAt != null) {
        minTime = Math.min(minTime, cell.startedAt);
        const durationMs = cell.timeSeconds != null ? cell.timeSeconds * 1000 : 0;
        maxTime = Math.max(maxTime, cell.startedAt + durationMs);
      }
    }

    if (!Number.isFinite(minTime) || !Number.isFinite(maxTime) || maxTime <= minTime) {
      minTime = 0;
      maxTime = 1;
    }
    const totalDuration = Math.max(maxTime - minTime, 1);

    cells.forEach((cell, order) => {
      const durationMs = cell.timeSeconds != null ? cell.timeSeconds * 1000 : 0;
      let start: number;
      let width: number;

      if (actualDuration && cell.startedAt != null) {
        start = (cell.startedAt - minTime) / totalDuration;
        width = durationMs > 0 ? durationMs / totalDuration : 0.004;
      } else {
        start = order / totalCells;
        width = 0.9 / totalCells;
      }

      let lane = 1;
      if (cell.kind === 'user' || cell.kind === 'system' || cell.kind === 'context') {
        lane = 0;
      } else if (cell.kind === 'tool' || cell.kind === 'subtool') {
        lane = 2;
      }

      result.push({
        index: cell.index,
        kind: cell.kind,
        start,
        width,
        lane,
        toolName: cell.toolName,
        text: cell.text,
        startedAt: cell.startedAt,
        durationMs: durationMs > 0 ? durationMs : null,
        ttftMs: cell.ttftMs,
        isError: cell.isError,
      });
    });

    return result;
  }, [cells, actualDuration]);

  const getSpanColor = (kind: string): string => {
    switch (kind) {
      case 'user': return 'bg-emerald-400 dark:bg-emerald-500';
      case 'message': return 'bg-violet-400 dark:bg-violet-500';
      case 'tool':
      case 'subtool': return 'bg-amber-400 dark:bg-amber-500';
      case 'system': return 'bg-gray-400 dark:bg-gray-500';
      case 'compacted': return 'bg-gray-300 dark:bg-gray-600';
      default: return 'bg-blue-400 dark:bg-blue-500';
    }
  };

  const emitSelection = useCallback(
    (selectionFraction: { start: number; end: number } | null) => {
      if (!selectionFraction) {
        onRangeChange?.(null);
        return;
      }

      const selectedIndexes = spans
        .filter((span) => {
          const spanEnd = span.start + span.width;
          return spanEnd >= selectionFraction.start && span.start <= selectionFraction.end;
        })
        .map((span) => span.index);

      if (selectedIndexes.length === 0) {
        onRangeChange?.(null);
        return;
      }

      onRangeChange?.({ start: Math.min(...selectedIndexes), end: Math.max(...selectedIndexes) });
    },
    [spans, onRangeChange],
  );

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.min(Math.max((e.clientX - rect.left) / rect.width, 0), 1);
    dragStartRef.current = x;
    setSelection({ start: x, end: x });
    setIsDragging(true);
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!isDragging) return;
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.min(Math.max((e.clientX - rect.left) / rect.width, 0), 1);
    const start = Math.min(dragStartRef.current, x);
    const end = Math.max(dragStartRef.current, x);
    setSelection({ start, end });
  }, [isDragging]);

  const handleMouseUp = useCallback(() => {
    if (!isDragging) return;
    setIsDragging(false);
    emitSelection(selection);
  }, [isDragging, selection, emitSelection]);

  const handleDoubleClick = useCallback(() => {
    setSelection(null);
    emitSelection(null);
  }, [emitSelection]);

  const hoveredSpan = hoverIndex != null ? spans.find((s) => s.index === hoverIndex) : null;

  return (
    <div className="h-[50px] border-b border-border/40 bg-muted/30 flex shrink-0 select-none">
      {/* Labels column */}
      <div className="w-11 shrink-0 border-r border-border/40 relative text-[9px] text-muted-foreground leading-none">
        <span className="absolute right-1 top-[7px]">Input</span>
        <span className="absolute right-1 top-[21px]">Model</span>
        <span className="absolute right-1 top-[35px]">Tools</span>
      </div>

      {/* Track */}
      <div
        ref={trackRef}
        className="flex-1 relative cursor-crosshair overflow-hidden"
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        onDoubleClick={handleDoubleClick}
      >
        {spans.length === 0 && (
          <span className="absolute inset-0 flex items-center justify-center text-[11px] text-muted-foreground">
            No events
          </span>
        )}

        <div className="absolute inset-2">
          {spans.map((span) => (
            <div
              key={span.index}
              data-search-match={searchMatchIndexes?.has(span.index) ? '' : undefined}
              data-error={span.isError ? '' : undefined}
              onClick={() => onRecordSelect?.(span.index)}
              onMouseEnter={() => setHoverIndex(span.index)}
              onMouseLeave={() => setHoverIndex(null)}
              className={cn(
                'absolute h-2.5 rounded-sm transition-opacity cursor-pointer',
                getSpanColor(span.kind),
                hoverIndex === span.index ? 'opacity-100' : 'opacity-70 hover:opacity-90',
                span.isError && 'ring-1 ring-red-400',
              )}
              style={{
                left: `${span.start * 100}%`,
                width: `${Math.max(span.width * 100, 0.5)}%`,
                top: `${span.lane * 16 + 1}px`,
              }}
              title={`${span.text} — ${span.durationMs ? formatDurationSeconds(span.durationMs / 1000) : '—'}`}
            />
          ))}
        </div>

        {selection && (
          <div
            className="absolute top-0 bottom-0 bg-primary/10 border-x border-primary/40 pointer-events-none"
            style={{
              left: `${selection.start * 100}%`,
              width: `${(selection.end - selection.start) * 100}%`,
            }}
          />
        )}

        {hoveredSpan && (
          <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-1 px-2 py-1 rounded bg-popover border border-border/40 shadow-md text-[10px] whitespace-nowrap z-20 pointer-events-none">
            <div className="font-medium">{hoveredSpan.text}</div>
            <div className="text-muted-foreground">
              {hoveredSpan.startedAt && formatTimestamp(hoveredSpan.startedAt)}
              {hoveredSpan.durationMs && ` · ${formatDurationSeconds(hoveredSpan.durationMs / 1000)}`}
              {hoveredSpan.ttftMs != null && ` · TTFT: ${formatDurationSeconds(hoveredSpan.ttftMs / 1000)}`}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}