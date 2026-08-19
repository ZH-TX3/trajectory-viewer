// ── Trajectory Timeline ──────────────────────────────────────────────────
//
// Chrome-Network-style timeline. Ported from DSH TrajectoryTimeline:
// spans are positioned by a shared timeline model (sequence or actual time);
// dragging emits a range in that SAME coordinate space, and the table dims
// rows outside via trajectoryTimelineFocusIndexes().

import React, { useMemo, useRef, useState, useCallback } from 'react';
import type { TrajectoryTurnModel, TrajectoryTimelineMode } from '../utils/layout';
import { deriveTrajectoryTimeline } from '../utils/layout';
import { formatDurationSeconds, formatTimestamp } from '../utils/format';
import { cn } from '../lib/utils';

interface TrajectoryTimelineProps {
  turns: readonly TrajectoryTurnModel[];
  mode: TrajectoryTimelineMode;
  range: { start: number; end: number } | null;
  searchMatchIndexes?: ReadonlySet<number> | null;
  onRangeChange: (range: { start: number; end: number } | null) => void;
  onRecordSelect?: (index: number) => void;
}

const MINIMUM_DRAG_PX = 3;

function orderedRange(left: number, right: number): { start: number; end: number } {
  return left <= right ? { start: left, end: right } : { start: right, end: left };
}

