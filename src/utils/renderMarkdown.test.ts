import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './renderMarkdown';

describe('renderMarkdown', () => {
  it('renders headings, lists and tables as GFM html', () => {
    const html = renderMarkdown('# Title\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |');
    expect(html).toContain('<h1');
    expect(html).toContain('Title');
    expect(html).toContain('<li>one</li>');
    expect(html).toContain('<table>');
    expect(html).toContain('<td>2</td>');
  });

  it('treats a lone newline as a line break', () => {
    // Memory documents are prose, not CommonMark — a single newline inside a
    // paragraph must break the line, not collapse into a space.
    expect(renderMarkdown('a\nb')).toBe('<p>a<br>b</p>\n');
  });

  it('escapes code bodies and tags them with the language', () => {
    const html = renderMarkdown('```json\n{"a":"<b>&"}\n```');
    expect(html).toContain('class="md-pre"');
    expect(html).toContain('data-lang="json"');
    // The payload is escaped, so no raw `<b>` survives.
    expect(html).not.toContain('<b>');
    expect(html).toContain('&lt;b&gt;&amp;');
  });

  it('strips extra info off a fence language tag', () => {
    // `marked` hands the whole info string to the renderer; only the first
    // word is a language.
    expect(renderMarkdown('```js extra info\nx\n```')).toContain('data-lang="js"');
  });

  it('renders an unterminated fence as a code block', () => {
    // A truncated document (a session killed mid-write) must not swallow the
    // rest of the file into a literal fence.
    const html = renderMarkdown('**Bash**\n```json\n{"a":1}');
    expect(html).toContain('<strong>Bash</strong>');
    expect(html).toContain('data-lang="json"');
    expect(html).not.toContain('```');
  });

  it('moves link hrefs into a data attribute and marks unsafe schemes', () => {
    const html = renderMarkdown('[docs](https://a.dev/?p=1&q=2) and [bad](javascript:x)');
    // No href attribute at all — a click must go through handleMarkdownClick
    // instead of letting the browser navigate the single-page webview.
    expect(html).not.toMatch(/\shref=/);
    expect(html).toContain('data-href="https://a.dev/?p=1&amp;q=2"');
    expect(html).toContain('data-unsafe="1"');
  });

  it('returns an empty string for an empty source', () => {
    expect(renderMarkdown('')).toBe('');
  });
});
