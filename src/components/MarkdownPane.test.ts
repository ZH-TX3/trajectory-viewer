import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { MarkdownPane, markdownHeight } from './MarkdownPane';
import { useMarkdownEditor } from '../utils/useMarkdownEditor';

const CONFIG_HUB = {
  source: 'max-h-24',
  preview: 'max-h-96',
  editor: 'h-[70vh] min-h-64',
};

describe('markdownHeight — the policy, one view at a time', () => {
  it('gives each view its own height', () => {
    expect(markdownHeight('source', CONFIG_HUB)).toBe('max-h-24');
    expect(markdownHeight('preview', CONFIG_HUB)).toBe('max-h-96');
    expect(markdownHeight('editor', CONFIG_HUB)).toBe('h-[70vh] min-h-64');
  });

  it('defaults the unset views to the source height (the memory viewer)', () => {
    const memory = { source: 'flex-1' };
    expect(markdownHeight('source', memory)).toBe('flex-1');
    expect(markdownHeight('preview', memory)).toBe('flex-1');
    expect(markdownHeight('editor', memory)).toBe('flex-1');
  });

  it('is empty when no heights are given at all', () => {
    expect(markdownHeight('source', undefined)).toBe('');
  });
});

// The rendered half: `previewing` is internal state, so the preview view cannot
// be reached from a server render. It is covered by the tests above instead.
const SOURCE = 'line\n'.repeat(60);

function render(heights: Record<string, string>, editing: boolean) {
  function Probe() {
    const editor = useMarkdownEditor({ original: SOURCE, save: async () => {} });
    if (editing && !editor.editing) editor.startEdit();
    return createElement(MarkdownPane, { source: SOURCE, editor, heights });
  }
  return renderToStaticMarkup(createElement(Probe));
}

/** The element that owns the scroll — heights have to land on this one. */
function scroller(html: string): { tag: string; classes: string[] } {
  for (const raw of html.match(/<[a-z]+[^>]*>/g) ?? []) {
    const classes = (/class="([^"]*)"/.exec(raw)?.[1] ?? '').split(/\s+/).filter(Boolean);
    if (classes.includes('overflow-auto')) {
      return { tag: /^<([a-z]+)/.exec(raw)![1], classes };
    }
  }
  throw new Error('nothing scrolls');
}

describe('Config Hub as it is actually called', () => {
  it('source is the compact one, on the scroller', () => {
    const s = scroller(render(CONFIG_HUB, false));
    expect(s.tag).toBe('pre');
    expect(s.classes).toContain('max-h-24');
    expect(s.classes).toContain('overflow-auto');
    // The roomy heights must not apply until asked for.
    expect(s.classes).not.toContain('max-h-96');
    expect(s.classes).not.toContain('h-[70vh]');
  });

  it('editing is 70vh, matching this panel\'s other formats', () => {
    const s = scroller(render(CONFIG_HUB, true));
    expect(s.tag).toBe('textarea');
    expect(s.classes).toContain('h-[70vh]');
    expect(s.classes).toContain('min-h-64');
    // The compact reading cap must not survive into the editor.
    expect(s.classes).not.toContain('max-h-24');
  });

  it('no height ever lands on the root', () => {
    for (const editing of [false, true]) {
      const root = (/^<div class="([^"]*)"/.exec(render(CONFIG_HUB, editing))![1]).split(/\s+/);
      expect(root.filter((c) => c.startsWith('max-h-') || c.startsWith('h-['))).toEqual([]);
    }
  });
});

describe('memory viewer fills in every view', () => {
  const MEMORY = { source: 'flex-1' };

  it('source fills, uncapped', () => {
    const s = scroller(render(MEMORY, false));
    expect(s.classes).toContain('flex-1');
    expect(s.classes.some((c) => c.startsWith('max-h-'))).toBe(false);
  });

  it('editing fills too', () => {
    const s = scroller(render(MEMORY, true));
    expect(s.tag).toBe('textarea');
    expect(s.classes).toContain('flex-1');
  });
});
