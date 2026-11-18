// ── Message Outline ──────────────────────────────────────────────────────
//
// The Messages tab carries a rail of the turns the reader typed, so a long
// transcript can be navigated instead of scrolled. This is the list maths
// behind it, kept out of the component so the numbering can be tested: an entry
// that pointed at the wrong row would scroll somewhere unrelated.

import type { SessionMessage } from '../types';
import { isInjectedContext } from './format';

export interface OutlineItem {
  /** Index into the message list, so the caller knows what to scroll to. */
  index: number;
  /** One-line excerpt of the turn. */
  preview: string;
  /**
   * When the turn was said, in epoch ms. The Trajectory tab locates a record by
   * timestamp rather than by position, so this is how the same entry jumps
   * there.
   */
  ts: number | null;
}

/** Characters kept per entry; CSS clamps the rest away. */
export const OUTLINE_PREVIEW_CHARS = 80;

/**
 * One entry per user turn, in transcript order.
 *
 * Injected context is skipped for the same reason it is drawn as `system`:
 * a workspace reminder is not something the reader wrote, so it is not a
 * landmark to navigate by. The index stays the ORIGINAL row index, since that
 * is what the caller scrolls to.
 */
export function deriveOutline(messages: SessionMessage[]): OutlineItem[] {
  const items: OutlineItem[] = [];
  messages.forEach((msg, index) => {
    if (msg.role !== 'user' || isInjectedContext(msg.content)) return;
    items.push({
      index,
      preview: msg.content.replace(/\s+/g, ' ').trim().slice(0, OUTLINE_PREVIEW_CHARS),
      ts: msg.ts ?? null,
    });
  });
  return items;
}
