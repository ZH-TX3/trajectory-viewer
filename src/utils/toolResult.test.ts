import { describe, expect, it } from 'vitest';
import { describeToolResult } from './toolResult';

describe('describeToolResult', () => {
  it('renders an Edit as a diff with surrounding context', () => {
    const view = describeToolResult({
      toolName: 'Edit',
      toolArgs: JSON.stringify({
        file_path: '/repo/a.rs',
        old_string: '1\n2\n3\n4\n5',
        new_string: '1\n2\nX\n4\n5',
      }),
      result: 'The file /repo/a.rs has been updated.',
    });

    expect(view).toEqual({
      kind: 'diff',
      filePath: '/repo/a.rs',
      replaceAll: false,
      truncated: false,
      rows: [
        { marker: ' ', text: '1' },
        { marker: ' ', text: '2' },
        { marker: '-', text: '3' },
        { marker: '+', text: 'X' },
        { marker: ' ', text: '4' },
        { marker: ' ', text: '5' },
      ],
    });
  });

  it('marks an Edit that replaces every occurrence', () => {
    const view = describeToolResult({
      toolName: 'Edit',
      toolArgs: JSON.stringify({ file_path: 'a', old_string: 'x', new_string: 'y', replace_all: true }),
    });
    expect(view).toMatchObject({ kind: 'diff', replaceAll: true });
  });

  it('renders a Write as additions only', () => {
    const view = describeToolResult({
      toolName: 'Write',
      toolArgs: JSON.stringify({ file_path: '/repo/new.rs', content: 'a\nb' }),
    });

    expect(view).toEqual({
      kind: 'write',
      filePath: '/repo/new.rs',
      truncated: false,
      rows: [
        { marker: '+', text: 'a' },
        { marker: '+', text: 'b' },
      ],
    });
  });

  it('renders a Bash call as a command plus its output', () => {
    const view = describeToolResult({
      toolName: 'Bash',
      toolArgs: JSON.stringify({ command: 'ls -la', description: 'List files' }),
      result: 'file1\nfile2',
    });

    expect(view).toEqual({
      kind: 'terminal',
      command: 'ls -la',
      description: 'List files',
      output: 'file1\nfile2',
    });
  });

  it('renders a Read as a numbered file', () => {
    const view = describeToolResult({
      toolName: 'Read',
      toolArgs: JSON.stringify({ file_path: '/repo/a.rs', offset: 330, limit: 60 }),
      result: '330\tfn main() {}',
    });

    expect(view).toEqual({
      kind: 'file',
      filePath: '/repo/a.rs',
      content: '330\tfn main() {}',
    });
  });

  it('caps a runaway diff instead of rendering the whole file', () => {
    const content = Array.from({ length: 500 }, (_, i) => `line ${i}`).join('\n');
    const view = describeToolResult({
      toolName: 'Write',
      toolArgs: JSON.stringify({ file_path: 'big', content }),
    });

    expect(view).toMatchObject({ kind: 'write', truncated: true });
    expect((view as { rows: unknown[] }).rows).toHaveLength(400);
  });

  it('leaves tools it has no opinion about to the generic renderer', () => {
    for (const toolName of ['Grep', 'Glob', 'Task', 'WebFetch', 'TodoWrite']) {
      expect(
        describeToolResult({ toolName, toolArgs: '{"pattern":"x"}', result: 'hit' }),
      ).toBeNull();
    }
  });

  it('falls back when the arguments are not the object it expects', () => {
    expect(describeToolResult({ toolName: 'Bash', toolArgs: 'not json', result: 'x' })).toBeNull();
    expect(describeToolResult({ toolName: 'Bash', toolArgs: '["a"]', result: 'x' })).toBeNull();
    expect(describeToolResult({ toolName: 'Bash', result: 'x' })).toBeNull();
    // An Edit missing one side of the change is not a diff.
    expect(
      describeToolResult({ toolName: 'Edit', toolArgs: '{"file_path":"a","old_string":"x"}' }),
    ).toBeNull();
    expect(describeToolResult({ toolName: 'Read', toolArgs: '{"file_path":"a"}', result: '' })).toBeNull();
  });
});
