// ── Trajectory Detail Panel ──────────────────────────────────────────────
//
// Detail/inspection panel for a selected trajectory record.
// 5 tabs: Summary, Payload, Result, Timing, Usage.
// Supports resizable width via drag handle.

import React, { useState, useRef, useCallback, useEffect } from 'react';
import type { TrajectoryCellProps } from '../utils/layout';
import { formatDurationMs, formatTimestamp, formatTokenCount, KIND_LABEL } from '../utils/format';
import { cn } from '../lib/utils';
import { X, GripVertical } from 'lucide-react';

interface TrajectoryDetailProps {
  cell: TrajectoryCellProps;
  onClose: () => void;
  detailWidth: number;
  onWidthChange: (width: number) => void;
}

type DetailTab = 'summary' | 'payload' | 'result' | 'timing' | 'usage';

export function TrajectoryDetail({ cell, onClose, detailWidth, onWidthChange }: TrajectoryDetailProps) {
  const [activeTab, setActiveTab] = useState<DetailTab>('summary');
  const isDragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const newWidth = Math.max(200, Math.min(600, window.innerWidth - e.clientX));
      onWidthChange(newWidth);
    };
    const handleMouseUp = () => {
      if (isDragging.current) {
        isDragging.current = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      }
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [onWidthChange]);

  const tabs: Array<{ id: DetailTab; label: string }> = [
    { id: 'summary', label: 'Summary' },
    { id: 'payload', label: 'Payload' },
    { id: 'result', label: 'Result' },
    { id: 'timing', label: 'Timing' },
    { id: 'usage', label: 'Usage' },
  ];

  const kindLabel = KIND_LABEL[cell.kind] ?? cell.kind;

  return (
    <aside
      className="border-l border-border/40 bg-background flex flex-col overflow-hidden shrink-0 relative"
      style={{ width: detailWidth }}
    >
      {/* Drag handle on the left edge */}
      <div
        onMouseDown={handleMouseDown}
        className="absolute left-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 transition-colors z-10 group"
      >
        <div className="absolute inset-y-0 -left-1 -right-1" />
        <GripVertical className="absolute top-1/2 -translate-y-1/2 -left-[3px] size-3 text-muted-foreground/0 group-hover:text-muted-foreground/50 transition-colors" />
      </div>

      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border/40 pl-4">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[10px] font-mono text-muted-foreground shrink-0">
            #{cell.index}
          </span>
          <span className="text-xs font-medium truncate">{kindLabel}</span>
        </div>
        <button onClick={onClose} className="shrink-0 p-0.5 rounded hover:bg-muted/80 transition-colors">
          <X className="size-3.5" />
        </button>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-border/40 overflow-x-auto shrink-0">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              'px-3 py-1.5 text-[11px] font-medium transition-colors shrink-0',
              activeTab === tab.id
                ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 font-medium shadow-sm ring-1 ring-blue-300 dark:ring-blue-700 border-b-2 border-blue-500 dark:border-blue-400'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted/40',
            )}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-3 text-xs space-y-2">
        {activeTab === 'summary' && <SummaryTab cell={cell} />}
        {activeTab === 'payload' && <PayloadTab cell={cell} />}
        {activeTab === 'result' && <ResultTab cell={cell} />}
        {activeTab === 'timing' && <TimingTab cell={cell} />}
        {activeTab === 'usage' && <UsageTab cell={cell} />}
      </div>
    </aside>
  );
}

function SummaryTab({ cell }: { cell: TrajectoryCellProps }) {
  const items: Array<[string, string]> = [
    ['Type', KIND_LABEL[cell.kind] ?? cell.kind],
    ['Index', String(cell.index)],
    ['Time', formatTimestamp(cell.startedAt ?? null)],
    ['Duration', cell.timeSeconds != null ? formatDurationMs(cell.timeSeconds * 1000) : '—'],
    ['Tool', cell.toolName ?? '—'],
    ['Error', cell.isError ? 'Yes' : 'No'],
  ];

  return (
    <div className="space-y-1">
      {items.map(([label, value]) => (
        <div key={label} className="flex justify-between py-0.5">
          <span className="text-muted-foreground">{label}</span>
          <span className="font-mono text-foreground/80 text-right max-w-[60%] truncate">{value}</span>
        </div>
      ))}
    </div>
  );
}

function PayloadTab({ cell }: { cell: TrajectoryCellProps }) {
  const content = cell.inputDetail ?? cell.outputDetail ?? '';
  if (!content) return <span className="text-muted-foreground">No payload data</span>;

  return (
    <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
      {safeJsonFormat(content)}
    </pre>
  );
}

function ResultTab({ cell }: { cell: TrajectoryCellProps }) {
  const result = cell.result ?? cell.outputDetail ?? '';
  if (!result) return <span className="text-muted-foreground">No result data</span>;

  return (
    <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
      {safeJsonFormat(result)}
    </pre>
  );
}

function TimingTab({ cell }: { cell: TrajectoryCellProps }) {
  const items: Array<[string, string]> = [
    ['Started at', formatTimestamp(cell.startedAt ?? null)],
    ['Duration', cell.timeSeconds != null ? formatDurationMs(cell.timeSeconds * 1000) : '—'],
  ];

  if (cell.assistantMetrics) {
    items.push([
      'TTFT',
      cell.assistantMetrics.firstTokenTime != null
        ? formatDurationMs(cell.assistantMetrics.firstTokenTime - (cell.startedAt ?? 0))
        : '—',
    ]);
    items.push(['Completed', formatTimestamp(cell.assistantMetrics.completedTime)]);
  }

  return (
    <div className="space-y-1">
      {items.map(([label, value]) => (
        <div key={label} className="flex justify-between py-0.5">
          <span className="text-muted-foreground">{label}</span>
          <span className="font-mono text-foreground/80">{value}</span>
        </div>
      ))}
    </div>
  );
}

function UsageTab({ cell }: { cell: TrajectoryCellProps }) {
  const items: Array<[string, string]> = [
    ['Input tokens', formatTokenCount(cell.input)],
    ['Output tokens', formatTokenCount(cell.output)],
    ['Reasoning tokens', formatTokenCount(cell.think)],
    ['Cache read', formatTokenCount(cell.cacheRead)],
    ['Cache write', formatTokenCount(cell.cacheWrite)],
  ];

  return (
    <div className="space-y-1">
      {items.map(([label, value]) => (
        <div key={label} className="flex justify-between py-0.5">
          <span className="text-muted-foreground">{label}</span>
          <span className="font-mono text-foreground/80">{value}</span>
        </div>
      ))}
    </div>
  );
}

function safeJsonFormat(text: string): string {
  try {
    const parsed = JSON.parse(text);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return text;
  }
}