import { describe, expect, it } from 'vitest';
import type { ConfigResource, McpEditorState, ToolInfo } from '../types';
import {
  editorToJson,
  editorToMcp,
  entriesToString,
  filterResources,
  hasDrift,
  jsonToEditor,
  linkableTools,
  mcpToEditor,
  presentKinds,
  stringToEntries,
} from './configHub';

function resource(
  id: string,
  kind: ConfigResource['kind'],
  apps: ConfigResource['apps'],
): ConfigResource {
  return { id, kind, name: id.split(':')[1], inSsot: true, apps };
}

const skills: ConfigResource[] = [
  resource('skill:alpha', 'skill', { claude: 'linked', codex: 'absent' }),
  resource('skill:beta', 'skill', { claude: 'drifted', codex: 'linked' }),
  resource('agent:gamma', 'agent', { claude: 'drifted' }),
];

const tools: ToolInfo[] = [
  {
    id: 'claude',
    displayName: 'Claude Code',
    installed: true,
    linkableKinds: ['skill', 'agent'],
    hasPrompt: true,
    hasMcp: true,
  },
  {
    id: 'codex',
    displayName: 'Codex',
    installed: true,
    linkableKinds: ['skill'],
    hasPrompt: true,
    hasMcp: true,
  },
  {
    id: 'dsh',
    displayName: 'DSH',
    installed: true,
    linkableKinds: [],
    hasPrompt: true,
    hasMcp: false,
  },
  {
    id: 'opencode',
    displayName: 'OpenCode',
    installed: false,
    linkableKinds: [],
    hasPrompt: false,
    hasMcp: false,
  },
];

describe('hasDrift', () => {
  it('is true only when some tool holds an unmanaged copy', () => {
    expect(hasDrift(skills[1])).toBe(true);
    expect(hasDrift(skills[0])).toBe(false);
  });
});

describe('filterResources', () => {
  it('filters by kind', () => {
    const out = filterResources(skills, { kind: 'agent', toolId: 'all', driftedOnly: false });
    expect(out.map((r) => r.id)).toEqual(['agent:gamma']);
  });

  it('filters by tool, excluding absent', () => {
    const out = filterResources(skills, { kind: 'all', toolId: 'codex', driftedOnly: false });
    expect(out.map((r) => r.id)).toEqual(['skill:beta']);
  });

  it('filters to drifted resources only', () => {
    const out = filterResources(skills, { kind: 'all', toolId: 'all', driftedOnly: true });
    expect(out.map((r) => r.id)).toEqual(['skill:beta', 'agent:gamma']);
  });

  it('combines kind and tool filters', () => {
    const out = filterResources(skills, { kind: 'skill', toolId: 'claude', driftedOnly: false });
    expect(out.map((r) => r.id)).toEqual(['skill:alpha', 'skill:beta']);
  });

  it('returns nothing when no resource matches', () => {
    const out = filterResources(skills, { kind: 'mcp', toolId: 'all', driftedOnly: false });
    expect(out).toEqual([]);
  });
});

describe('linkableTools', () => {
  it('returns installed tools that support the kind', () => {
    expect(linkableTools(tools, 'skill').map((t) => t.id)).toEqual(['claude', 'codex']);
    expect(linkableTools(tools, 'agent').map((t) => t.id)).toEqual(['claude']);
  });

  it('excludes uninstalled tools even if they declare the kind', () => {
    expect(linkableTools(tools, 'skill').some((t) => t.id === 'opencode')).toBe(false);
  });

  it('is empty for a kind no tool supports', () => {
    expect(linkableTools(tools, 'mcp')).toEqual([]);
  });
});

describe('presentKinds', () => {
  it('lists each kind once, in first-seen order', () => {
    expect(presentKinds(skills)).toEqual(['skill', 'agent']);
  });

  it('is empty for no resources', () => {
    expect(presentKinds([])).toEqual([]);
  });
});

