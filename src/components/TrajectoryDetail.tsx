// ── Trajectory Detail Panel ──────────────────────────────────────────────
//
// Detail/inspection panel for the selected trajectory record. Ported from
// DSH TrajectoryTable's inspector:
//   - tabs are DIFFERENTIATED by what was selected (a request aggregates the
//     whole assistant step; a record shows its own kind-specific tabs)
//   - Summary is an aggregated overview (status / tokens / timing / preview),
//     with jump links into the matching tab
// Supports resizable width via drag handle.

import React, { useState } from 'react';
import type { TrajectoryCellProps, TrajectoryRequestDetail, TrajectoryUsage } from '../utils/layout';
import { timelineRecordDetail } from '../utils/layout';
import {
  formatDurationMs,
  formatTimestamp,
  formatTokenCount,
  formatThroughput,
  KIND_LABEL,
} from '../utils/format';
import { cn } from '../lib/utils';
import { X, ChevronRight } from 'lucide-react';

interface TrajectoryDetailProps {
  cell: TrajectoryCellProps;
  request: TrajectoryRequestDetail | null;
  onClose: () => void;
  detailWidth: number;
  onWidthChange: (width: number) => void;
}

type TabId =
  | 'overview'
  | 'usage'
  | 'timing'
  | 'rendered'
  | 'raw'
  | 'payload'
  | 'result'
  | 'schema';

interface TabDef {
  id: TabId;
  label: string;
}

/** Whether a record has renderable preview content (Markdown / result). */
function hasPreview(cell: TrajectoryCellProps): boolean {
  return (
    cell.previewMarkdown != null ||
    cell.resultPreviewMarkdown != null ||
    cell.result != null
  );
}

function previewContent(cell: TrajectoryCellProps): string {
  return (
    cell.previewMarkdown ??
    cell.resultPreviewMarkdown ??
    cell.result ??
    cell.outputDetail ??
    ''
  );
}

/** Differentiated tabs per selected object, mirroring DSH detailTabs(). */
function detailTabs(
  cell: TrajectoryCellProps,
  request: TrajectoryRequestDetail | null,
): TabDef[] {
  if (request !== null) {
    return [
      { id: 'overview', label: 'Summary' },
      ...(hasPreview(cell) ? [{ id: 'rendered', label: 'Preview' } as TabDef] : []),
      ...(request.usage !== undefined ? [{ id: 'usage', label: 'Usage' } as TabDef] : []),
      ...(request.assistant !== undefined ? [{ id: 'timing', label: 'Timing' } as TabDef] : []),
    ];
  }
  switch (cell.kind) {
    case 'system':
      return [
        { id: 'overview', label: 'Summary' },
        { id: 'raw', label: 'Raw' },
      ];
    case 'compacted':
      return [
        { id: 'overview', label: 'Summary' },
        { id: 'raw', label: 'Raw Output' },
      ];
    case 'user':
    case 'context':
    case 'message':
      return [
        { id: 'overview', label: 'Summary' },
        ...(hasPreview(cell) ? [{ id: 'rendered', label: 'Preview' } as TabDef] : []),
        { id: 'raw', label: 'Raw' },
      ];
    default: // tool / subtool
      return [
        { id: 'overview', label: 'Summary' },
        ...(hasPreview(cell) ? [{ id: 'rendered', label: 'Preview' } as TabDef] : []),
        ...(cell.inputDetail !== undefined
          ? [{ id: 'payload', label: 'Payload' } as TabDef]
          : []),
        ...(cell.result ?? cell.outputDetail
          ? [{ id: 'result', label: 'Result' } as TabDef]
          : []),
        ...(cell.schemaDetail !== undefined
          ? [{ id: 'schema', label: 'Schema' } as TabDef]
          : []),
        { id: 'timing', label: 'Timing' },
      ];
  }
}

function stateLabel(state: 'complete' | 'error'): string {
  return state === 'error' ? 'Error' : 'Complete';
}

