// ── Trajectory View ──────────────────────────────────────────────────────
//
// Main trajectory view orchestrating toolbar, timeline, and table.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TrajectoryToolbar } from './TrajectoryToolbar';
import { TrajectoryTimeline } from './TrajectoryTimeline';
import { TrajectoryTable } from './TrajectoryTable';
import {
  deriveTrajectoryLayout,
  trajectoryRecordId,
  trajectoryTimelineFocusIndexes,
} from '../utils/layout';
import { TrajectorySearchIndex } from '../utils/layout';
import type { TrajectoryTurnModel, TrajectoryTimelineMode } from '../utils/layout';
import type { TrajectoryData } from '../types';

interface TrajectoryViewProps {
  data: TrajectoryData;
}

export function TrajectoryView({ data }: TrajectoryViewProps) {
  // UI state
  const [searchQuery, setSearchQuery] = useState('');
  const [collapsedTurns, setCollapsedTurns] = useState<Set<number>>(new Set());
  const [collapsedAssistants, setCollapsedAssistants] = useState<Set<string>>(new Set());
  const [actualDuration, setActualDuration] = useState(false);
  const [timelineRange, setTimelineRange] = useState<{ start: number; end: number } | null>(null);
  const [selectedRecordIndex, setSelectedRecordIndex] = useState<number | null>(null);

  // Derive layout from events
  const turns = useMemo(() => {
    if (!data) return [] as readonly TrajectoryTurnModel[];
    return deriveTrajectoryLayout(data.events);
  }, [data]);

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
      />
    </div>
  );
}