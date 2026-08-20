// ── Trajectory Layout Algorithm ──────────────────────────────────────────
//
// Converts a flat list of TrajectoryEvent[] into turn-grouped models
// suitable for rendering in TrajectoryTable and TrajectoryTimeline.

import type { TrajectoryEvent, ContentBlock } from '../types';
import { trajectoryPreviewText } from './format';

// ── Cell types ────────────────────────────────────────────────────────────

export type TrajectoryCellKind =
  | 'system'
  | 'user'
  | 'context'
  | 'compacted'
  | 'message'
  | 'tool'
  | 'subtool';

export interface AssistantMetricDetail {
  timingRecorded: boolean;
  stepStartTime: number | null;
  firstTokenTime: number | null;
  completedTime: number | null;
  usageProvided: boolean;
  outputTokens: number | null;
}

export interface TrajectorySourceBlock {
  type: string;
  content: string;
  imageSrc?: string;
  imageAlt?: string;
  callId?: string;
  toolName?: string;
}

export interface TrajectoryCellProps {
  index: number;
  recordId?: string;
  kind: TrajectoryCellKind;
  text: string;
  previewMarkdown?: string;
  opensTurn?: boolean;
  sourceSeq?: number;
  requestOnly?: boolean;
  turn?: number | null;
  step?: number | null;

  /** Full content for detail panel */
  inputDetail?: string;
  outputDetail?: string;
  thinkingDetail?: string;
  sourceBlocks?: readonly TrajectorySourceBlock[];
  outputBlocks?: readonly TrajectorySourceBlock[];

  /** Tool-specific */
  callId?: string;
  toolName?: string;
  toolArgs?: string;
  schemaDetail?: string;
  isError?: boolean;
  result?: string;
  resultPreviewMarkdown?: string;

  /** Timing */
  timeSeconds: number | null;
  startedAt?: number | null;

  /** Token usage */
  input?: number;
  output?: number;
  think?: number;
  cacheRead?: number;
  cacheWrite?: number;

  /** Assistant-only metrics */
  assistantMetrics?: AssistantMetricDetail;

  /** Selection */
  selected?: boolean;
}

export interface TrajectoryGroupModel {
  title: string;
  description?: string;
  cells: readonly TrajectoryCellProps[];
}

export interface TrajectoryTurnModel {
  turn: number | null;
  groups: readonly TrajectoryGroupModel[];
}

/**
 * Fill missing turn/step values for providers that do not emit them.
 * A real user message starts a new turn; tool activity becomes step 1.
 */
function withDerivedTurnStep(events: readonly TrajectoryEvent[]): TrajectoryEvent[] {
  let turn = 0;
  let step = 0;

  return events.map((event) => {
    if (event.eventType === 'user-message') {
      turn += 1;
      step = 0;
    } else if (event.eventType === 'tool-call' && turn > 0 && step === 0) {
      step = 1;
    }

    return {
      ...event,
      turn: event.turn ?? turn,
      step: event.step ?? step,
    };
  });
}

// ── Layout derivation ─────────────────────────────────────────────────────

/**
 * Derive turn-grouped layout from a flat event list.
 */
export function deriveTrajectoryLayout(
  events: readonly TrajectoryEvent[],
): readonly TrajectoryTurnModel[] {
  const turnMap = new Map<number, TrajectoryCellProps[]>();
  const standaloneCells: TrajectoryCellProps[] = [];
  let cellIndex = 0;

  for (const event of withDerivedTurnStep(events)) {
    const cell = eventToCell(event, cellIndex);
    if (!cell) continue;
    cell.turn = event.turn;
    cell.step = event.step;
    cellIndex++;

    if (event.turn != null && event.turn > 0) {
      const existing = turnMap.get(event.turn) ?? [];
      existing.push(cell);
      turnMap.set(event.turn, existing);
    } else {
      standaloneCells.push(cell);
    }
  }

  const turns: TrajectoryTurnModel[] = [];

  // Standalone (turn 0) cells go first
  if (standaloneCells.length > 0) {
    turns.push({
      turn: null,
      groups: [{ title: 'Prologue', cells: standaloneCells }],
    });
  }

  // Sort turns by turn number
  const sortedTurns = [...turnMap.entries()].sort(([a], [b]) => a - b);

  for (const [turnNum, cells] of sortedTurns) {
    const groups = groupCellsIntoSteps(cells);
    turns.push({ turn: turnNum, groups });
  }

  return turns;
}