function OverviewSection({
  label,
  onOpen,
  children,
}: {
  label: string;
  onOpen?: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="overview-section">
      <button
        type="button"
        onClick={onOpen}
        className={cn(
          'w-full text-left text-[11px] text-muted-foreground flex items-center justify-between py-1 border-y border-border/10',
          onOpen && 'hover:text-foreground hover:bg-blue-50 dark:hover:bg-blue-900/20 transition-colors',
        )}
      >
        <span>{label}</span>
        {onOpen && <ChevronRight className="size-3.5 text-foreground/40 group-hover:text-foreground shrink-0" aria-hidden />}
      </button>
      {children}
    </div>
  );
}

function KeyValue({ label, value, error }: { label: string; value: React.ReactNode; error?: boolean }) {
  return (
    <div className="flex justify-between items-baseline py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className={cn('font-mono text-foreground/80 text-right break-all', error && 'text-red-600 dark:text-red-400')}>
        {value}
      </span>
    </div>
  );
}

/** Message token usage rows — DSH TokenRows. */
function TokenRows({ cell }: { cell: TrajectoryCellProps }) {
  const rows: Array<[string, React.ReactNode]> = [];
  const push = (label: string, value: React.ReactNode) => {
    if (value !== '—') rows.push([label, value]);
  };
  push('Input tokens', formatTokenCount(cell.input));
  push('Output tokens', formatTokenCount(cell.output));
  if (cell.think !== undefined) push('Reasoning tokens', formatTokenCount(cell.think));
  push('Cache read', formatTokenCount(cell.cacheRead));
  push('Cache write', formatTokenCount(cell.cacheWrite));

  if (rows.length === 0) return null;
  return <>{rows.map(([label, value]) => <KeyValue key={label} label={label} value={value} />)}</>;
}

function spanMs(cell: TrajectoryCellProps): number | undefined {
  if (cell.timeSeconds === null || cell.timeSeconds === undefined || !Number.isFinite(cell.timeSeconds)) {
    return undefined;
  }
  return Math.max(0, cell.timeSeconds * 1000);
}

/** DSH AssistantTimingPanel: Started / Total / TTFT / Generation / Throughput. */
function TimingPanel({ cell }: { cell: TrajectoryCellProps }) {
  const metrics = cell.assistantMetrics;
  const detail = timelineRecordDetail(cell);

  if (metrics === undefined) {
    return (
      <dl className="space-y-1">
        <KeyValue label="Started" value={formatTimestamp(cell.startedAt)} />
        <KeyValue label="Duration" value={spanMs(cell) != null ? formatDurationMs(spanMs(cell)!) : '—'} />
      </dl>
    );
  }

  const outputTokens = metrics.outputTokens ?? cell.output ?? null;
  const ttftMs = detail.ttftMs;
  const generationMs = detail.decodingMs;
  const totalMs = detail.durationMs;
  const throughput =
    outputTokens != null && generationMs != null
      ? formatThroughput(outputTokens, generationMs / 1000)
      : '—';

  return (
    <dl className="space-y-1">
      <KeyValue label="Started" value={formatTimestamp(metrics.stepStartTime)} />
      <KeyValue label="Total duration" value={totalMs != null ? formatDurationMs(totalMs) : '—'} />
      <KeyValue label="TTFT" value={ttftMs != null ? formatDurationMs(ttftMs) : '—'} />
      <KeyValue label="Generation" value={generationMs != null ? formatDurationMs(generationMs) : '—'} />
      <KeyValue label="Throughput" value={throughput} />
    </dl>
  );
}

