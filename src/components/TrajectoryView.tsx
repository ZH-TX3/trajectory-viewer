// ── Trajectory View ──────────────────────────────────────────────────────
//
// Main trajectory view orchestrating toolbar, timeline, and table.
//
// It also owns the subagent drill-down stack: a dispatch row can be opened as
// a full trajectory of its own ("主线 › Explore › d2 Explore"), rendered by the
// same body, so the nested view keeps the timeline, detail panel and search.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TrajectoryToolbar } from './TrajectoryToolbar';
import { TrajectoryTimeline } from './TrajectoryTimeline';
import { TrajectoryTable } from './TrajectoryTable';
import {
  deriveTrajectoryLayout,
  flattenCells,
  trajectoryRecordId,
  trajectoryTimelineFocusIndexes,
} from '../utils/layout';
import { TrajectorySearchIndex } from '../utils/layout';
import type { TrajectoryCellProps, TrajectoryTurnModel, TrajectoryTimelineMode } from '../utils/layout';
import type { SubagentRun, TrajectoryData } from '../types';
import { api } from '../api';

interface TrajectoryViewProps {
  data: TrajectoryData;
  /**
   * Timestamp of the message to scroll to and open. Set when arriving from a
   * cross-session search result, so the table lands on the exact match rather
   * than highlighting every occurrence of the query.
   */
  focusTs?: number | null;
  /**
   * The session file on disk. Subagent transcripts live beside it, so this is
   * what makes drill-down and inline expansion possible; without it the view
   * stays flat.
   */
  sourcePath?: string | null;
}

interface DrillEntry {
  run: SubagentRun;
  data: TrajectoryData;
}

export function TrajectoryView({ data, focusTs, sourcePath }: TrajectoryViewProps) {
  // One entry per drilled-in subagent; the last one is what's on screen.
  const [stack, setStack] = useState<DrillEntry[]>([]);
  const [drillError, setDrillError] = useState<string | null>(null);

  // The root session changed — a drill path from the previous one is stale.
  useEffect(() => {
    setStack([]);
    setDrillError(null);
  }, [sourcePath]);

  const active = stack.length > 0 ? stack[stack.length - 1] : null;

  const handleOpenSubagent = useCallback(
    async (run: SubagentRun) => {
      if (sourcePath == null) return;
      setDrillError(null);
      try {
        const sub = await api.getSubagentTrajectory(sourcePath, run.agentId);
        setStack((prev) => [...prev, { run, data: sub }]);
      } catch (error) {
        setDrillError(String(error));
      }
    },
    [sourcePath],
  );

  return (
    <div className="flex flex-col h-full min-h-0">
      {stack.length > 0 && (
        <nav
          aria-label="Subagent drill path"
          className="flex items-center gap-1 px-3 py-1 text-[11px] border-b border-border/40 bg-muted/30 shrink-0 min-w-0"
        >
          <button
            type="button"
            onClick={() => setStack([])}
            className="shrink-0 text-cyan-700 dark:text-cyan-300 hover:underline"
          >
            main session
          </button>
          {stack.map((entry, level) => (
            <span key={entry.run.agentId} className="flex items-center gap-1 min-w-0">
              <span className="text-muted-foreground/60 shrink-0">›</span>
              <button
                type="button"
                disabled={level === stack.length - 1}
                onClick={() => setStack((prev) => prev.slice(0, level + 1))}
                className={
                  level === stack.length - 1
                    ? 'truncate text-foreground/80 cursor-default'
                    : 'truncate text-cyan-700 dark:text-cyan-300 hover:underline'
                }
              >
                {subagentLabel(entry.run)}
              </button>
            </span>
          ))}
          <button
            type="button"
            onClick={() => setStack((prev) => prev.slice(0, -1))}
            className="shrink-0 ml-auto px-1.5 py-0.5 rounded border border-border/60 text-muted-foreground hover:text-foreground hover:bg-background"
          >
            ← back
          </button>
        </nav>
      )}

      {drillError !== null && (
        <div className="px-3 py-1 text-[11px] text-red-600 dark:text-red-400 border-b border-border/40 shrink-0">
          Could not open subagent: {drillError}
        </div>
      )}

      <TrajectoryBody
        key={active?.run.agentId ?? 'root'}
        data={active?.data ?? data}
        focusTs={focusTs}
        sourcePath={sourcePath}
        onOpenSubagent={sourcePath == null ? undefined : handleOpenSubagent}
      />
    </div>
  );
}