// ── Helpers ───────────────────────────────────────────────────────────────

function eventToCell(
  event: TrajectoryEvent,
  index: number,
): TrajectoryCellProps | null {
  const timeSeconds = event.durationMs != null ? event.durationMs / 1000 : null;

  switch (event.eventType) {
    case 'user-message':
      return {
        index,
        sourceSeq: event.seq,
        timeSeconds,
        startedAt: event.ts,
        callId: event.toolCallId,
        toolName: event.toolName,
        toolArgs: event.toolArgs,
        isError: event.isError ?? undefined,
        inputDetail: event.content,
        outputDetail: event.toolResult,
        input: event.inputTokens ?? undefined,
        output: event.outputTokens ?? undefined,
        cacheRead: event.cacheReadTokens ?? undefined,
        cacheWrite: event.cacheWriteTokens ?? undefined,
        kind: 'user',
        text: truncateContent(event.content, 120),
        previewMarkdown: event.content,
        opensTurn: true,
        sourceBlocks: contentBlocksToSourceBlocks(event.contentBlocks ?? undefined),
      } as TrajectoryCellProps;

    case 'assistant-message':
      return {
        index,
        sourceSeq: event.seq,
        timeSeconds,
        startedAt: event.ts,
        callId: event.toolCallId,
        toolName: event.toolName,
        toolArgs: event.toolArgs,
        isError: event.isError ?? undefined,
        inputDetail: event.content,
        outputDetail: event.toolResult,
        input: event.inputTokens ?? undefined,
        output: event.outputTokens ?? undefined,
        cacheRead: event.cacheReadTokens ?? undefined,
        cacheWrite: event.cacheWriteTokens ?? undefined,
        kind: 'message',
        text: truncateContent(event.content, 120),
        previewMarkdown: event.content,
        sourceBlocks: contentBlocksToSourceBlocks(event.contentBlocks ?? undefined),
        assistantMetrics: event.ttftMs != null ? {
          timingRecorded: true,
          stepStartTime: event.ts,
          firstTokenTime: event.ts + event.ttftMs,
          completedTime: event.durationMs != null ? event.ts + event.durationMs : null,
          usageProvided: event.inputTokens != null || event.outputTokens != null,
          outputTokens: event.outputTokens ?? null,
        } : undefined,
      } as TrajectoryCellProps;

    case 'tool-call':
      return {
        index,
        sourceSeq: event.seq,
        timeSeconds,
        startedAt: event.ts,
        callId: event.toolCallId,
        toolName: event.toolName,
        toolArgs: event.toolArgs,
        isError: event.isError ?? undefined,
        outputDetail: event.toolResult,
        input: event.inputTokens ?? undefined,
        output: event.outputTokens ?? undefined,
        cacheRead: event.cacheReadTokens ?? undefined,
        cacheWrite: event.cacheWriteTokens ?? undefined,
        kind: 'tool',
        text: event.toolName ?? 'Tool call',
        inputDetail: event.toolArgs,
        previewMarkdown: event.toolName
          ? `**${event.toolName}**\n\`\`\`json\n${event.toolArgs ?? ''}\n\`\`\``
          : undefined,
      } as TrajectoryCellProps;

    case 'tool-result':
      return {
        index,
        sourceSeq: event.seq,
        timeSeconds,
        startedAt: event.ts,
        callId: event.toolCallId,
        toolName: event.toolName,
        toolArgs: event.toolArgs,
        isError: event.isError ?? undefined,
        inputDetail: event.content,
        outputDetail: event.toolResult,
        input: event.inputTokens ?? undefined,
        output: event.outputTokens ?? undefined,
        cacheRead: event.cacheReadTokens ?? undefined,
        cacheWrite: event.cacheWriteTokens ?? undefined,
        kind: 'tool',
        text: truncateContent(event.toolResult, 120),
        result: event.toolResult,
        resultPreviewMarkdown: event.toolResult,
      } as TrajectoryCellProps;

    case 'turn-boundary':
      return null;

    case 'compaction':
      return {
        index,
        sourceSeq: event.seq,
        timeSeconds,
        startedAt: event.ts,
        callId: event.toolCallId,
        toolName: event.toolName,
        toolArgs: event.toolArgs,
        isError: event.isError ?? undefined,
        inputDetail: event.content,
        input: event.inputTokens ?? undefined,
        output: event.outputTokens ?? undefined,
        cacheRead: event.cacheReadTokens ?? undefined,
        cacheWrite: event.cacheWriteTokens ?? undefined,
        kind: 'compacted',
        text: 'Context compaction',
        outputDetail: event.content,
      } as TrajectoryCellProps;

    default:
      return null;
  }
}

