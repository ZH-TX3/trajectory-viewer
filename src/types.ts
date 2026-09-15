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

// ── Backup & Restore Types ───────────────────────────────────────────────

export interface ProviderSessionInfo {
  providerId: string;
  path?: string | null;
  exists: boolean;
  fileCount: number;
  totalBytes: number;
}

export interface BackupProviderStat {
  providerId: string;
  fileCount: number;
  totalBytes: number;
}

export interface BackupResult {
  filePath: string;
  sizeBytes: number;
  providers: BackupProviderStat[];
}

export interface RestoreResult {
  providers: BackupProviderStat[];
  safetyBackupDir?: string | null;
  warnings: string[];
}

export interface BackupEntry {
  filename: string;
  sizeBytes: number;
  createdAt: number;
}

export interface BackupSettings {
  intervalHours: number;
  retainCount: number;
}

export interface TrashEntry {
  id: string;
  providerId: string;
  sessionId: string;
  title: string;
  deletedAt: number;
  sizeBytes: number;
}

export interface TrashManifest {
  providerId: string;
  sessionId: string;
  title: string;
  deletedAt: number;
  kind: string;
  items: Array<{ originalPath: string; storedName: string }>;
}

// ── Config Hub Types ─────────────────────────────────────────────────────

export type ResourceKind = 'skill' | 'agent' | 'prompt' | 'mcp' | 'config';

/** How one tool relates to a resource. */
export type AppState = 'linked' | 'copied' | 'drifted' | 'absent';

export interface ConfigResource {
  id: string;
  kind: ResourceKind;
  name: string;
  description?: string | null;
  ssotPath?: string | null;
  inSsot: boolean;
  /** Per-tool state, keyed by tool id. */
  apps: Record<string, AppState>;
}

export interface ToolInfo {
  id: string;
  displayName: string;
  installed: boolean;
  /** Resource kinds this tool can link (e.g. `["skill", "agent"]`). */
  linkableKinds: ResourceKind[];
  hasPrompt: boolean;
  hasMcp: boolean;
}

export type ConfigFormat = 'json' | 'toml' | 'yaml' | 'markdown';

export interface ConfigFileContent {
  label: string;
  path: string;
  format: ConfigFormat;
  exists: boolean;
  content?: string | null;
}

export interface ToolConfigs {
  toolId: string;
  displayName: string;
  installed: boolean;
  prompt?: ConfigFileContent | null;
  files: ConfigFileContent[];
}

/** A tool's config root as the settings UI sees it. */
export interface ToolRootInfo {
  toolId: string;
  displayName: string;
  defaultRoot?: string | null;
  overrideRoot?: string | null;
}

export interface HubSnapshot {
  tools: ToolInfo[];
  resources: ConfigResource[];
  configs: ToolConfigs[];
}

export interface ImportPreview {
  kind: ResourceKind;
  name: string;
  toolId: string;
  sourcePath: string;
  targetPath: string;
  conflict: boolean;
  action: string;
}

// ── Config Hub: MCP Types ────────────────────────────────────────────────

export type McpServerType = 'stdio' | 'http' | 'sse';

/** Canonical MCP server spec (the unified store format). */
export interface McpServerSpec {
  type?: McpServerType;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  url?: string;
  headers?: Record<string, string>;
  [key: string]: unknown;
}

export interface McpServer {
  id: string;
  name: string;
  description?: string | null;
  server: McpServerSpec;
  /** Per-tool enablement (`claude` / `codex` / `opencode`). */
  apps: Record<string, boolean>;
}

export interface McpEditorState {
  id: string;
  name: string;
  description: string;
  type: McpServerType;
  command: string;
  args: string;
  env: string;
  url: string;
  headers: string;
  apps: Record<string, boolean>;
}

/** Result of a live MCP handshake test. */
export interface McpTestResult {
  ok: boolean;
  latencyMs: number;
  serverName?: string | null;
  serverVersion?: string | null;
  message: string;
}

// ── Config Hub: Profile Types ────────────────────────────────────────────

/** A named snapshot of which resources are enabled for which tools. */
export interface Profile {
  name: string;
  /** Skills/agents: resource id → tool ids it's enabled for. */
  resources: Record<string, string[]>;
  /** MCP servers: server id → tool ids it's enabled for. */
  mcpServers: Record<string, string[]>;
  createdAt: number;
}

/** Outcome of applying a profile. */
export interface ApplyReport {
  enabled: number;
  disabled: number;
  /** Resources skipped because the tool holds an unmanaged copy. */
  skipped: string[];
  errors: string[];
}

/** A profile plus the current live state, for the editor. */
export interface ProfileDetail {
  name: string;
  resources: Record<string, string[]>;
  mcpServers: Record<string, string[]>;
  /** resource id → tool id → enabled (current live state). */
  liveResources: Record<string, Record<string, boolean>>;
  /** server id → tool id → enabled (current live state). */
  liveMcp: Record<string, Record<string, boolean>>;
}