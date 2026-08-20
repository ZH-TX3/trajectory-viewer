// ── Trajectory Timeline ──────────────────────────────────────────────────
//
// Chrome-Network-style timeline. Ported from DSH TrajectoryTimeline:
//   - spans are positioned by a shared timeline model (sequence or actual time)
//   - dragging emits a range in that SAME coordinate space; the table dims
//     rows outside via trajectoryTimelineFocusIndexes()
//   - a viewport (wheel zoom + right-drag pan + edge-pan while dragging) lets
//     you explore a long ledger without losing the full-timeline context

import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { TrajectoryTurnModel, TrajectoryTimelineMode } from '../utils/layout';
import { deriveTrajectoryTimeline, timelineRecordDetail } from '../utils/layout';
import { formatDurationMs, formatDurationSeconds, formatTimestamp } from '../utils/format';
import { cn } from '../lib/utils';

interface TrajectoryTimelineProps {
  turns: readonly TrajectoryTurnModel[];
  mode: TrajectoryTimelineMode;
  range: { start: number; end: number } | null;
  searchMatchIndexes?: ReadonlySet<number> | null;
  /** Currently selected record (clicked span) — drawn with a highlight ring. */
  selectedIndex?: number | null;
  onRangeChange: (range: { start: number; end: number } | null) => void;
  onRecordSelect?: (index: number) => void;
}

type Range = { start: number; end: number };

const MINIMUM_DRAG_PX = 3;
const MINIMUM_ZOOM_OPERATIONS = 4;
const EDGE_PAN_ZONE_FRACTION = 0.08;
const EDGE_PAN_STEP_FRACTION = 0.025;
const MAXIMUM_EDGE_PAN_PX = 32;

