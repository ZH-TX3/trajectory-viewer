import { describe, expect, it } from 'vitest';
import {
  formatCodeBlock,
  parseInline,
  parseMarkdownBlocks,
  tokenizeJson,
} from './markdown';

describe('parseMarkdownBlocks', () => {
  it('splits a bold label from its fenced json block', () => {
    const blocks = parseMarkdownBlocks('**Bash**\n```json\n{"command":"ls"}\n```');
    expect(blocks).toEqual([
      { type: 'text', content: '**Bash**' },
      { type: 'code', lang: 'json', content: '{"command":"ls"}' },
    ]);
  });

  it('keeps multi-line code bodies intact', () => {
    const blocks = parseMarkdownBlocks('```\nline1\nline2\n```');
    expect(blocks).toEqual([{ type: 'code', lang: '', content: 'line1\nline2' }]);
  });

  it('handles text on both sides of a code block', () => {
    const blocks = parseMarkdownBlocks('before\n```js\nx\n```\nafter');
    expect(blocks.map((b) => b.type)).toEqual(['text', 'code', 'text']);
    expect(blocks[2]).toEqual({ type: 'text', content: 'after' });
  });

  it('treats an unterminated fence as literal text', () => {
    const blocks = parseMarkdownBlocks('**Bash**\n```json\n{"a":1}');
    expect(blocks).toEqual([{ type: 'text', content: '**Bash**\n```json\n{"a":1}' }]);
  });

  it('returns a single text block for plain content', () => {
    expect(parseMarkdownBlocks('just text')).toEqual([
      { type: 'text', content: 'just text' },
    ]);
  });

  it('ignores an empty string', () => {
    expect(parseMarkdownBlocks('')).toEqual([]);
  });
});

describe('formatCodeBlock', () => {
  it('pretty-prints one-line json', () => {
    const out = formatCodeBlock('json', '{"command":"ls","description":"list"}');
    expect(out).toBe('{\n  "command": "ls",\n  "description": "list"\n}');
  });

  it('leaves invalid json untouched', () => {
    expect(formatCodeBlock('json', '{not json')).toBe('{not json');
  });

  it('leaves non-json languages untouched', () => {
    expect(formatCodeBlock('bash', '{"a":1}')).toBe('{"a":1}');
  });
});

describe('parseInline', () => {
  it('marks bold spans', () => {
    expect(parseInline('**Bash** extra')).toEqual([
      { text: 'Bash', bold: true },
      { text: ' extra', bold: false },
    ]);
  });

  it('returns one plain segment when there is no bold', () => {
    expect(parseInline('plain')).toEqual([{ text: 'plain', bold: false }]);
  });

  it('handles text before and after a bold span', () => {
    expect(parseInline('a **b** c')).toEqual([
      { text: 'a ', bold: false },
      { text: 'b', bold: true },
      { text: ' c', bold: false },
    ]);
  });
});

describe('tokenizeJson', () => {
  it('marks keys and values separately', () => {
    const tokens = tokenizeJson('{\n  "command": "ls"\n}');
    expect(tokens.find((t) => t.text === '"command"')?.kind).toBe('key');
    expect(tokens.find((t) => t.text === '"ls"')?.kind).toBe('value');
  });

  it('marks non-string scalars as values', () => {
    const tokens = tokenizeJson('{"n": 3, "ok": true, "x": null}');
    expect(tokens.find((t) => t.text === '3')?.kind).toBe('value');
    expect(tokens.find((t) => t.text === 'true')?.kind).toBe('value');
    expect(tokens.find((t) => t.text === 'null')?.kind).toBe('value');
  });

  it('keeps punctuation and whitespace plain', () => {
    const tokens = tokenizeJson('{"a":1}');
    const plain = tokens.filter((t) => t.kind === 'plain').map((t) => t.text).join('');
    expect(plain).toContain('{');
    expect(plain).toContain(':');
    expect(plain).toContain('}');
  });

  it('reassembles to the original text', () => {
    const source = '{\n  "command": "Get-ChildItem Env:",\n  "n": 1\n}';
    expect(tokenizeJson(source).map((t) => t.text).join('')).toBe(source);
  });

  it('returns one plain token for invalid json', () => {
    expect(tokenizeJson('{not json')).toEqual([{ text: '{not json', kind: 'plain' }]);
  });
});
