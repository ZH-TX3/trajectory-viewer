// ── Trajectory Table ─────────────────────────────────────────────────────
//
// Virtual-scrolled event ledger with a resizable detail panel.
// Uses @tanstack/react-virtual for efficient rendering of large datasets.

import { useCallback, useMemo, useRef, useState, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { TrajectoryCell } from './TrajectoryCell';
import { TrajectoryDetail } from './TrajectoryDetail';
import type { TrajectoryCellProps, TrajectoryTurnModel } from '../utils/layout';
import { aggregateRequestDetail, groupVirtualRows } from '../utils/layout';

interface TrajectoryTableProps {
  turns: readonly TrajectoryTurnModel[];
  streamingCells?: TrajectoryCellProps[];
  searchMatchIndexes?: ReadonlySet<number> | null;
  timelineFocusIndexes?: ReadonlySet<number> | null;
  collapsedTurns: ReadonlySet<number>;
  /** Timeline-clicked record; opens the detail panel and scrolls it into view. */
  selectedIndex?: number | null;
  _onToggleTurn?: (turn: number) => void;
  _collapsedAssistants?: ReadonlySet<string>;
  _onToggleAssistant?: (id: string) => void;
}

export function TrajectoryTable({
  turns,
  streamingCells = [],
  searchMatchIndexes = null,
  timelineFocusIndexes = null,
  collapsedTurns,
  selectedIndex = null,
  _collapsedAssistants = undefined,
}: TrajectoryTableProps) {
  const tablePaneRef = useRef<HTMLDivElement>(null);
  const [selectedRecord, setSelectedRecord] = useState<TrajectoryCellProps | null>(null);
  const [detailWidth, setDetailWidth] = useState(320);

  // Flatten turns into records
  const allRecords = useMemo(() => {
    const records: TrajectoryCellProps[] = [];
    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          records.push(cell);
        }
      }
    }
    for (const cell of streamingCells) {
      records.push(cell);
    }
    return records;
  }, [turns, streamingCells]);

  // Record index → its assistant step key (turn + group), for the Calls filter.
  const groupKeyByIndex = useMemo(() => {
    const map = new Map<number, string>();
    for (const turn of turns) {
      for (const group of turn.groups) {
        const key = `${turn.turn ?? 0}\0${group.title}`;
        for (const cell of group.cells) map.set(cell.index, key);
      }
    }
    return map;
  }, [turns]);

  // Apply filters: search, collapse turns, collapse calls
  const filteredRecords = useMemo(() => {
    let records = allRecords;

    if (searchMatchIndexes !== null) {
      records = records.filter((r) => searchMatchIndexes.has(r.index));
    }

    if (collapsedTurns.size > 0) {
      records = records.filter((r) => r.turn == null || !collapsedTurns.has(r.turn));
    }

    // Calls collapse: keep only the first row of each assistant step group.
    if (_collapsedAssistants !== undefined && _collapsedAssistants.size > 0) {
      const seenGroups = new Set<string>();
      records = records.filter((cell) => {
        const key = groupKeyByIndex.get(cell.index);
        if (key === undefined || !_collapsedAssistants.has(key)) return true;
        if (seenGroups.has(key)) return false;
        seenGroups.add(key);
        return true;
      });
    }

    return records;
  }, [allRecords, searchMatchIndexes, collapsedTurns, _collapsedAssistants, groupKeyByIndex]);

  // Virtual rows
  const virtualRows = useMemo(() => groupVirtualRows(filteredRecords), [filteredRecords]);

  const rowVirtualizer = useVirtualizer({
    count: virtualRows.length,
    getScrollElement: () => tablePaneRef.current,
    estimateSize: () => 30,
    overscan: 12,
    getItemKey: (index) => virtualRows[index]?.key ?? String(index),
  });

  // Auto-scroll to the first focused row when timeline range changes
  useEffect(() => {
    if (!timelineFocusIndexes || timelineFocusIndexes.size === 0 || !tablePaneRef.current) return;
    const firstFocusIndex = Math.min(...timelineFocusIndexes);
    const rowIndex = virtualRows.findIndex((r) =>
      r.entries.some((e) => e.cell.index === firstFocusIndex)
    );
    if (rowIndex >= 0) {
      rowVirtualizer.scrollToIndex(rowIndex, { align: 'center' });
    }
  }, [timelineFocusIndexes]);

  // A timeline-clicked record opens the detail panel and scrolls row into view;