/** Breadcrumb text for a drilled-in run. */
function subagentLabel(run: SubagentRun): string {
  if (run.name != null && run.name !== '') return run.name;
  if (run.description != null && run.description !== '') return run.description;
  return run.agentType ?? run.agentId;
}

// ── Body ─────────────────────────────────────────────────────────────────
//
// The ledger itself: toolbar + timeline + table. Mounted once per drill level
// (keyed by agent id), so each level keeps its own search, collapse and
// selection state.

interface TrajectoryBodyProps {
  data: TrajectoryData;
  focusTs?: number | null;
  sourcePath?: string | null;
  onOpenSubagent?: (run: SubagentRun) => void;
}

function TrajectoryBody({ data, focusTs, sourcePath, onOpenSubagent }: TrajectoryBodyProps) {
  // UI state
  const [searchQuery, setSearchQuery] = useState('');
  const [collapsedTurns, setCollapsedTurns] = useState<Set<number>>(new Set());
  const [collapsedAssistants, setCollapsedAssistants] = useState<Set<string>>(new Set());
  const [actualDuration, setActualDuration] = useState(false);
  const [timelineRange, setTimelineRange] = useState<{ start: number; end: number } | null>(null);
  const [selectedRecordIndex, setSelectedRecordIndex] = useState<number | null>(null);

  // Inline-expanded subagent transcripts, keyed by agent id. Loaded on demand:
  // one session can hold a dozen runs and thousands of events between them.
  const [expandedAgents, setExpandedAgents] = useState<Set<string>>(new Set());
  const [transcripts, setTranscripts] = useState<Map<string, TrajectoryCellProps[]>>(new Map());
  const [pendingAgents, setPendingAgents] = useState<Set<string>>(new Set());
  const [expandError, setExpandError] = useState<string | null>(null);

  // Derive layout from events
  const turns = useMemo(
    () => deriveTrajectoryLayout(data.events, data.subagents),
    [data],
  );

  // A transcript larger than this is not spliced into the ledger — a 400-event
  // run would bury the main session around it. The drill-down view is the way
  // in instead.
  const INLINE_LIMIT = 200;

  // Each transcript is laid out independently, so its record indexes restart
  // at 0 and would collide with the main ledger's — a nested row would then
  // inherit the selection highlight (and request aggregation) of an unrelated
  // main row. Rebase them into a disjoint range instead.
  const maxCellIndex = useMemo(
    () => flattenCells(turns).reduce((max, cell) => Math.max(max, cell.index + 1), 0),
    [turns],
  );
  const nextIndexRef = useRef(0);

  const loadTranscript = useCallback(
    async (run: SubagentRun) => {
      if (sourcePath == null) return;
      setExpandError(null);
      setPendingAgents((prev) => new Set(prev).add(run.agentId));
      try {
        const sub = await api.getSubagentTrajectory(sourcePath, run.agentId);
        const cells = flattenCells(deriveTrajectoryLayout(sub.events, sub.subagents));
        if (cells.length > INLINE_LIMIT) {
          // Too large to inline: undo the expansion and hand off to the
          // drill-down, which can render it with its own timeline.
          setExpandedAgents((prev) => {
            const next = new Set(prev);
            next.delete(run.agentId);
            return next;
          });
          onOpenSubagent?.(run);
          return;
        }
        const base = Math.max(nextIndexRef.current, maxCellIndex);
        nextIndexRef.current = base + cells.length;
        setTranscripts((prev) =>
          new Map(prev).set(
            run.agentId,
            cells.map((cell, offset) => ({ ...cell, index: base + offset })),
          ),
        );
      } catch (error) {
        setExpandError(String(error));
        setExpandedAgents((prev) => {
          const next = new Set(prev);
          next.delete(run.agentId);
          return next;
        });
      } finally {
        setPendingAgents((prev) => {
          const next = new Set(prev);
          next.delete(run.agentId);
          return next;
        });
      }
    },
    [sourcePath, onOpenSubagent, maxCellIndex],
  );

  const handleToggleExpand = useCallback(
    (run: SubagentRun) => {
      const collapsing = expandedAgents.has(run.agentId);
      setExpandedAgents((prev) => {
        const next = new Set(prev);
        if (collapsing) next.delete(run.agentId);
        else next.add(run.agentId);
        return next;
      });
      if (!collapsing && !transcripts.has(run.agentId)) void loadTranscript(run);
    },
    [expandedAgents, transcripts, loadTranscript],
  );

  // When told to focus a message (cross-session search result), select and
  // open the record whose start time is closest to the target. The first
  // source row matching the ts wins.
  useEffect(() => {
    if (focusTs == null || turns.length === 0) return;
    let bestIndex: number | null = null;
    let bestDiff = Infinity;
    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          const start = cell.startedAt;
          if (start == null || !Number.isFinite(start)) continue;
          const diff = Math.abs(start - focusTs);
          if (diff < bestDiff) {
            bestDiff = diff;
            bestIndex = cell.index;
          }
        }
      }
    }
    if (bestIndex !== null) setSelectedRecordIndex(bestIndex);
  }, [focusTs, turns]);

  // Search index (ported from DSH TrajectorySearchIndex): one stable index
  // keyed by record id, rebuilt as layouts change, queried for the input.
  const searchIndexRef = useRef<TrajectorySearchIndex | null>(null);
  if (searchIndexRef.current === null) searchIndexRef.current = new TrajectorySearchIndex();
  const [searchIndexRevision, setSearchIndexRevision] = useState(0);
  const searchLayouts = useMemo(() => [turns] as TrajectoryTurnModel[][], [turns]);

  useEffect(() => {
    const index = searchIndexRef.current;
    if (index !== null && index.update(searchLayouts)) {
      setSearchIndexRevision((revision) => revision + 1);
    }
  }, [searchLayouts]);

  const searchMatchIndexes = useMemo(() => {
    const index = searchIndexRef.current;
    if (index === null) return null;
    // Re-query whenever the index changes below.
    void searchIndexRevision;
    const recordIds = index.search(searchQuery);
    if (recordIds === null) return null;

    const matches = new Set<number>();
    for (const turnsSlice of searchLayouts) {
      for (const turn of turnsSlice) {
        for (const group of turn.groups) {
          for (const cell of group.cells) {
            if (recordIds.has(trajectoryRecordId(cell))) matches.add(cell.index);
          }
        }
      }
    }
    return matches;
  }, [searchIndexRevision, searchQuery, searchLayouts]);

  // The timeline projects every record into a shared coordinate space; the
  // drag range lives in that space. Map it back to the exact record indexes.
  const timelineMode: TrajectoryTimelineMode = actualDuration ? 'actual' : 'sequence';
  const timelineFocusIndexes = useMemo(
    () =>
      timelineRange === null
        ? null
        : trajectoryTimelineFocusIndexes(turns, timelineRange, timelineMode),
    [turns, timelineRange, timelineMode],
  );

  const handleTimelineRangeChange = useCallback(
    (range: { start: number; end: number } | null) => {
      setTimelineRange(range);
    },
    [],
  );

  // Collapse/expand handlers
  const allTurnsCollapsed = collapsedTurns.size > 0;
  const allAssistantsCollapsed = collapsedAssistants.size > 0;

  const handleToggleAllTurns = useCallback(() => {
    if (allTurnsCollapsed) {
      setCollapsedTurns(new Set());
    } else {
      const turnNumbers = new Set<number>();
      for (const turn of turns) {
        if (turn.turn != null) turnNumbers.add(turn.turn);
      }
      setCollapsedTurns(turnNumbers);
    }
  }, [allTurnsCollapsed, turns]);

  const handleToggleAllAssistants = useCallback(() => {
    if (allAssistantsCollapsed) {
      setCollapsedAssistants(new Set());
    } else {
      // Collapse each assistant step (= turn + group) to its first row.
      const keys = new Set<string>();
      for (const turn of turns) {
        for (const group of turn.groups) keys.add(`${turn.turn ?? 0}\0${group.title}`);
      }
      setCollapsedAssistants(keys);
    }
  }, [allAssistantsCollapsed, turns]);

  const handleToggleTurn = useCallback((turn: number) => {
    setCollapsedTurns((prev) => {
      const next = new Set(prev);
      if (next.has(turn)) next.delete(turn);
      else next.add(turn);
      return next;
    });
  }, []);

  const handleToggleAssistant = useCallback((id: string) => {
    setCollapsedAssistants((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleToggleDuration = useCallback(() => {
    // Switching the projection invalidates the current range selection.
    setActualDuration((prev) => !prev);
    setTimelineRange(null);
  }, []);

  // Escape clears both the box-select range and the point-selected record,
  // returning the view to its default state.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setTimelineRange(null);
        setSelectedRecordIndex(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Count turns and calls
  const { turnCount, callCount } = useMemo(() => {
    let tc = 0;
    // A single tool invocation yields a tool-call AND a tool-result cell, both
    // kind "tool"; count distinct calls by callId instead of cells.
    const callIds = new Set<string>();
    for (const turn of turns) {
      if (turn.turn != null) tc++;
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          if (cell.kind === 'tool' || cell.kind === 'subtool') {
            callIds.add(cell.callId ?? `${cell.kind}:${cell.index}`);
          }
        }
      }
    }
    return { turnCount: tc, callCount: callIds.size };
  }, [turns]);

  return (
    <div className="flex flex-col h-full min-h-0 bg-background">
      {/* Toolbar */}
      <TrajectoryToolbar
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        onToggleAllTurns={handleToggleAllTurns}
        onToggleAllAssistants={handleToggleAllAssistants}
        allTurnsCollapsed={allTurnsCollapsed}
        allAssistantsCollapsed={allAssistantsCollapsed}
        actualDuration={actualDuration}
        onToggleDuration={handleToggleDuration}
        turnCount={turnCount}
        callCount={callCount}
      />

      {expandError !== null && (
        <div className="px-3 py-1 text-[11px] text-red-600 dark:text-red-400 border-b border-border/40 shrink-0">
          Could not load subagent transcript: {expandError}
        </div>
      )}

      {/* Timeline */}
      <TrajectoryTimeline
        turns={turns}
        mode={timelineMode}
        range={timelineRange}
        searchMatchIndexes={searchMatchIndexes}
        selectedIndex={selectedRecordIndex}
        onRangeChange={handleTimelineRangeChange}
        onRecordSelect={setSelectedRecordIndex}
      />

      {/* Table */}
      <TrajectoryTable
        turns={turns}
        searchMatchIndexes={searchMatchIndexes}
        timelineFocusIndexes={timelineFocusIndexes}
        collapsedTurns={collapsedTurns}
        selectedIndex={selectedRecordIndex}
        _onToggleTurn={handleToggleTurn}
        _collapsedAssistants={collapsedAssistants}
        _onToggleAssistant={handleToggleAssistant}
        transcripts={transcripts}
        expandedAgents={expandedAgents}
        pendingAgents={pendingAgents}
        onToggleExpand={sourcePath == null ? undefined : handleToggleExpand}
        onOpenSubagent={onOpenSubagent}
      />
    </div>
  );
}