function groupCellsIntoSteps(
  cells: TrajectoryCellProps[],
): TrajectoryGroupModel[] {
  const groups: TrajectoryGroupModel[] = [];
  let currentStep: number | null = null;
  let stepCells: TrajectoryCellProps[] = [];

  const flush = () => {
    if (stepCells.length === 0) return;
    groups.push({
      title: currentStep === 0 ? 'Message' : `Step ${currentStep ?? 0}`,
      cells: stepCells,
    });
    stepCells = [];
  };

  for (const cell of cells) {
    const step = cell.step ?? (cell.opensTurn ? 0 : null);

    if (step !== null && currentStep !== null && step !== currentStep) {
      flush();
    }
    if (step !== null) {
      currentStep = step;
    }
    stepCells.push(cell);
  }

  flush();

  return groups;
}

function contentBlocksToSourceBlocks(
  blocks?: ContentBlock[],
): TrajectorySourceBlock[] | undefined {
  if (!blocks || blocks.length === 0) return undefined;
  return blocks.map((b) => ({
    type: b.blockType,
    content: b.text ?? '',
    callId: b.toolCallId ?? undefined,
    toolName: b.toolName ?? undefined,
    imageSrc: b.imageSrc ?? undefined,
  }));
}

function truncateContent(content: string | null | undefined, maxLen: number): string {
  if (!content) return '(empty)';
  const trimmed = content.replace(/\s+/g, ' ').trim();
  if (trimmed.length <= maxLen) return trimmed;
  return trimmed.slice(0, maxLen) + '…';
}

// ── Virtual row helpers ───────────────────────────────────────────────────

export interface VirtualRow {
  entries: Array<{ logicalIndex: number; cell: TrajectoryCellProps }>;
  height: number;
  key: string;
}

const CONTENT_ROW_HEIGHT = 30;

/**
 * Group records into measurable virtual rows.
 *
 * The row key MUST be unique per row: tool-call and tool-result cells share
 * the same callId (and kind), so trajectoryRecordId alone would collide and
 * cause React to overlap/reuse rows (ghosting with progressively bolder text).
 */
export function groupVirtualRows(records: TrajectoryCellProps[]): VirtualRow[] {
  const rows: VirtualRow[] = [];

  for (const cell of records) {
    rows.push({
      entries: [{ logicalIndex: cell.index, cell }],
      height: CONTENT_ROW_HEIGHT,
      key: `${trajectoryRecordId(cell)}\0#\0${cell.index}`,
    });
  }

  return rows;
}

// ── Record ID helper ──────────────────────────────────────────────────────

export function trajectoryRecordId(cell: {
  recordId?: string;
  callId?: string;
  sourceSeq?: number;
  kind: string;
  index: number;
}): string {
  if (cell.recordId !== undefined) return cell.recordId;
  if (cell.callId !== undefined) return `${cell.kind}\0call\0${cell.callId}`;
  if (cell.sourceSeq !== undefined) return `${cell.kind}\0seq\0${cell.sourceSeq}`;
  return `${cell.kind}\0index\0${cell.index}`;
}

// ── Timeline model (ported from DSH deriveTrajectoryTimeline) ─────────────
//
// The timeline is a stable three-lane projection of every visible record.
// Each span carries its position in a shared coordinate space:
//   - mode "sequence" → cell order (0..N)
//   - mode "actual"   → wall-clock time (milliseconds)
// The drag range emitted by the timeline lives in this SAME coordinate space,
// so trajectoryTimelineFocusIndexes() can map it back to record indexes
// without any drift between what is drawn and what is dimmed.

