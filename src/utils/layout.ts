// ── Trajectory Layout Algorithm ──────────────────────────────────────────
//
// Converts a flat list of TrajectoryEvent[] into turn-grouped models
// suitable for rendering in TrajectoryTable and TrajectoryTimeline.

import type { TrajectoryEvent, ContentBlock } from '../types';

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
 */
export function groupVirtualRows(records: TrajectoryCellProps[]): VirtualRow[] {
  const rows: VirtualRow[] = [];

  for (const cell of records) {
    rows.push({
      entries: [{ logicalIndex: cell.index, cell }],
      height: CONTENT_ROW_HEIGHT,
      key: trajectoryRecordId(cell),
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