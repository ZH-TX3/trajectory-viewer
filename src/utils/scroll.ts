// ── Wheel → horizontal scroll remapping ──────────────────────────────────
//
// A horizontally scrollable strip (the provider filter bar) should answer a
// plain vertical wheel the way a scrollable tab strip does, while leaving a
// trackpad's own horizontal gesture alone.
//
// The subtlety is that a trackpad reports fractional pixel deltas and emits
// momentum events, whereas `scrollLeft` only accepts whole pixels. Adding each
// delta on its own therefore rounds the small ones to zero and the strip
// stutters. The remainder is carried across events instead.

/** The subset of `WheelEvent` this logic reads. */
export interface WheelInput {
  deltaX: number;
  deltaY: number;
  /** 0 = pixels, 1 = lines, 2 = pages (DOM_DELTA_*). */
  deltaMode: number;
}

export interface WheelRemap {
  /**
   * Pixels to scroll, or `null` to let the browser handle the event natively.
   *
   * Native handling is the right answer for a horizontal gesture: it keeps
   * macOS momentum and rubber-banding, which synthesising the scroll destroys.
   */
  scrollBy: number | null;
  /** Sub-pixel remainder to carry into the next event. */
  pending: number;
}

/**
 * Decide what a wheel event should do to a horizontally scrollable strip.
 *
 * @param event         The wheel event's deltas and mode.
 * @param pending       Sub-pixel remainder left over from the previous event.
 * @param viewportWidth Used to convert page-mode deltas; ignored otherwise.
 */
export function remapWheelToHorizontal(
  event: WheelInput,
  pending: number,
  viewportWidth: number,
): WheelRemap {
  const { deltaX, deltaY, deltaMode } = event;

  // A horizontal gesture is already what the strip wants. Remapping it would
  // consume the event and then scroll by a near-zero `deltaY`, so it is handed
  // back untouched.
  if (Math.abs(deltaX) > Math.abs(deltaY)) {
    return { scrollBy: null, pending };
  }

  const scale = deltaMode === 1 ? 16 : deltaMode === 2 ? viewportWidth : 1;
  const total = pending + deltaY * scale;
  let whole = Math.trunc(total);
  let rest = total - whole;
  // Float error can leave the remainder a hair under a whole pixel (0.6 added
  // five times lands on 0.9999999999999999), and `trunc` would then hold that
  // pixel back indefinitely. Snap it across.
  const step = Math.sign(total);
  if (step !== 0 && Math.abs(rest - step) < 1e-9) {
    whole += step;
    rest = 0;
  }
  return { scrollBy: whole, pending: rest };
}