export type TrajectoryTimelineMode = 'sequence' | 'actual';

export interface TrajectorySpanModel {
  start: number;
  end: number;
  index: number;
  isError: boolean;
  kind: TrajectoryCellKind;
  lane: number;
  label: string;
}

export interface TrajectoryTimelineModel {
  start: number;
  end: number;
  spans: TrajectorySpanModel[];
  turnBoundaries: { turn: number; time: number }[];
}

function laneFor(kind: TrajectoryCellKind): number {
  if (kind === 'tool' || kind === 'subtool') return 2;
  if (kind === 'message' || kind === 'compacted') return 1;
  return 0;
}

function cellRange(cell: TrajectoryCellProps):
  { start: number; end: number } | null {
  const startedAt = cell.startedAt;
  if (startedAt === null || startedAt === undefined || !Number.isFinite(startedAt)) return null;
  const durationMs =
    cell.timeSeconds !== null && cell.timeSeconds !== undefined && Number.isFinite(cell.timeSeconds)
      ? Math.max(0, cell.timeSeconds * 1000)
      : 0;
  return { start: startedAt, end: startedAt + durationMs };
}

/** Build the timeline model for modes: sequence layout or actual recorded time. */
export function deriveTrajectoryTimeline(
  turns: readonly TrajectoryTurnModel[],
  mode: TrajectoryTimelineMode = 'sequence',
): TrajectoryTimelineModel | null {
  if (mode === 'actual') {
    const spans: TrajectorySpanModel[] = [];
    const turnBoundaries: { turn: number; time: number }[] = [];
    let hasSpans = false;

    for (const turn of turns) {
      const turnSpans = turn.groups.flatMap((group) =>
        group.cells.flatMap((cell) => {
          if (cell.requestOnly === true) return [];
          const range = cellRange(cell);
          return range === null
            ? []
            : [{
                start: range.start,
                end: range.end,
                index: cell.index,
                isError: cell.isError === true,
                kind: cell.kind,
                lane: laneFor(cell.kind),
                label: cell.text,
              }];
        }),
      );
      if (turnSpans.length === 0) continue;
      hasSpans = true;
      if (turn.turn !== null) {
        turnBoundaries.push({ turn: turn.turn, time: Math.min(...turnSpans.map((s) => s.start)) });
      }
      spans.push(...turnSpans);
    }

    if (!hasSpans) return null;
    return {
      start: Math.min(...spans.map((s) => s.start)),
      end: Math.max(...spans.map((s) => s.end)),
      spans,
      turnBoundaries,
    };
  }

  // "sequence" mode — layout every visible record at integer positions.
  const spans: TrajectorySpanModel[] = [];
  const turnBoundaries: { turn: number; time: number }[] = [];

  for (const turn of turns) {
    const cells = turn.groups.flatMap((group) => group.cells.filter((cell) => cell.requestOnly !== true));
    if (cells.length === 0) continue;
    if (turn.turn !== null) turnBoundaries.push({ turn: turn.turn, time: spans.length });
    spans.push(
      ...cells.map((cell, offset) => ({
        start: spans.length + offset,
        end: spans.length + offset + 1,
        index: cell.index,
        isError: cell.isError === true,
        kind: cell.kind,
        lane: laneFor(cell.kind),
        label: cell.text,
      })),
    );
  }

  if (spans.length === 0) return null;
  return { start: 0, end: spans.length, spans, turnBoundaries };
}

/**
 * Identify records active at any point inside an inclusive selected interval.
 * @returns Record indexes inside the focus interval (exact — spans overlapping the range).
 */
export function trajectoryTimelineFocusIndexes(
  turns: readonly TrajectoryTurnModel[],
  range: { start: number; end: number },
  mode: TrajectoryTimelineMode = 'sequence',
): Set<number> {
  const model = deriveTrajectoryTimeline(turns, mode);
  if (!model) return new Set();
  return new Set(
    model.spans
      .filter((span) => span.start <= range.end && span.end >= range.start)
      .map((span) => span.index),
  );
}

