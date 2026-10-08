import { describe, expect, it } from 'vitest';
import { remapWheelToHorizontal } from './scroll';

describe('remapWheelToHorizontal', () => {
  it('remaps a vertical wheel onto the horizontal axis', () => {
    const result = remapWheelToHorizontal({ deltaX: 0, deltaY: 40, deltaMode: 0 }, 0, 300);
    expect(result.scrollBy).toBe(40);
    expect(result.pending).toBe(0);
  });

  // Regression: the filter bar called `preventDefault()` unconditionally and
  // then scrolled by `deltaY`. On a trackpad a horizontal gesture arrives as
  // `deltaX` with `deltaY` near zero, so the gesture was swallowed and the bar
  // barely moved.
  it('leaves a horizontal trackpad gesture to the browser', () => {
    const result = remapWheelToHorizontal({ deltaX: -30, deltaY: 1, deltaMode: 0 }, 0, 300);
    expect(result.scrollBy).toBeNull();
  });

  it('does not let a horizontal gesture corrupt the pending remainder', () => {
    const result = remapWheelToHorizontal({ deltaX: -30, deltaY: 0, deltaMode: 0 }, 0.6, 300);
    expect(result.scrollBy).toBeNull();
    expect(result.pending).toBe(0.6);
  });

  // Regression: each fractional delta was added straight to `scrollLeft`, which
  // rounds to whole pixels — so a trackpad's small deltas rounded to zero and
  // the bar stuttered.
  it('carries the sub-pixel remainder across events', () => {
    let pending = 0;
    const steps: number[] = [];
    for (let i = 0; i < 5; i += 1) {
      const result = remapWheelToHorizontal({ deltaX: 0, deltaY: 0.6, deltaMode: 0 }, pending, 300);
      pending = result.pending;
      steps.push(result.scrollBy ?? 0);
    }
    // Five 0.6px steps must total 3px, not round away to zero.
    expect(steps.reduce((a, b) => a + b, 0)).toBe(3);
    expect(pending).toBeCloseTo(0, 5);
  });

  it('accumulates the same distance in either direction', () => {
    const run = (deltaY: number) => {
      let pending = 0;
      let total = 0;
      for (let i = 0; i < 5; i += 1) {
        const result = remapWheelToHorizontal({ deltaX: 0, deltaY, deltaMode: 0 }, pending, 300);
        pending = result.pending;
        total += result.scrollBy ?? 0;
      }
      return total;
    };
    expect(run(0.6)).toBe(3);
    expect(run(-0.6)).toBe(-3);
  });

  it('does not drift on whole-pixel deltas', () => {
    let pending = 0;
    for (let i = 0; i < 20; i += 1) {
      const result = remapWheelToHorizontal({ deltaX: 0, deltaY: 40, deltaMode: 0 }, pending, 300);
      expect(result.scrollBy).toBe(40);
      pending = result.pending;
    }
    expect(pending).toBe(0);
  });

  it('scales line-mode deltas to pixels', () => {
    const result = remapWheelToHorizontal({ deltaX: 0, deltaY: 3, deltaMode: 1 }, 0, 300);
    expect(result.scrollBy).toBe(48);
  });

  it('scales page-mode deltas by the viewport width', () => {
    const result = remapWheelToHorizontal({ deltaX: 0, deltaY: 1, deltaMode: 2 }, 0, 300);
    expect(result.scrollBy).toBe(300);
  });

  it('scrolls backwards for an upward wheel', () => {
    const result = remapWheelToHorizontal({ deltaX: 0, deltaY: -25, deltaMode: 0 }, 0, 300);
    expect(result.scrollBy).toBe(-25);
  });
});
