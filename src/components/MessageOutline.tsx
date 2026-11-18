// ── Turn rail ───────────────────────────────────────────────────────────
//
// A minimap of the turns the reader typed, pinned to the right edge of the
// Messages pane. Ported from DSH's turn navigator (`dsh-client-ui-chat`), whose
// geometry this copies: a fixed 10px pitch, one 20×2px tick per turn, ticks
// growing from 60% to full width as they go from idle to hovered to current.
//
// Why ticks instead of a text list: at 40 turns a list is a second scrollbar
// competing with the transcript, and its rows are too narrow to read anyway.
// A tick rail stays one glance wide, and hovering any tick previews the turn it
// stands for.

import { X } from 'lucide-react';
import { useState } from 'react';

import { cn } from '../lib/utils';
import type { OutlineItem } from '../utils/outline';

/** Fixed pitch between neighbouring ticks, so the rail's length encodes count. */
const TURN_SPACING_PX = 10;
/** Rail padding above the first tick. */
const RAIL_INSET_PX = 1;
/** Height of one tick's hit area. */
const TICK_HIT_PX = 10;

interface MessageOutlineProps {
  items: OutlineItem[];
  /** The turn currently in view, or the one just jumped to. */
  activeIndex: number | null;
  onSelect: (index: number) => void;
}

export function MessageOutline({ items, activeIndex, onSelect }: MessageOutlineProps) {
  const [preview, setPreview] = useState<number | null>(null);

  // One tick is not a structure worth its own gutter.
  if (items.length < 2) return null;

  const activePosition = items.findIndex((item) => item.index === activeIndex);
  const previewPosition = items.findIndex((item) => item.index === preview);
  const marksHeight = items.length * TURN_SPACING_PX + RAIL_INSET_PX + 1;

  return (
    <div className="absolute right-2 top-1/2 z-10 -translate-y-1/2">
      <nav
        aria-label="Turn navigation"
        className="relative w-7"
        // Long transcripts scroll inside the rail instead of running off the
        // pane, which is what keeps the axis aligned with the viewport centre.
        style={{ maxHeight: 'min(420px, calc(100dvh - 360px))' }}
      >
        <div
          className="overflow-y-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          style={{ maxHeight: 'inherit' }}
        >
          <div className="relative" style={{ height: marksHeight }}>
            {items.map((item, position) => {
              const active = activePosition === position;
              const hovered = previewPosition === position;
              return (
                <button
                  key={item.index}
                  type="button"
                  data-index={position}
                  aria-label={`Jump to turn ${position + 1}`}
                  aria-current={active ? 'true' : undefined}
                  onPointerMove={() => setPreview(item.index)}
                  onPointerLeave={() => setPreview(null)}
                  onFocus={() => setPreview(item.index)}
                  onBlur={() => setPreview(null)}
                  onClick={() => onSelect(item.index)}
                  className="group absolute left-0 right-0 rounded-lg focus-visible:outline-none"
                  style={{
                    height: TICK_HIT_PX,
                    transform: `translate3d(0, ${position * TURN_SPACING_PX + RAIL_INSET_PX}px, 0)`,
                  }}
                >
                  <span
                    className={cn(
                      // Only core-palette colours: this project's `*-foreground`
                      // / `*-muted` design tokens are not wired into Tailwind, so
                      // those utilities render as nothing.
                      'absolute right-0 top-1/2 h-0.5 w-5 origin-right -translate-y-1/2 rounded-full transition-transform duration-150',
                      active
                        ? 'bg-emerald-500 scale-x-100'
                        : hovered
                          ? 'bg-zinc-600 scale-x-90 dark:bg-zinc-300'
                          : 'bg-zinc-400/80 scale-x-[.6]',
                      'group-focus-visible:bg-emerald-500 group-focus-visible:scale-x-100',
                    )}
                  />
                </button>
              );
            })}
          </div>
        </div>

        {previewPosition >= 0 && (
          <div
            className="pointer-events-none absolute right-[calc(100%+10px)] w-[min(300px,40vw)] rounded-lg border border-zinc-200 bg-white px-3 py-2 shadow-lg dark:border-zinc-700 dark:bg-zinc-900"
            style={{
              top: previewPosition * TURN_SPACING_PX + RAIL_INSET_PX + TICK_HIT_PX / 2,
              transform: 'translateY(-50%)',
            }}
          >
            <div className="line-clamp-2 text-[11px] font-semibold leading-snug text-zinc-900 dark:text-zinc-100">
              {previewPosition + 1}. {items[previewPosition].preview}
            </div>
            <div className="mt-1 text-[10px] text-zinc-500">Click to jump</div>
          </div>
        )}
      </nav>
    </div>
  );
}

/**
 * The same turns as a readable list, opened from the header.
 *
 * The rail answers "where am I in this conversation"; this answers "which turn
 * was the one about PATH". Anchored under the header button that opens it, so
 * the caller supplies the positioned wrapper and the outside click.
 */
export function MessageTurnList({
  items,
  onSelect,
  onClose,
}: {
  items: OutlineItem[];
  onSelect: (index: number) => void;
  onClose: () => void;
}) {
  return (
    <div className="absolute right-0 top-full mt-1 z-50 w-72 max-h-80 overflow-auto rounded-lg border border-zinc-200 bg-white shadow-lg dark:border-zinc-700 dark:bg-zinc-900">
      <div className="sticky top-0 flex items-center gap-1 border-b border-zinc-200 bg-white px-2.5 py-1.5 text-[10px] font-medium text-zinc-500 dark:border-zinc-700 dark:bg-zinc-900">
        User messages
        <span className="ml-auto tabular-nums text-zinc-400">{items.length}</span>
        <button
          type="button"
          onClick={onClose}
          title="Close"
          aria-label="Close"
          className="rounded p-0.5 hover:bg-zinc-100 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
        >
          <X className="size-3" />
        </button>
      </div>
      <div className="p-1">
        {items.map((item, position) => (
          <button
            key={item.index}
            type="button"
            onClick={() => onSelect(item.index)}
            className="flex w-full items-start gap-1.5 rounded px-1.5 py-1 text-left text-[11px] text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            <span className="mt-px flex size-4 shrink-0 items-center justify-center rounded-full bg-zinc-100 text-[9px] font-semibold tabular-nums text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
              {position + 1}
            </span>
            <span className="line-clamp-2 leading-snug break-words">{item.preview}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