// ── Assistant timing detail ───────────────────────────────────────────────
//
// Ported from DSH assistantTimingDetail/timelineRecordDetail: derive precise
// TTFT (step start → first token) and decoding (first token → completed) from
// the recorded step timings, when all three points are present and ordered.

export interface TimelineRecordDetail {
  durationMs?: number;
  startedAt?: number;
  ttftMs?: number;
  decodingMs?: number;
}

export function assistantTimingDetail(
  metrics: AssistantMetricDetail | null | undefined,
): Pick<TimelineRecordDetail, 'ttftMs' | 'decodingMs'> {
  const start = metrics?.stepStartTime;
  const first = metrics?.firstTokenTime;
  const completed = metrics?.completedTime;
  if (
    metrics?.timingRecorded !== true ||
    typeof start !== 'number' ||
    typeof first !== 'number' ||
    typeof completed !== 'number' ||
    !Number.isFinite(start) ||
    !Number.isFinite(first) ||
    !Number.isFinite(completed) ||
    first < start ||
    completed < first
  ) {
    return {};
  }
  return { ttftMs: first - start, decodingMs: completed - first };
}

export function timelineRecordDetail(cell: TrajectoryCellProps): TimelineRecordDetail {
  const durationMs =
    cell.timeSeconds === null || !Number.isFinite(cell.timeSeconds)
      ? undefined
      : Math.max(0, cell.timeSeconds * 1000);
  const startedAt =
    cell.startedAt === null || !Number.isFinite(cell.startedAt)
      ? undefined
      : cell.startedAt;
  return {
    ...(durationMs === undefined ? {} : { durationMs }),
    ...(startedAt === undefined ? {} : { startedAt }),
    ...assistantTimingDetail(cell.assistantMetrics),
  };
}

// ── Trajectory search index (ported from DSH TrajectorySearchIndex) ───────
//
// Incremental per-record index keyed by stable record id. Re-parses Markdown
// only when a record's source fields actually change; search matches every
// whitespace-separated term against the joined lowercased text.

function sameSources(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function markdownPreview(cell: TrajectoryCellProps): string {
  if (cell.previewMarkdown === undefined) return '';
  const preview = trajectoryPreviewText(cell.previewMarkdown);
  if (cell.text === '') return preview;
  return preview === '' ? cell.text : `${cell.text} · ${preview}`;
}

function resultPreview(cell: TrajectoryCellProps): string {
  return cell.resultPreviewMarkdown === undefined
    ? cell.result ?? ''
    : trajectoryPreviewText(cell.resultPreviewMarkdown);
}

function recordSources(
  turn: number | null,
  groupTitle: string,
  cell: TrajectoryCellProps,
): string[] {
  const blocks = [...(cell.sourceBlocks ?? []), ...(cell.outputBlocks ?? [])];
  return [
    turn === null ? 'between turns' : `turn ${turn}`,
    groupTitle,
    cell.kind,
    cell.kind === 'message' ? 'assistant' : '',
    cell.text,
    cell.previewMarkdown ?? '',
    cell.inputDetail ?? '',
    cell.outputDetail ?? '',
    cell.thinkingDetail ?? '',
    cell.schemaDetail ?? '',
    cell.result ?? '',
    cell.resultPreviewMarkdown ?? '',
    cell.callId ?? '',
    ...blocks.flatMap((block) => [
      block.type,
      block.content,
      block.callId ?? '',
      block.toolName ?? '',
      block.imageAlt ?? '',
    ]),
  ];
}

export interface TrajectorySearchEntry {
  sources: readonly string[];
  text: string;
}

export class TrajectorySearchIndex {
  private entries = new Map<string, TrajectorySearchEntry>();
  private layouts?: readonly TrajectoryTurnModel[][];

  /** Incrementally synchronize the index with the given layout slices. */
  update(layouts: readonly TrajectoryTurnModel[][]): boolean {
    if (this.layouts === layouts) return false;
    this.layouts = layouts;
    const seen = new Set<string>();

    for (const turns of layouts) {
      for (const turn of turns) {
        for (const group of turn.groups) {
          for (const cell of group.cells) {
            if (cell.requestOnly === true) continue;
            const id = trajectoryRecordId(cell);
            const sources = recordSources(turn.turn, group.title, cell);
            const previous = this.entries.get(id);
            const entry =
              previous !== undefined && sameSources(previous.sources, sources)
                ? previous
                : {
                    sources,
                    text: [...sources, markdownPreview(cell), resultPreview(cell)]
                      .join('\n')
                      .toLocaleLowerCase(),
                  };
            this.entries.set(id, entry);
            seen.add(id);
          }
        }
      }
    }

    for (const id of this.entries.keys()) {
      if (!seen.has(id)) this.entries.delete(id);
    }
    return true;
  }

  /** Match space-separated case-insensitive terms; `null` without a query. */
  search(query: string): Set<string> | null {
    const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) return null;
    const matches = new Set<string>();
    for (const [id, entry] of this.entries) {
      if (terms.every((term) => entry.text.includes(term))) matches.add(id);
    }
    return matches;
  }
}