describe('MCP editor conversions', () => {
  it('editorToMcp builds a stdio spec from the form fields', () => {
    const spec = editorToMcp({
      id: 'time',
      name: 'time',
      description: '',
      type: 'stdio',
      command: 'npx',
      args: '-y, @modelcontextprotocol/server-time',
      env: 'API_KEY=sk-123',
      url: '',
      headers: '',
      apps: { claude: true, codex: false, opencode: false },
    });
    expect(spec.type).toBe('stdio');
    expect(spec.command).toBe('npx');
    expect(spec.args).toEqual(['-y', '@modelcontextprotocol/server-time']);
    expect(spec.env).toEqual({ API_KEY: 'sk-123' });
  });

  it('editorToMcp builds an http spec from url + headers', () => {
    const spec = editorToMcp({
      id: 'web',
      name: 'web',
      description: '',
      type: 'http',
      command: '',
      args: '',
      env: '',
      url: 'https://example.com/mcp',
      headers: 'Authorization=Bearer x\nX-Trace=abc',
      apps: { claude: false, codex: true, opencode: false },
    });
    expect(spec.type).toBe('http');
    expect(spec.url).toBe('https://example.com/mcp');
    expect(spec.headers).toEqual({ Authorization: 'Bearer x', 'X-Trace': 'abc' });
  });

  it('mcpToEditor round-trips a stored server', () => {
    const editor = mcpToEditor({
      id: 'time',
      name: 'time',
      description: 'Temporal server',
      server: {
        type: 'stdio',
        command: 'npx',
        args: ['-y', 'pkg'],
        env: { A: '1' },
      },
      apps: { claude: true },
    });
    expect(editor.description).toBe('Temporal server');
    expect(editor.args).toBe('-y, pkg');
    expect(editor.env).toBe('A=1');

    // Back to a spec via the fields.
    const spec = editorToMcp(editor);
    expect(spec.args).toEqual(['-y', 'pkg']);
    expect(spec.env).toEqual({ A: '1' });
  });

  it('comma-separated args keep a space inside one argument', () => {
    const spec = editorToMcp({
      id: 'x',
      name: 'x',
      description: '',
      type: 'stdio',
      command: 'my-tool',
      args: '--dir, C:/My Files/data, --verbose',
      env: '',
      url: '',
      headers: '',
      apps: {},
    });
    expect(spec.args).toEqual(['--dir', 'C:/My Files/data', '--verbose']);
  });

  it('stringToEntries handles blank lines and missing equals', () => {
    const out = stringToEntries('A=1\n\nplain\nB = 2 ');
    expect(out).toEqual({ A: '1', plain: '', B: '2' });
  });

  it('entriesToString is the inverse of stringToEntries', () => {
    const entries = { A: '1', B: 'two words' };
    expect(stringToEntries(entriesToString(entries))).toEqual(entries);
  });
});

describe('MCP JSON view', () => {
  const base: McpEditorState = {
    id: 'time',
    name: 'time',
    description: '',
    type: 'stdio',
    command: 'npx',
    args: '-y, pkg',
    env: 'A=1',
    url: '',
    headers: '',
    apps: { claude: true, codex: false, opencode: false },
  };

  it('editorToJson reflects the fields', () => {
    const parsed = JSON.parse(editorToJson(base));
    expect(parsed).toEqual({
      type: 'stdio',
      command: 'npx',
      args: ['-y', 'pkg'],
      env: { A: '1' },
    });
  });

  it('jsonToEditor parses a stdio spec back into fields', () => {
    const result = jsonToEditor(
      JSON.stringify({ type: 'stdio', command: 'uvx', args: ['a', 'b'], env: { K: 'V' } }),
      base,
    );
    expect('editor' in result).toBe(true);
    if ('editor' in result) {
      expect(result.editor.command).toBe('uvx');
      expect(result.editor.args).toBe('a, b');
      expect(result.editor.env).toBe('K=V');
      // Identity fields are preserved from the base.
      expect(result.editor.id).toBe('time');
      expect(result.editor.apps.claude).toBe(true);
    }
  });

  it('jsonToEditor parses an http spec and switches the transport', () => {
    const result = jsonToEditor(
      JSON.stringify({ type: 'http', url: 'https://x/mcp', headers: { A: 'b' } }),
      base,
    );
    expect('editor' in result).toBe(true);
    if ('editor' in result) {
      expect(result.editor.type).toBe('http');
      expect(result.editor.url).toBe('https://x/mcp');
      expect(result.editor.headers).toBe('A=b');
    }
  });

  it('jsonToEditor reports invalid JSON', () => {
    const result = jsonToEditor('{ not json', base);
    expect('error' in result).toBe(true);
  });

  it('jsonToEditor rejects a non-object', () => {
    expect('error' in jsonToEditor('[1,2,3]', base)).toBe(true);
    expect('error' in jsonToEditor('"text"', base)).toBe(true);
    expect('error' in jsonToEditor('', base)).toBe(true);
  });

  it('fields and JSON round-trip through each other', () => {
    const json = editorToJson(base);
    const result = jsonToEditor(json, base);
    expect('editor' in result).toBe(true);
    if ('editor' in result) {
      expect(editorToMcp(result.editor)).toEqual(editorToMcp(base));
    }
  });
});