function UsageBlock({ usage, label }: { usage: TrajectoryUsage | undefined; label: string }) {
  if (usage === undefined) return null;
  const rows: Array<[string, number]> = [];
  const push = (name: string, value: number | undefined) => {
    if (value !== undefined) rows.push([name, value]);
  };
  push('Input', usage.input);
  push('Output', usage.output);
  push('Reasoning', usage.think);
  push('Cache read', usage.cacheRead);
  push('Cache write', usage.cacheWrite);

  return (
    <div className="space-y-1">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      {rows.map(([name, value]) => (
        <KeyValue key={name} label={name} value={formatTokenCount(value)} />
      ))}
    </div>
  );
}

function SummaryTab({
  cell,
  request,
  onTabChange,
}: {
  cell: TrajectoryCellProps;
  request: TrajectoryRequestDetail | null;
  onTabChange: (tab: TabId) => void;
}) {
  const isRequest = request !== null;
  const contentPreview = previewContent(cell);
  const previewOpens = hasPreview(cell);

  return (
    <div className="space-y-3">
      {/* Aggregate stats */}
      <dl className="space-y-1">
        {isRequest && request !== null ? (
          <>
            <KeyValue label="Status" value={stateLabel(request.state)} error={request.state === 'error'} />
            <KeyValue label="Turn" value={`Turn ${request.turn}`} />
            <KeyValue label="Step" value={request.group} />
            <KeyValue label="Tool calls" value={request.toolCalls} />
            {request.subtoolCalls > 0 && <KeyValue label="Subtool calls" value={request.subtoolCalls} />}
            {request.usage !== undefined && (
              <>
                <KeyValue label="Input tokens" value={formatTokenCount(request.usage.input)} />
                <KeyValue label="Output tokens" value={formatTokenCount(request.usage.output)} />
                {request.usage.cacheWrite !== undefined && (
                  <KeyValue label="Cache write" value={formatTokenCount(request.usage.cacheWrite)} />
                )}
              </>
            )}
          </>
        ) : (
          <>
            <KeyValue label="Status" value={cell.isError ? 'Error' : 'Complete'} error={cell.isError === true} />
            <KeyValue label="Type" value={KIND_LABEL[cell.kind] ?? cell.kind} />
            <KeyValue label="Index" value={String(cell.index)} />
            <TokenRows cell={cell} />
          </>
        )}
      </dl>

      {/* Preview of the message/result content */}
      {contentPreview !== '' && (
        <OverviewSection
          label="Preview"
          onOpen={previewOpens ? () => onTabChange('rendered') : undefined}
        >
          <pre className="text-[10px] font-mono text-foreground/70 whitespace-pre-wrap break-all leading-relaxed max-h-40 overflow-auto">
            {contentPreview}
          </pre>
        </OverviewSection>
      )}

      {/* Payload preview for tool records */}
      {!isRequest && cell.inputDetail !== undefined && (
        <OverviewSection label="Payload" onOpen={() => onTabChange('payload')}>
          <pre className="text-[10px] font-mono text-foreground/70 whitespace-pre-wrap break-all leading-relaxed max-h-32 overflow-auto">
            {cell.inputDetail}
          </pre>
        </OverviewSection>
      )}

      {/* Timing */}
      {!isRequest && (cell.assistantMetrics !== undefined || cell.timeSeconds != null) && (
        <OverviewSection label="Timing" onOpen={() => onTabChange('timing')}>
          <TimingPanel cell={cell} />
        </OverviewSection>
      )}
      {isRequest && request !== null && request.assistant !== undefined && (
        <OverviewSection label="Timing" onOpen={() => onTabChange('timing')}>
          <TimingPanel cell={request.assistant} />
        </OverviewSection>
      )}
    </div>
  );
}