function clampFraction(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function orderedRange(left: number, right: number): Range {
  return left <= right ? { start: left, end: right } : { start: right, end: left };
}

function centeredRange(center: number, width: number, minimum: number, maximum: number): Range {
  const clampedWidth = Math.min(maximum - minimum, Math.max(0, width));
  const start = Math.min(Math.max(center - clampedWidth / 2, minimum), maximum - clampedWidth);
  return { start, end: start + clampedWidth };
}

/** Convert a model-space range into fractions of the visible viewport. */
function rangeFraction(
  range: Range,
  start: number,
  duration: number,
  minimum: number,
  maximum: number,
): Range {
  const bounded = orderedRange(
    Math.min(maximum, Math.max(minimum, range.start)),
    Math.min(maximum, Math.max(minimum, range.end)),
  );
  return {
    start: (bounded.start - start) / duration,
    end: (bounded.end - start) / duration,
  };
}

function LaneLabels() {
  return (
    <div className="w-11 shrink-0 border-r border-border/40 relative text-[9px] text-muted-foreground leading-none">
      <span className="absolute right-1 top-[7px]">Input</span>
      <span className="absolute right-1 top-[21px]">Model</span>
      <span className="absolute right-1 top-[35px]">Tools</span>
    </div>
  );
}

export function TrajectoryTimeline({
  turns,
  mode,
  range,
  searchMatchIndexes = null,
  selectedIndex = null,
  onRangeChange,
  onRecordSelect,
}: TrajectoryTimelineProps) {
  const model = useMemo(() => deriveTrajectoryTimeline(turns, mode), [turns, mode]);
  const detailByIndex = useMemo(
    () =>
      new Map(
        turns.flatMap((turn) =>
          turn.groups.flatMap((group) =>
            group.cells.map((cell) => [cell.index, timelineRecordDetail(cell)] as const),
          ),
        ),
      ),
    [turns],
  );

  const rootRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{
    pointerId: number;
    anchor: number;
    anchorClientX: number;
    recordIndex: number | null;
  } | null>(null);
  const panRef = useRef<{
    pointerId: number;
    anchorClientX: number;
    anchorStart: number;
    moved: boolean;
    pannable: boolean;
  } | null>(null);
  const [draft, setDraft] = useState<Range | null>(null);
  const [hover, setHover] = useState<{ fraction: number; recordIndex: number | null } | null>(null);
  const [panning, setPanning] = useState(false);
  const [viewport, setViewport] = useState<Range | null>(null);

  // When the model is replaced (e.g. duration mode switched), drop a stale
  // viewport that no longer overlaps the new coordinate space.
  useEffect(() => {
    if (model === null) return;
    setViewport((current) =>
      current !== null && (current.end < model.start || current.start > model.end) ? null : current,
    );
  }, [model]);

  const spans = model?.spans ?? [];
  const fullDuration = Math.max(1, (model?.end ?? 0) - (model?.start ?? 0));
  const viewportDuration = Math.min(
    fullDuration,
    Math.max(1, (viewport?.end ?? 0) - (viewport?.start ?? 0)),
  );
  const viewportStart =
    model === null || viewport === null
      ? model?.start ?? 0
      : Math.min(Math.max(viewport.start, model.start), model.end - viewportDuration);
  const domainStart = viewport === null ? model?.start ?? 0 : viewportStart;
  const domainDuration = viewport === null ? fullDuration : viewportDuration;

  // Wheel zooms the viewport anchored at the cursor.
  useEffect(() => {
    const root = rootRef.current;
    if (root === null || model === null) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const track = trackRef.current;
      if (track === null) return;
      const rect = track.getBoundingClientRect();
      const anchorFraction = clampFraction((event.clientX - rect.left) / Math.max(1, rect.width));
      const nextDuration = Math.min(
        fullDuration,
        Math.max(
          Math.min(mode === 'sequence' ? MINIMUM_ZOOM_OPERATIONS : 20, fullDuration),
          domainDuration * Math.exp(event.deltaY * 0.0015),
        ),
      );
      if (nextDuration >= fullDuration * 0.999) {
        setViewport(null);
        return;
      }
      const anchorTime = domainStart + anchorFraction * domainDuration;
      const nextStart = Math.min(
        Math.max(anchorTime - anchorFraction * nextDuration, model.start),
        model.end - nextDuration,
      );
      setViewport({ start: nextStart, end: nextStart + nextDuration });
    };
    root.addEventListener('wheel', onWheel, { passive: false });
    return () => root.removeEventListener('wheel', onWheel);
  }, [model, mode, fullDuration, domainStart, domainDuration]);

  const minimumSelectionDuration = model === null ? 0 : Math.min(domainDuration, fullDuration / Math.max(1, spans.length));

  const fractionAt = (event: React.PointerEvent | React.MouseEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return (event.clientX - rect.left) / Math.max(1, rect.width);
  };

  const recordIndexAt = (event: React.PointerEvent | React.MouseEvent): number | null => {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const el = target?.closest('[data-timeline-record-index]') as HTMLElement | null;
    const value = el?.dataset.timelineRecordIndex;
    if (value === undefined) return null;
    const index = Number(value);
    return Number.isFinite(index) ? index : null;
  };

  const valueAt = (fraction: number) => domainStart + fraction * domainDuration;

  const handlePointerDown = (event: React.PointerEvent) => {
    // Right button pans the viewport.
    if (event.button === 2) {
      panRef.current = {
        pointerId: event.pointerId,
        anchorClientX: event.clientX,
        anchorStart: domainStart,
        moved: false,
        pannable: viewport !== null,
      };
      setPanning(true);
      event.currentTarget.setPointerCapture?.(event.pointerId);
      return;
    }
    if (event.button !== 0) return;
    const fraction = clampFraction(fractionAt(event));
    const anchor = valueAt(fraction);
    const recordIndex = recordIndexAt(event);
    setHover({ fraction, recordIndex });
    dragRef.current = { pointerId: event.pointerId, anchor, anchorClientX: event.clientX, recordIndex };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDraft({ start: anchor, end: anchor });
  };

  const handlePointerMove = (event: React.PointerEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const fraction = clampFraction(fractionAt(event));
    setHover({ fraction, recordIndex: recordIndexAt(event) });

    const pan = panRef.current;
    if (pan !== null && pan.pointerId === event.pointerId) {
      if (Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX) pan.moved = true;
      if (!pan.pannable) return;
      const delta = (event.clientX - pan.anchorClientX) / Math.max(1, rect.width);
      const nextStart = Math.min(
        Math.max(pan.anchorStart - delta * domainDuration, model?.start ?? 0),
        (model?.end ?? 0) - domainDuration,
      );
      setViewport({ start: nextStart, end: nextStart + domainDuration });
      return;
    }

    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;

    // Edge-pan while dragging near either end extends the focused range.
    let nextDomainStart = domainStart;
    if (viewport !== null && model !== null) {
      const localX = event.clientX - rect.left;
      const edgeWidth = Math.min(MAXIMUM_EDGE_PAN_PX, Math.max(1, rect.width * EDGE_PAN_ZONE_FRACTION));
      const direction = localX < edgeWidth ? -1 : localX > rect.width - edgeWidth ? 1 : 0;
      if (direction !== 0) {
        const strength = clampFraction(
          (direction < 0 ? edgeWidth - localX : localX - (rect.width - edgeWidth)) / edgeWidth,
        );
        const desiredStart =
          domainStart + direction * domainDuration * EDGE_PAN_STEP_FRACTION * Math.max(0.2, strength);
        nextDomainStart = Math.min(Math.max(desiredStart, model.start), model.end - domainDuration);
        if (nextDomainStart !== domainStart) {
          setViewport({ start: nextDomainStart, end: nextDomainStart + domainDuration });
        }
      }
    }

    const point = nextDomainStart + fraction * domainDuration;
    setDraft(orderedRange(drag.anchor, point));
  };

  const handlePointerEnd = (event: React.PointerEvent) => {
    const pan = panRef.current;
    if (pan !== null && pan.pointerId === event.pointerId) {
      const moved = pan.moved || Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX;
      panRef.current = null;
      setPanning(false);
      if (!moved) onRangeChange(null);
      return;
    }

    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    const fraction = clampFraction(fractionAt(event));
    const point = valueAt(fraction);
    const selected = orderedRange(drag.anchor, point);
    const moved = Math.abs(event.clientX - drag.anchorClientX) >= MINIMUM_DRAG_PX;
    dragRef.current = null;
    setDraft(null);
    setHover({ fraction, recordIndex: recordIndexAt(event) });

    const clickedSpan =
      !moved && drag.recordIndex !== null
        ? spans.find((span) => span.index === drag.recordIndex)
        : undefined;
    if (clickedSpan !== undefined) {
      // Box-select (range) has priority over point-select: selecting a record
      // must NOT clear an existing range, so the range focus stays visible.
      onRecordSelect?.(clickedSpan.index);
      return;
    }
    if (!model) return;
    if (selected.end - selected.start < minimumSelectionDuration) {
      const center = moved ? (selected.start + selected.end) / 2 : selected.start;
      onRangeChange(centeredRange(center, minimumSelectionDuration, model.start, model.end));
    } else {
      onRangeChange(selected);
    }
  };

  const handlePointerCancel = () => {
    dragRef.current = null;
    panRef.current = null;
    setDraft(null);
    setHover(null);
    setPanning(false);
  };

  const clearRange = () => {
    dragRef.current = null;
    panRef.current = null;
    setDraft(null);
    onRangeChange(null);
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape' && range !== null) {
      event.preventDefault();
      onRangeChange(null);
    }
  };

  if (model === null) {
    return (
      <section
        ref={rootRef}
        aria-label="Trajectory timeline"
        className="h-[50px] border-b border-border/40 bg-muted/30 flex shrink-0 select-none"
      >
        <LaneLabels />
        <div className="flex-1 flex items-center justify-center text-[11px] text-muted-foreground">
          No timing data
        </div>
      </section>
    );
  }

  const projectedDomainStyle: React.CSSProperties = {
    left: `${(-(domainStart - model.start) / domainDuration) * 100}%`,
    width: `${(fullDuration / domainDuration) * 100}%`,
  };
  const visibleSpans = spans.filter(
    (span) => span.end >= domainStart && span.start <= domainStart + domainDuration,
  );
  const activeRange = draft ?? range;
  const committedFraction =
    range === null ? null : rangeFraction(range, domainStart, domainDuration, model.start, model.end);
  const draftFraction =
    draft === null ? null : rangeFraction(draft, domainStart, domainDuration, model.start, model.end);
  const visibleRange = draftFraction ?? committedFraction;
  const hoveredSpan = hover !== null ? spans.find((s) => s.index === hover.recordIndex) : null;
  const hoveredDetail = hoveredSpan != null ? detailByIndex.get(hoveredSpan.index) : undefined;

  return (
    <section
      ref={rootRef}
      aria-label="Trajectory timeline"
      className="h-[50px] border-b border-border/40 bg-muted/30 flex shrink-0 select-none"
    >
      <LaneLabels />

      {/* Track */}
      <div
        ref={trackRef}
        role="slider"
        aria-label="Trajectory timeline; drag to focus events; scroll to zoom; right-drag to pan"
        tabIndex={0}
        className={cn(
          'flex-1 relative overflow-hidden outline-none focus:outline-none',
          panning ? 'cursor-grabbing' : 'cursor-crosshair',
        )}
        style={{ outline: 'none' }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerEnd}
        onPointerCancel={handlePointerCancel}
        onPointerLeave={() => {
          if (dragRef.current === null && panRef.current === null) setHover(null);
        }}
        onDoubleClick={clearRange}
        onContextMenu={(event) => event.preventDefault()}
        onKeyDown={handleKeyDown}
      >
        {spans.length === 0 && (
          <span className="absolute inset-0 flex items-center justify-center text-[11px] text-muted-foreground">
            No events
          </span>
        )}

        {/* Hover line */}
        {hover !== null && hover.recordIndex === null && draft === null && (
          <div
            className="absolute top-0 bottom-0 w-px bg-border/60 pointer-events-none z-[5]"
            style={{ left: `${hover.fraction * 100}%` }}
          />
        )}

        {/* Viewport projection: turn boundaries + spans share this offset */}
        <div className="absolute inset-y-0 left-0" style={projectedDomainStyle}>
          {/* Turn boundaries */}
          {model.turnBoundaries
            .filter(
              (boundary) =>
                boundary.time > model.start &&
                boundary.time >= domainStart &&
                boundary.time <= domainStart + domainDuration,
            )
            .map((boundary) => (
              <div
                key={`turn-${boundary.turn}`}
                className="absolute top-0 bottom-0 w-px bg-border/40 pointer-events-none"
                style={{ left: `${((boundary.time - model.start) / fullDuration) * 100}%` }}
              />
            ))}

          {/* Spans */}
          {visibleSpans.map((span) => {
            const inRange = activeRange !== null && span.start <= activeRange.end && span.end >= activeRange.start;
            const selected = inRange === true;
            const dimmed = activeRange !== null && inRange === false;
            const searchMatch = searchMatchIndexes?.has(span.index) ?? false;
            const hovered = hover?.recordIndex === span.index;
            const activeSelected = selectedIndex === span.index;
            const highlighted = hovered || activeSelected;

            return (
              <div
                key={span.index}
                data-timeline-record-index={span.index}
                data-timeline-span={span.kind}
                data-error={span.isError ? 'true' : undefined}
                data-selected={activeRange === null ? undefined : String(selected)}
                data-search-match={searchMatchIndexes === null ? undefined : String(searchMatch)}
                data-hovered={hovered ? 'true' : undefined}
                data-current={activeSelected ? 'true' : undefined}
                className={cn(
                  'absolute h-2.5 rounded-sm pointer-events-auto',
                  span.kind === 'user' && 'bg-emerald-400 dark:bg-emerald-500',
                  span.kind === 'message' && 'bg-violet-400 dark:bg-violet-500',
                  (span.kind === 'tool' || span.kind === 'subtool') && 'bg-amber-400 dark:bg-amber-500',
                  span.kind === 'system' && 'bg-gray-400 dark:bg-gray-500',
                  span.kind === 'compacted' && 'bg-gray-300 dark:bg-gray-600',
                  dimmed && 'opacity-[0.2]',
                  selected && 'opacity-100',
                  highlighted && 'opacity-100',
                  span.isError && !highlighted && 'ring-1 ring-red-400',
                  hovered && 'ring-2 ring-blue-400/70 dark:ring-blue-300/80',
                  activeSelected && 'ring-2 ring-blue-500/80 dark:ring-blue-400/80',
                )}
                style={{
                  left: `${((span.start - model.start) / fullDuration) * 100}%`,
                  width: `${Math.max(((span.end - span.start) / fullDuration) * 100, 0.5)}%`,
                  top: `${span.lane * 16 + 1}px`,
                }}
                onPointerEnter={() => setHover({ fraction: hover?.fraction ?? 0, recordIndex: span.index })}
                onPointerLeave={() => setHover(null)}
              />
            );
          })}
        </div>

        {/* Selection overlay — interior fill + dim the outside like DSH */}
        {visibleRange !== null && (
          <div
            className="absolute top-0 bottom-0 pointer-events-none z-10"
            style={{
              left: `${visibleRange.start * 100}%`,
              width: `${Math.max((visibleRange.end - visibleRange.start) * 100, 0.5)}%`,
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
            <div>{hoveredSpan.label}</div>
            <div className="text-muted-foreground">
              {hoveredSpan.start >= 1e12
                ? formatTimestamp(hoveredSpan.start)
                : `#${hoveredSpan.index} · ${formatDurationSeconds((hoveredSpan.end - hoveredSpan.start) / 1000)}`}
            </div>
            {hoveredDetail !== undefined &&
              (hoveredDetail.ttftMs !== undefined || hoveredDetail.decodingMs !== undefined) && (
                <div className="text-muted-foreground">
                  Total {formatDurationMs(hoveredDetail.durationMs ?? null)} · TTFT{' '}
                  {formatDurationMs(hoveredDetail.ttftMs ?? null)} · Decoding{' '}
                  {formatDurationMs(hoveredDetail.decodingMs ?? null)}
                </div>
              )}
          </div>
        )}
      </div>
    </section>
  );
}