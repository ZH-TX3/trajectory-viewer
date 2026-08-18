// ── Trajectory Type Definitions ──────────────────────────────────────────

export interface TrajectoryData {
  sessionId: string;
  providerId: string;
  events: TrajectoryEvent[];
  metadata: TrajectoryMetadata;
}

export interface TrajectoryMetadata {
  model: string | null;
  totalInputTokens: number | null;
  totalOutputTokens: number | null;
  totalDurationMs: number | null;
  eventCount: number;
}

export type TrajectoryEventType =
  | 'user-message'
  | 'assistant-message'
  | 'tool-call'
  | 'tool-result'
  | 'turn-boundary'
  | 'compaction';

export interface TrajectoryEvent {
  seq: number;
  ts: number;
  eventType: TrajectoryEventType;
  role: string | null;
  content: string | null;
  contentBlocks: ContentBlock[] | null;
  toolCallId: string | null;
  toolName: string | null;
  toolArgs: string | null;
  toolResult: string | null;
  isError: boolean | null;
  turn: number | null;
  step: number | null;
  durationMs: number | null;
  ttftMs: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  reasoningTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  model: string | null;
  provider: string | null;
}

export interface ContentBlock {
  blockType: string;
  text: string | null;
  toolCallId: string | null;
  toolName: string | null;
  toolArgs: string | null;
  imageSrc: string | null;
}

// ── Session Types ────────────────────────────────────────────────────────

export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string | null;
  summary?: string | null;
  projectDir?: string | null;
  projectGroup?: string | null;
  createdAt?: number | null;
  lastActiveAt?: number | null;
  sourcePath?: string | null;
  resumeCommand?: string | null;
}

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number | null;
}