// ── Request aggregation ───────────────────────────────────────────────────
//
// A "request" groups every record that belongs to one assistant step
// (turn + group, e.g. "Message" / "Step 2"). Selecting any record inside an
// assistant/tool step shows the aggregate request detail, mirroring DSH.

export interface TrajectoryUsage {
  input?: number;
  output?: number;
  think?: number;
  cacheRead?: number;
  cacheWrite?: number;
}

export interface TrajectoryRequestDetail {
  turn: number | null;
  group: string;
  number: number;
  state: 'complete' | 'error';
  toolCalls: number;
  subtoolCalls: number;
  assistant?: TrajectoryCellProps;
  usage?: TrajectoryUsage;
  cumulative?: TrajectoryUsage;
  records: readonly TrajectoryCellProps[];
}

function mergeCellUsage(list: readonly TrajectoryCellProps[]): TrajectoryUsage | undefined {
  const out: TrajectoryUsage = {};
  let any = false;
  for (const cell of list) {
    for (const [key, value] of [
      ['input', cell.input],
      ['output', cell.output],
      ['think', cell.think],
      ['cacheRead', cell.cacheRead],
      ['cacheWrite', cell.cacheWrite],
    ] as const) {
      if (value == null) continue;
      out[key] = (out[key] ?? 0) + value;
      any = true;
    }
  }
  return any ? out : undefined;
}

function addUsage(left: TrajectoryUsage, right: TrajectoryUsage): TrajectoryUsage {
  const out: TrajectoryUsage = { ...left };
  for (const key of ['input', 'output', 'think', 'cacheRead', 'cacheWrite'] as const) {
    const value = right[key];
    if (value != null) out[key] = (out[key] ?? 0) + value;
  }
  return out;
}

/**
 * Aggregate the request a record belongs to. Records outside any request
 * (prologue system/compacted/etc.) return `null` — they open record-level.
 */
export function aggregateRequestDetail(
  turns: readonly TrajectoryTurnModel[],
  cellIndex: number,
): TrajectoryRequestDetail | null {
  let requestNumber = 0;
  let cumulative: TrajectoryUsage | undefined;

  for (const turn of turns) {
    for (const group of turn.groups) {
      // A request group is any non-prologue group (turn > 0).
      const isRequest = turn.turn != null;
      if (isRequest) requestNumber += 1;

      const cells = group.cells;
      const hit = cells.some((cell) => cell.index === cellIndex);
      if (!hit) {
        if (isRequest) {
          cumulative = addUsage(cumulative ?? {}, mergeCellUsage(cells) ?? {});
        }
        continue;
      }
      if (!isRequest) return null; // prologue → record-level selection

      const usage = mergeCellUsage(cells);
      // DSH's cumulative usage INCLUDES the current request.
      cumulative = addUsage(cumulative ?? {}, usage ?? {});
      const hasUsage =
        cumulative.input != null || cumulative.output != null || cumulative.think != null;

      return {
        turn: turn.turn,
        group: group.title,
        number: requestNumber,
        state: cells.some((cell) => cell.isError === true) ? 'error' : 'complete',
        toolCalls: cells.filter((cell) => cell.kind === 'tool').length,
        subtoolCalls: cells.filter((cell) => cell.kind === 'subtool').length,
        assistant: cells.find((cell) => cell.kind === 'message'),
        usage,
        cumulative: hasUsage ? cumulative : undefined,
        records: cells,
      };
    }
  }
  return null;
}