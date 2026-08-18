// ── Trajectory View ──────────────────────────────────────────────────────
//
// Main trajectory view orchestrating toolbar, timeline, and table.

import { useCallback, useMemo, useState } from 'react';
import { TrajectoryToolbar } from './TrajectoryToolbar';
import { TrajectoryTimeline } from './TrajectoryTimeline';
import { TrajectoryTable } from './TrajectoryTable';
import { deriveTrajectoryLayout } from '../utils/layout';
import type { TrajectoryTurnModel } from '../utils/layout';
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

  // Derive layout from events
  const turns = useMemo(() => {
    if (!data) return [] as readonly TrajectoryTurnModel[];
    return deriveTrajectoryLayout(data.events);
  }, [data]);

  // Search index
  const searchMatchIndexes = useMemo(() => {
    if (!searchQuery.trim()) return null;
    const query = searchQuery.toLowerCase();
    const matches = new Set<number>();

    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          const searchText = [cell.text, cell.previewMarkdown, cell.inputDetail, cell.outputDetail, cell.toolName, cell.result]
            .filter(Boolean)
            .join(' ')
            .toLowerCase();

          if (searchText.includes(query)) {
            matches.add(cell.index);
          }
        }
      }
    }

    return matches;
  }, [turns, searchQuery]);

  // Timeline focus indexes
  const timelineFocusIndexes = useMemo(() => {
    if (!timelineRange) return null;
    const focus = new Set<number>();

    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          if (cell.index >= timelineRange.start && cell.index <= timelineRange.end) {
            focus.add(cell.index);
          }
        }
      }
    }

    return focus;
  }, [turns, timelineRange]);

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
      const ids = new Set<string>();
      for (const turn of turns) {
        for (const group of turn.groups) {
          for (const cell of group.cells) {
            if (cell.recordId) ids.add(cell.recordId);
          }
        }
      }
      setCollapsedAssistants(ids);
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
    setActualDuration((prev) => !prev);
  }, []);

  // Count turns and calls
  const { turnCount, callCount } = useMemo(() => {
    let tc = 0;
    let cc = 0;
    for (const turn of turns) {
      if (turn.turn != null) tc++;
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          if (cell.kind === 'tool' || cell.kind === 'subtool') cc++;
        }
      }
    }
    return { turnCount: tc, callCount: cc };
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
        actualDuration={actualDuration}
        searchMatchIndexes={searchMatchIndexes}
        onRangeChange={setTimelineRange}
      />

      {/* Table */}
      <TrajectoryTable
        turns={turns}
        searchMatchIndexes={searchMatchIndexes}
        timelineFocusIndexes={timelineFocusIndexes}
        collapsedTurns={collapsedTurns}
        _onToggleTurn={handleToggleTurn}
        _collapsedAssistants={collapsedAssistants}
        _onToggleAssistant={handleToggleAssistant}
      />
    </div>
  );
}