export function TrajectoryTimeline({
  turns,
  mode,
  range,
  searchMatchIndexes = null,
  onRangeChange,
  onRecordSelect,
}: TrajectoryTimelineProps) {
  const model = useMemo(() => deriveTrajectoryTimeline(turns, mode), [turns, mode]);
  const trackRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{
    pointerId: number;
    anchor: number;
    anchorClientX: number;
    recordIndex: number | null;
  } | null>(null);
  const [draft, setDraft] = useState<{ start: number; end: number } | null>(null);
  const [hover, setHover] = useState<{ fraction: number; recordIndex: number | null } | null>(null);

  const spans = model?.spans ?? [];
  const fullDuration = Math.max(1, (model?.end ?? 0) - (model?.start ?? 0));
  const modelStart = model?.start ?? 0;

  const valueAt = (fraction: number) => modelStart + fraction * fullDuration;
  const fractionToPercent = (value: number) => ((value - modelStart) / fullDuration) * 100;

  const recordIndexAt = (event: React.PointerEvent | React.MouseEvent): number | null => {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const el = target?.closest('[data-timeline-record-index]') as HTMLElement | null;
    const value = el?.dataset.timelineRecordIndex;
    if (value === undefined) return null;
    const index = Number(value);
    return Number.isFinite(index) ? index : null;
  };

  const fractionAt = (event: React.PointerEvent | React.MouseEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, (event.clientX - rect.left) / Math.max(1, rect.width)));
  };

  const handlePointerDown = useCallback((event: React.PointerEvent) => {
    if (event.button !== 0) return;
    const anchor = valueAt(fractionAt(event));
    const recordIndex = recordIndexAt(event);
    setHover({ fraction: fractionAt(event), recordIndex });
    dragRef.current = {
      pointerId: event.pointerId,
      anchor,
      anchorClientX: event.clientX,
      recordIndex,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDraft({ start: anchor, end: anchor });
  }, [valueAt, fractionAt, recordIndexAt]);

  const handlePointerMove = useCallback((event: React.PointerEvent) => {
    const fraction = fractionAt(event);
    setHover({ fraction, recordIndex: recordIndexAt(event) });
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    const point = valueAt(fraction);
    // Ignore sub-pixel jitter when nothing really moved
    if (
      Math.abs(event.clientX - drag.anchorClientX) < MINIMUM_DRAG_PX &&
      Math.abs(point - drag.anchor) < fullDuration / Math.max(1, spans.length)
    ) {
      return;
    }
    setDraft(orderedRange(drag.anchor, point));
  }, [valueAt, fractionAt, recordIndexAt, fullDuration, spans.length]);

  const handlePointerUp = useCallback((event: React.PointerEvent) => {
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    const point = valueAt(fractionAt(event));
    const selected = orderedRange(drag.anchor, point);
    const moved = Math.abs(event.clientX - drag.anchorClientX) >= MINIMUM_DRAG_PX;
    const replied = drag.recordIndex !== null;
    dragRef.current = null;
    setDraft(null);
    setHover({ fraction: fractionAt(event), recordIndex: recordIndexAt(event) });

    if (!moved && replied) {
      // A click onto a span: clear the range and select the record.
      onRangeChange(null);
      onRecordSelect?.(drag.recordIndex!);
      return;
    }
    if (!moved) {
      onRangeChange(null);
      return;
    }
    if (selected.end - selected.start > 0) {
      onRangeChange(selected);
    }
  }, [valueAt, fractionAt, recordIndexAt, onRangeChange, onRecordSelect]);

  const clearRange = useCallback(() => {
    dragRef.current = null;
    setDraft(null);
    onRangeChange(null);
  }, [onRangeChange]);

  const handlePointerCancel = useCallback(() => {
    dragRef.current = null;
    setDraft(null);
    setHover(null);
  }, []);

  const handleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key === 'Escape' && range !== null) {
      event.preventDefault();
      onRangeChange(null);
    }
  }, [range, onRangeChange]);

  const activeRange = draft ?? range;
  const hoveredSpan = hover !== null ? spans.find((s) => s.index === hover.recordIndex) : null;

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
        role="slider"
        aria-label="Trajectory timeline; drag to focus events"
        tabIndex={0}
        className="flex-1 relative cursor-crosshair overflow-hidden"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerCancel}
        onKeyDown={handleKeyDown}
        onDoubleClick={clearRange}
      >
        {spans.length === 0 && (
          <span className="absolute inset-0 flex items-center justify-center text-[11px] text-muted-foreground">
            No events
          </span>
        )}

        {/* Turn boundaries */}
        {model !== null &&
          model.turnBoundaries.map((b) => (
            <div
              key={`turn-${b.turn}`}
              className="absolute top-0 bottom-0 w-px bg-border/40 pointer-events-none"
              style={{ left: `${fractionToPercent(b.time)}%` }}
            />
          ))}

        {/* Spans */}
        <div className="absolute inset-2">
          {spans.map((span) => {
            const inRange = activeRange !== null && span.start <= activeRange.end && span.end >= activeRange.start;
            const selected = inRange === true;
            const dimmed = activeRange !== null && inRange === false;
            const searchMatch = searchMatchIndexes?.has(span.index) ?? false;

            return (
              <div
                key={span.index}
                data-timeline-record-index={span.index}
                data-timeline-span={span.kind}
                data-error={span.isError ? 'true' : undefined}
                data-selected={activeRange === null ? undefined : String(selected)}
                data-search-match={searchMatchIndexes === null ? undefined : String(searchMatch)}
                data-hovered={hover?.recordIndex === span.index ? 'true' : undefined}
                className={cn(
                  'absolute h-2.5 rounded-sm cursor-pointer transition-all',
                  span.kind === 'user' && 'bg-emerald-400 dark:bg-emerald-500',
                  span.kind === 'message' && 'bg-violet-400 dark:bg-violet-500',
                  (span.kind === 'tool' || span.kind === 'subtool') && 'bg-amber-400 dark:bg-amber-500',
                  span.kind === 'system' && 'bg-gray-400 dark:bg-gray-500',
                  span.kind === 'compacted' && 'bg-gray-300 dark:bg-gray-600',
                  dimmed && 'opacity-[0.2]',
                  selected && 'opacity-100 ring-2 ring-white/60 dark:ring-white/30',
                  hover?.recordIndex === span.index && 'opacity-100',
                  span.isError && 'ring-1 ring-red-400',
                )}
                style={{
                  left: `${fractionToPercent(span.start)}%`,
                  width: `${Math.max(fractionToPercent(span.end) - fractionToPercent(span.start), 0.5)}%`,
                  top: `${span.lane * 16 + 1}px`,
                }}
                onPointerEnter={() => setHover({ fraction: hover?.fraction ?? 0, recordIndex: span.index })}
                onPointerLeave={() => setHover(null)}
              />
            );
          })}
        </div>

        {/* Selection overlay — interior fill + dim the outside like DSH */}
        {activeRange !== null && (
          <div
            className="absolute top-0 bottom-0 pointer-events-none z-10"
            style={{
              left: `${fractionToPercent(activeRange.start)}%`,
              width: `${Math.max(fractionToPercent(activeRange.end) - fractionToPercent(activeRange.start), 0.5)}%`,
            }}
          >
            <div className="absolute inset-0 bg-blue-300/25 dark:bg-blue-500/20" />
            <div className="absolute left-0 top-0 bottom-0 w-[3px] bg-blue-500/80 dark:bg-blue-400/80 shadow-sm shadow-blue-500/50" />
            <div className="absolute right-0 top-0 bottom-0 w-[3px] bg-blue-500/80 dark:bg-blue-400/80 shadow-sm shadow-blue-500/50" />
          </div>
        )}

        {/* Hover tooltip */}
        {hoveredSpan !== undefined && hoveredSpan !== null && (
          <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-1 px-2 py-1 rounded bg-popover border border-border/40 shadow-md text-[10px] whitespace-nowrap z-20 pointer-events-none">
            <div className="font-medium">{hoveredSpan.label}</div>
            <div className="text-muted-foreground">
              {hoveredSpan.start >= 1e12
                ? formatTimestamp(hoveredSpan.start)
                : `#${hoveredSpan.index} · ${formatDurationSeconds((hoveredSpan.end - hoveredSpan.start) / 1000)}`}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}