export function TrajectoryDetail({ cell, request, onClose, detailWidth, onWidthChange }: TrajectoryDetailProps) {
  const [activeTab, setActiveTab] = useState<TabId>('overview');
  const tabs = detailTabs(cell, request ?? null);
  const active = tabs.some((t) => t.id === activeTab) ? activeTab : 'overview';

  const onTabChange = (tab: TabId) => setActiveTab(tab);

  const handleMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const move = (ev: MouseEvent) => {
      onWidthChange(Math.max(200, Math.min(600, window.innerWidth - ev.clientX)));
    };
    const up = () => {
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', move);
      document.removeEventListener('mouseup', up);
    };
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
  };

  const kindLabel = KIND_LABEL[cell.kind] ?? cell.kind;
  const location =
    request !== null
      ? `Turn ${request.turn} · ${request.group}`
      : cell.turn != null
        ? `Turn ${cell.turn}`
        : 'Between turns';

  return (
    <aside
      className="border-l border-border/40 bg-background flex flex-col overflow-hidden shrink-0 relative"
      style={{ width: detailWidth }}
    >
      {/* Drag handle on the left edge */}
      <div
        onMouseDown={handleMouseDown}
        className="absolute left-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 transition-colors z-10"
      />

      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border/40 pl-4">
        <div className="flex items-center gap-2 min-w-0">
          {request !== null ? (
            <>
              <span className="size-2 rounded-full bg-blue-500 shrink-0" aria-hidden />
              <span className="text-xs truncate text-blue-600 dark:text-blue-400">
                Request #{request.number}
              </span>
            </>
          ) : (
            <>
              <span className="text-[10px] font-mono text-muted-foreground shrink-0">#{cell.index}</span>
              <span className="text-xs truncate">{kindLabel}</span>
            </>
          )}
        </div>
        <button onClick={onClose} className="shrink-0 p-0.5 rounded hover:bg-muted/80 transition-colors">
          <X className="size-3.5" />
        </button>
      </div>

      {/* Location line */}
      <div className="px-3 pt-1.5 pb-1 text-[10px] text-muted-foreground truncate">{location}</div>

      {/* Tabs */}
      <div className="flex border-b border-border/40 overflow-x-auto shrink-0">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={active === tab.id}
            onClick={() => onTabChange(tab.id)}
            className={cn(
              'px-3 py-1.5 text-[11px] transition-colors shrink-0',
              active === tab.id
                ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 shadow-sm ring-1 ring-blue-300 dark:ring-blue-700 border-b-2 border-blue-500 dark:border-blue-400'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted/40',
            )}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-3 text-xs space-y-2">
        {active === 'overview' && <SummaryTab cell={cell} request={request ?? null} onTabChange={onTabChange} />}
        {active === 'usage' && request !== null && (
          <div className="space-y-2">
            <UsageBlock usage={request.usage} label="Request usage" />
            {request.cumulative !== undefined && (
              <UsageBlock usage={request.cumulative} label="Cumulative (incl. this)" />
            )}
          </div>
        )}
        {active === 'timing' && (
          <TimingTab cell={request?.assistant ?? cell} />
        )}
        {active === 'rendered' && (
          <pre className="text-[10px] font-mono text-foreground/70 whitespace-pre-wrap break-all leading-relaxed">
            {previewContent(cell) || '(no content)'}
          </pre>
        )}
        {active === 'raw' && (
          <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
            {safeJsonFormat(cell.outputDetail ?? cell.inputDetail ?? cell.text)}
          </pre>
        )}
        {active === 'payload' && (
          <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
            {safeJsonFormat(cell.inputDetail ?? '(no payload)')}
          </pre>
        )}
        {active === 'result' && (
          <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
            {safeJsonFormat(cell.result ?? cell.resultPreviewMarkdown ?? cell.outputDetail ?? '(no result)')}
          </pre>
        )}
        {active === 'schema' && (
          <pre className="text-[10px] font-mono text-foreground/80 whitespace-pre-wrap break-all leading-relaxed">
            {safeJsonFormat(cell.schemaDetail ?? '(no schema)')}
          </pre>
        )}
      </div>
    </aside>
  );
}

function TimingTab({ cell }: { cell: TrajectoryCellProps }) {
  return <TimingPanel cell={cell} />;
}

function safeJsonFormat(text: string): string {
  try {
    const parsed = JSON.parse(text);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return text;
  }
}