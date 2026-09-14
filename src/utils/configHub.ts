// ── Config Hub: pure view logic ──────────────────────────────────────────
//
// Filtering and state labelling, kept out of the components so they can be
// tested without a DOM.

import type {
  AppState,
  ConfigResource,
  McpEditorState,
  McpServer,
  McpServerSpec,
  McpServerType,
  ResourceKind,
  ToolInfo,
} from '../types';

/** Human-readable labels for the four per-tool states. */
export const APP_STATE_LABELS: Record<AppState, string> = {
  linked: 'Linked',
  copied: 'Copied',
  drifted: 'Needs import',
  absent: 'Not present',
};

/**
 * Per-tool active styling, mirroring cc-switch's `APP_ICON_MAP` tint: a
 * coloured chip that highlights which tools a resource is enabled for.
 */
export const TOOL_ACTIVE_CLASSES: Record<string, string> = {
  claude:
    'bg-orange-500/10 ring-1 ring-orange-500/20 hover:bg-orange-500/20 text-orange-600 dark:text-orange-400',
  codex:
    'bg-green-500/10 ring-1 ring-green-500/20 hover:bg-green-500/20 text-green-600 dark:text-green-400',
  opencode:
    'bg-indigo-500/10 ring-1 ring-indigo-500/20 hover:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400',
};

export const KIND_LABELS: Record<ResourceKind, string> = {
  skill: 'Skill',
  agent: 'Agent',
  prompt: 'Prompt',
  mcp: 'MCP',
  config: 'Config',
};

// ── MCP editor helpers ───────────────────────────────────────────────────
//
// The editor offers two views of one server: structured fields (type/command/
// args/env or url/headers) and the raw JSON spec. The fields are the source of
// truth; the JSON is derived from them, and editing the JSON parses it back
// into the fields so the two stay in step.

/** Convert a stored MCP server into the editor form state. */
export function mcpToEditor(server: McpServer): McpEditorState {
  const spec = server.server;
  const type = spec.type ?? 'stdio';
  return {
    id: server.id,
    name: server.name,
    description: server.description ?? '',
    type,
    command: (spec.command ?? '').toString(),
    args: (spec.args ?? []).join(', '),
    env: entriesToString(spec.env),
    url: (spec.url ?? '').toString(),
    headers: entriesToString(spec.headers),
    apps: { ...server.apps },
  };
}

/** Build the canonical spec from the editor form fields. */
export function editorToMcp(editor: McpEditorState): McpServerSpec {
  const spec: McpServerSpec = { type: editor.type };
  if (editor.type === 'stdio') {
    spec.command = editor.command.trim();
    // Comma-separated so an argument containing a space (a path, say) stays
    // one argument instead of being split apart.
    const args = editor.args
      .split(',')
      .map((a) => a.trim())
      .filter(Boolean);
    if (args.length > 0) spec.args = args;
    const env = stringToEntries(editor.env);
    if (Object.keys(env).length > 0) spec.env = env;
  } else {
    spec.url = editor.url.trim();
    const headers = stringToEntries(editor.headers);
    if (Object.keys(headers).length > 0) spec.headers = headers;
  }
  return spec;
}

/** The JSON text shown in the editor's JSON view. */
export function editorToJson(editor: McpEditorState): string {
  return JSON.stringify(editorToMcp(editor), null, 2);
}

/**
 * Parse JSON text back into the editor fields, keeping id/name/description and
 * the per-tool flags from `base`. Returns the updated state, or an error when
 * the text isn't a JSON object.
 */
export function jsonToEditor(
  text: string,
  base: McpEditorState,
): { editor: McpEditorState } | { error: string } {
  const trimmed = text.trim();
  if (!trimmed) return { error: 'Server config is empty' };
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (err) {
    return { error: `Invalid JSON: ${String(err)}` };
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { error: 'Server config must be a JSON object' };
  }
  const spec = parsed as McpServerSpec;
  const type: McpServerType =
    spec.type === 'http' || spec.type === 'sse' ? spec.type : 'stdio';
  return {
    editor: {
      ...base,
      type,
      command: (spec.command ?? '').toString(),
      args: (spec.args ?? []).join(', '),
      env: entriesToString(spec.env),
      url: (spec.url ?? '').toString(),
      headers: entriesToString(spec.headers),
    },
  };
}

/** Fresh (empty) editor state for the "add server" form. */
export function emptyMcpEditor(apps: Record<string, boolean>): McpEditorState {
  return {
    id: '',
    name: '',
    description: '',
    type: 'stdio',
    command: 'npx',
    args: '',
    env: '',
    url: '',
    headers: '',
    apps: { ...apps },
  };
}

/** `{"K":"V"}` ↔ `"K=V\nV2=X"` for the env/headers textareas. */
export function entriesToString(entries?: Record<string, string>): string {
  return Object.entries(entries ?? {})
    .map(([k, v]) => `${k}=${v}`)
    .join('\n');
}

export function stringToEntries(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    if (eq < 0) {
      out[trimmed] = '';
    } else {
      out[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
    }
  }
  return out;
}

export interface ResourceFilter {
  kind: ResourceKind | 'all';
  /** Only show resources present in this tool. `'all'` shows everything. */
  toolId: string | 'all';
  /** Only show resources with at least one tool needing an import. */
  driftedOnly: boolean;
}

/** Apply the toolbar filters to the scanned resource list. */
export function filterResources(
  resources: ConfigResource[],
  filter: ResourceFilter,
): ConfigResource[] {
  return resources.filter((resource) => {
    if (filter.kind !== 'all' && resource.kind !== filter.kind) return false;
    if (filter.driftedOnly && !hasDrift(resource)) return false;
    if (filter.toolId !== 'all') {
      const state = resource.apps[filter.toolId];
      if (!state || state === 'absent') return false;
    }
    return true;
  });
}

/** Whether any tool holds an unmanaged copy of this resource. */
export function hasDrift(resource: ConfigResource): boolean {
  return Object.values(resource.apps).some((state) => state === 'drifted');
}

/** Tools that can actually link this resource kind, installed only. */
export function linkableTools(tools: ToolInfo[], kind: ResourceKind): ToolInfo[] {
  return tools.filter((tool) => tool.installed && tool.linkableKinds.includes(kind));
}

/** The resource kinds present in a scan, for the kind filter. */
export function presentKinds(resources: ConfigResource[]): ResourceKind[] {
  const seen = new Set<ResourceKind>();
  for (const resource of resources) seen.add(resource.kind);
  return [...seen];
}