// clearing it (selectedIndex → null, e.g. Escape) closes the detail panel.
  useEffect(() => {
    if (selectedIndex == null) {
      setSelectedRecord(null);
      return;
    }
    const cell = allRecords.find((r) => r.index === selectedIndex);
    if (cell) {
      setSelectedRecord((prev) => (prev?.index === selectedIndex ? prev : cell));
    }
    const rowIndex = virtualRows.findIndex((r) =>
      r.entries.some((e) => e.cell.index === selectedIndex)
    );
    if (rowIndex >= 0) {
      rowVirtualizer.scrollToIndex(rowIndex, { align: 'center' });
    }
  }, [selectedIndex]);

  const handleRecordClick = useCallback((cell: TrajectoryCellProps) => {
    setSelectedRecord((prev) => (prev?.index === cell.index ? null : cell));
  }, []);

  const handleCloseDetail = useCallback(() => {
    setSelectedRecord(null);
  }, []);

  // Aggregate the request (turn + group) around the selected record, so the
  // detail panel can show the whole assistant step like DSH does.
  const selectedRequest = useMemo(
    () => (selectedRecord ? aggregateRequestDetail(turns, selectedRecord.index) : null),
    [selectedRecord, turns],
  );

  // Summary stats
  const { totalTurns, totalCalls } = useMemo(() => {
    let tc = 0;
    // tool-call + tool-result share a callId; count distinct calls.
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
    return { totalTurns: tc, totalCalls: callIds.size };
  }, [turns]);

  return (
    <div className="flex flex-1 min-h-0 overflow-hidden">
      {/* Table pane */}
      <div ref={tablePaneRef} className="flex-1 overflow-auto relative" style={{ minWidth: 0 }}>
        <table className="w-full border-collapse">
          <colgroup>
            <col className="w-28" />
            <col />
          </colgroup>
          <tbody>
            <tr style={{ height: rowVirtualizer.getTotalSize() }}>
              <td colSpan={2} className="p-0 border-0">
                <div
                  style={{
                    height: rowVirtualizer.getTotalSize(),
                    position: 'relative',
                  }}
                >
                  {rowVirtualizer.getVirtualItems().map((virtualItem) => {
                    const row = virtualRows[virtualItem.index];
                    if (!row) return null;

                    return (
                      <div
                        key={row.key}
                        style={{
                          position: 'absolute',
                          top: 0,
                          left: 0,
                          width: '100%',
                          transform: `translateY(${virtualItem.start}px)`,
                        }}
                      >
                        <table className="w-full border-collapse">
                          <colgroup>
                            <col className="w-28" />
                            <col />
                          </colgroup>
                          <tbody>
                            {row.entries.map((entry) => (
                              <TrajectoryCell
                                key={entry.cell.index}
                                {...entry.cell}
                                onClick={() => handleRecordClick(entry.cell)}
                                selected={selectedRecord?.index === entry.cell.index}
                                searchMatch={searchMatchIndexes?.has(entry.cell.index)}
                                timelineFocus={
                                  timelineFocusIndexes === null || timelineFocusIndexes.size === 0
                                    ? undefined
                                    : timelineFocusIndexes.has(entry.cell.index)
                                      ? 'inside'
                                      : 'outside'
                                }
                              />
                            ))}
                          </tbody>
                        </table>
                      </div>
                    );
                  })}
                </div>
              </td>
            </tr>
          </tbody>
        </table>

        {/* Footer summary */}
        <div className="px-3 py-2 text-[11px] text-muted-foreground border-t border-border/40 flex gap-4">
          <span>{totalTurns} turns</span>
          <span>{totalCalls} calls</span>
          <span>{allRecords.length} records</span>
        </div>
      </div>

      {/* Detail panel */}
      {selectedRecord && (
        <TrajectoryDetail
          cell={selectedRecord}
          request={selectedRequest}
          onClose={handleCloseDetail}
          detailWidth={detailWidth}
          onWidthChange={setDetailWidth}
        />
      )}
    </div>
  );
}