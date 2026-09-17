// ── Trajectory Layout Tests ──────────────────────────────────────────────
//
// Covers the pure layout/derivation logic that TrajectoryView/Timeline/Table
// build on. This is where the null-content white-screen originated, so the
// null/empty cases are exercised explicitly.

import { describe, expect, it } from 'vitest';
import {
  aggregateRequestDetail,
  assistantTimingDetail,
  deriveTrajectoryLayout,
  deriveTrajectoryTimeline,
  groupVirtualRows,
  summarizeTurnText,
  timelineRecordDetail,
  trajectoryRecordId,
  trajectoryTimelineFocusIndexes,
  TrajectorySearchIndex,
} from './layout';
import type { TrajectoryEvent } from '../types';
import type { TrajectoryTurnModel } from './layout';

/** `update()` takes mutable inner arrays; deriveTrajectoryLayout returns
 * readonly ones (the app casts the same way). */
function layoutsOf(events: TrajectoryEvent[]): TrajectoryTurnModel[][] {
  return [deriveTrajectoryLayout(events)] as TrajectoryTurnModel[][];
}

/** Build a TrajectoryEvent with sensible defaults. */
function event(overrides: Partial<TrajectoryEvent> = {}): TrajectoryEvent {
  return {
    seq: 1,
    ts: 1000,
    eventType: 'user-message',
    role: 'user',
    content: 'hello',
    contentBlocks: null,
    toolCallId: null,
    toolName: null,
    toolArgs: null,
    toolResult: null,
    isError: null,
    turn: 1,
    step: 0,
    durationMs: null,
    ttftMs: null,
    inputTokens: null,
    outputTokens: null,
    reasoningTokens: null,
    cacheReadTokens: null,
    cacheWriteTokens: null,
    model: null,
    provider: 'claude',
    ...overrides,
  };
}

describe('deriveTrajectoryLayout', () => {
  it('groups events into turns and steps', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', turn: 1, step: 0 }),
      event({ seq: 2, eventType: 'assistant-message', turn: 1, step: 1 }),
      event({ seq: 3, eventType: 'user-message', turn: 2, step: 0 }),
      event({ seq: 4, eventType: 'assistant-message', turn: 2, step: 1 }),
    ]);

    expect(turns).toHaveLength(2);
    expect(turns[0].turn).toBe(1);
    expect(turns[1].turn).toBe(2);
    expect(turns[0].groups.length).toBeGreaterThan(0);
  });

  it('marks the first cell of a turn as opening it', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', content: 'hi' }),
      event({ seq: 2, eventType: 'assistant-message', content: 'yo' }),
    ]);
    const cells = turns[0].groups.flatMap((g) => g.cells);
    expect(cells[0].opensTurn).toBe(true);
    expect(cells[1].opensTurn).toBeFalsy();
  });

  // Regression: opencode emits assistant messages with null content (pure
  // tool/reasoning rounds). Passing null into the preview path used to throw
  // `Cannot read properties of null (reading 'slice')` and white-screen the
  // whole view.
  it('handles null assistant content without throwing', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', content: 'run it' }),
      event({ seq: 2, eventType: 'assistant-message', content: null }),
      event({ seq: 3, eventType: 'tool-call', toolName: 'bash', toolArgs: '{}' }),
    ]);

    const cells = turns.flatMap((t) => t.groups.flatMap((g) => g.cells));
    expect(cells).toHaveLength(3);
    expect(cells[1].kind).toBe('message');
    expect(cells[1].text).toBe('(empty)');
  });

  it('classifies system-reminder user messages as context', () => {
    const turns = deriveTrajectoryLayout([
      event({
        seq: 1,
        eventType: 'user-message',
        content: '<system-reminder>be nice</system-reminder>',
      }),
    ]);
    const cell = turns[0].groups[0].cells[0];
    expect(cell.kind).toBe('context');
    expect(cell.opensTurn).toBeFalsy();
  });

  it('derives turn/step when the provider omits them', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', turn: null, step: null }),
      event({ seq: 2, eventType: 'tool-call', turn: null, step: null, toolName: 'bash' }),
    ]);
    expect(turns[0].turn).toBe(1);
    const cells = turns[0].groups.flatMap((g) => g.cells);
    expect(cells[0].step).toBe(0);
    expect(cells[1].step).toBe(1);
  });

  it('returns no turns for an empty event list', () => {
    expect(deriveTrajectoryLayout([])).toEqual([]);
  });
});

describe('deriveTrajectoryTimeline', () => {
  const events = [
    event({ seq: 1, eventType: 'user-message', turn: 1, step: 0, ts: 1000 }),
    event({ seq: 2, eventType: 'assistant-message', turn: 1, step: 1, ts: 1100 }),
    event({ seq: 3, eventType: 'tool-call', turn: 1, step: 1, ts: 1200, toolName: 'bash' }),
    event({ seq: 4, eventType: 'user-message', turn: 2, step: 0, ts: 2000 }),
    event({ seq: 5, eventType: 'assistant-message', turn: 2, step: 1, ts: 2100 }),
  ];

  it('lays every event out as a one-unit span in sequence mode', () => {
    const model = deriveTrajectoryTimeline(deriveTrajectoryLayout(events), 'sequence');
    expect(model).not.toBeNull();
    expect(model!.spans).toHaveLength(5);
    for (const span of model!.spans) {
      expect(span.end - span.start).toBe(1);
    }
  });

  // Regression: dense ledgers used to render as one solid band. Each turn now
  // gets a one-unit gap, so model.end exceeds the span count.
  it('inserts a gap after each turn in sequence mode', () => {
    const turns = deriveTrajectoryLayout(events);
    const model = deriveTrajectoryTimeline(turns, 'sequence')!;
    const turnCount = turns.filter((t) => t.turn !== null).length;
    expect(model.end).toBe(model.spans.length + turnCount);
  });

  it('keeps span indexes contiguous and unique across turn gaps', () => {
    const model = deriveTrajectoryTimeline(deriveTrajectoryLayout(events), 'sequence')!;
    const indexes = model.spans.map((s) => s.index);
    expect(new Set(indexes).size).toBe(indexes.length);
    for (let i = 1; i < indexes.length; i++) {
      expect(indexes[i]).toBeGreaterThan(indexes[i - 1]);
    }
  });

  it('records turn boundaries at each turn start', () => {
    const turns = deriveTrajectoryLayout(events);
    const model = deriveTrajectoryTimeline(turns, 'sequence')!;
    expect(model.turnBoundaries.map((b) => b.turn)).toEqual([1, 2]);
    expect(model.turnBoundaries[0].time).toBe(0);
  });

  it('assigns lanes by kind (input / model / tools)', () => {
    const model = deriveTrajectoryTimeline(deriveTrajectoryLayout(events), 'sequence')!;
    const laneOf = (kind: string) => model.spans.find((s) => s.kind === kind)?.lane;
    expect(laneOf('user')).toBe(0);
    expect(laneOf('message')).toBe(1);
    expect(laneOf('tool')).toBe(2);
  });

  it('uses wall-clock time in actual mode', () => {
    const model = deriveTrajectoryTimeline(deriveTrajectoryLayout(events), 'actual')!;
    expect(model.start).toBe(1000);
    expect(model.end).toBeGreaterThanOrEqual(2100);
  });

  it('returns null when there is nothing to lay out', () => {
    expect(deriveTrajectoryTimeline([], 'sequence')).toBeNull();
  });
});

describe('trajectoryTimelineFocusIndexes', () => {
  const events = [
    event({ seq: 1, eventType: 'user-message', turn: 1, step: 0 }),
    event({ seq: 2, eventType: 'assistant-message', turn: 1, step: 1 }),
    event({ seq: 3, eventType: 'user-message', turn: 2, step: 0 }),
  ];

  it('returns the records overlapping the range', () => {
    const turns = deriveTrajectoryLayout(events);
    const all = trajectoryTimelineFocusIndexes(turns, { start: 0, end: 99 }, 'sequence');
    expect(all.size).toBe(3);

    const first = trajectoryTimelineFocusIndexes(turns, { start: 0, end: 0 }, 'sequence');
    expect(first.size).toBe(1);
  });

  it('returns an empty set for an empty layout', () => {
    expect(trajectoryTimelineFocusIndexes([], { start: 0, end: 1 }, 'sequence').size).toBe(0);
  });
});

describe('subagent dispatch cells', () => {
  const agentArgs = JSON.stringify({
    description: 'Audit the parser',
    prompt: '...',
    subagent_type: 'Explore',
  });

  function cellsOf(events: TrajectoryEvent[]) {
    return deriveTrajectoryLayout(events).flatMap((t) => t.groups.flatMap((g) => g.cells));
  }

  it('merges an Agent call and its result into one subtool record', () => {
    const cells = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Agent', toolArgs: agentArgs, toolCallId: 'c1' }),
      event({ seq: 2, eventType: 'tool-result', toolName: 'Agent', toolResult: 'done', toolCallId: 'c1' }),
    ]);
    expect(cells.map((c) => c.kind)).toEqual(['subtool']);
    expect(cells[0].result).toBe('done');
  });

  it('extracts the agent type, description and background flag', () => {
    const [cell] = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Agent', toolArgs: agentArgs }),
    ]);
    expect(cell.subagentType).toBe('Explore');
    expect(cell.subagentDescription).toBe('Audit the parser');
    expect(cell.text).toBe('Audit the parser');
    expect(cell.subagentBackground).toBe(false);
  });

  it('flags background dispatches', () => {
    const [cell] = cellsOf([
      event({
        seq: 1,
        eventType: 'tool-call',
        toolName: 'Agent',
        toolArgs: JSON.stringify({ description: 'x', run_in_background: true }),
      }),
    ]);
    expect(cell.subagentBackground).toBe(true);
  });

  it('accepts the legacy Task name but not task-tracking tools', () => {
    const kinds = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Task', toolArgs: agentArgs }),
      event({ seq: 2, eventType: 'tool-call', toolName: 'TaskCreate', toolArgs: '{"subject":"x"}' }),
      event({ seq: 3, eventType: 'tool-call', toolName: 'TaskUpdate', toolArgs: '{"id":"1"}' }),
    ]).map((c) => c.kind);
    expect(kinds).toEqual(['subtool', 'tool', 'tool']);
  });

  it('stays a plain tool when the arguments are unparseable', () => {
    const [cell] = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Agent', toolArgs: 'not json' }),
    ]);
    expect(cell.kind).toBe('subtool');
    expect(cell.subagentType).toBeUndefined();
    expect(cell.text).toBe('Agent');
  });

  it('counts subagent calls in the request aggregate', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', content: 'go', turn: 1, step: 0 }),
      event({ seq: 2, eventType: 'tool-call', toolName: 'Agent', toolArgs: agentArgs, turn: 1, step: 1 }),
      event({ seq: 3, eventType: 'tool-call', toolName: 'Bash', toolArgs: '{}', turn: 1, step: 1 }),
    ]);
    const request = aggregateRequestDetail(turns, 1);
    expect(request?.subtoolCalls).toBe(1);
    expect(request?.toolCalls).toBe(1);
  });
});

describe('tool result merging', () => {
  function cellsOf(events: TrajectoryEvent[]) {
    return deriveTrajectoryLayout(events).flatMap((t) => t.groups.flatMap((g) => g.cells));
  }

  it('folds a tool-result into its call, producing ONE record', () => {
    const cells = cellsOf([
      event({ seq: 1, eventType: 'user-message', content: 'go', turn: 1, step: 0 }),
      event({ seq: 2, eventType: 'tool-call', toolName: 'Bash', toolArgs: '{"command":"ls"}', toolCallId: 'c1', turn: 1, step: 1 }),
      event({ seq: 3, eventType: 'tool-result', toolName: 'Bash', content: null, toolResult: 'file1\nfile2', toolCallId: 'c1', turn: 1, step: 1 }),
    ]);
    const tools = cells.filter((c) => c.kind === 'tool');
    expect(tools).toHaveLength(1);
    expect(tools[0].inputDetail).toBe('{"command":"ls"}');
    expect(tools[0].result).toBe('file1\nfile2');
  });

  it('keeps the call duration from the result timestamp', () => {
    const cells = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Bash', toolArgs: '{}', toolCallId: 'c1', ts: 1000 }),
      event({ seq: 2, eventType: 'tool-result', toolName: 'Bash', toolResult: 'ok', toolCallId: 'c1', ts: 1500 }),
    ]);
    expect(cells[0].timeSeconds).toBe(0.5);
  });

  it('keeps an orphan tool-result as its own record', () => {
    const cells = cellsOf([
      event({ seq: 1, eventType: 'tool-result', toolName: 'Bash', toolResult: 'orphan', toolCallId: 'gone' }),
    ]);
    expect(cells).toHaveLength(1);
    expect(cells[0].result).toBe('orphan');
  });

  it('leaves tool calls without a matching result untouched', () => {
    const cells = cellsOf([
      event({ seq: 1, eventType: 'tool-call', toolName: 'Bash', toolArgs: '{}', toolCallId: 'c1' }),
    ]);
    expect(cells).toHaveLength(1);
    expect(cells[0].result).toBeUndefined();
  });
});

describe('TrajectorySearchIndex', () => {
  const events = [
    event({ seq: 1, eventType: 'user-message', content: 'find the config file' }),
    event({ seq: 2, eventType: 'tool-call', toolName: 'grep', toolArgs: '{"pattern":"secret"}' }),
  ];

  it('matches case-insensitive terms across records', () => {
    const index = new TrajectorySearchIndex();
    index.update(layoutsOf(events));
    const matches = index.search('CONFIG');
    expect(matches).not.toBeNull();
    expect(matches!.size).toBe(1);
  });

  it('requires every whitespace-separated term to match', () => {
    const index = new TrajectorySearchIndex();
    index.update(layoutsOf(events));
    expect(index.search('config secret')!.size).toBe(0); // different records
    expect(index.search('config file')!.size).toBe(1);
  });

  it('returns null for an empty query', () => {
    const index = new TrajectorySearchIndex();
    index.update(layoutsOf(events));
    expect(index.search('   ')).toBeNull();
  });

  it('drops entries that disappear from the layout', () => {
    const index = new TrajectorySearchIndex();
    index.update(layoutsOf(events));
    expect(index.search('config')!.size).toBe(1);
    index.update(layoutsOf([events[1]]));
    expect(index.search('config')!.size).toBe(0);
  });

  // Regression: null content used to blow up while building the index.
  it('survives null message content', () => {
    const index = new TrajectorySearchIndex();
    expect(() =>
      index.update(layoutsOf([event({ eventType: 'assistant-message', content: null })])),
    ).not.toThrow();
  });
});

describe('groupVirtualRows / trajectoryRecordId', () => {
  it('gives every row a unique key even when callId is shared', () => {
    const turns = deriveTrajectoryLayout([
      event({ seq: 1, eventType: 'user-message', turn: 1, step: 0 }),
      event({ seq: 2, eventType: 'tool-call', turn: 1, step: 1, toolCallId: 'call_1', toolName: 'bash' }),
      event({ seq: 3, eventType: 'tool-result', turn: 1, step: 1, toolCallId: 'call_1', toolName: 'bash' }),
    ]);
    const cells = turns.flatMap((t) => t.groups.flatMap((g) => g.cells));
    const rows = groupVirtualRows(cells);
    const keys = rows.map((r) => r.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it('prefers recordId, then callId, then sourceSeq', () => {
    expect(trajectoryRecordId({ recordId: 'r1', kind: 'tool', index: 0 })).toBe('r1');
    expect(trajectoryRecordId({ callId: 'c1', kind: 'tool', index: 0 })).toContain('c1');
    expect(trajectoryRecordId({ sourceSeq: 7, kind: 'tool', index: 0 })).toContain('7');
    expect(trajectoryRecordId({ kind: 'tool', index: 3 })).toContain('3');
  });
});

describe('assistantTimingDetail / timelineRecordDetail', () => {
  it('derives ttft and decoding from recorded timings', () => {
    const detail = assistantTimingDetail({
      timingRecorded: true,
      stepStartTime: 1000,
      firstTokenTime: 1200,
      completedTime: 2000,
      usageProvided: true,
      outputTokens: 50,
    });
    expect(detail.ttftMs).toBe(200);
    expect(detail.decodingMs).toBe(800);
  });

  it('omits timings when they are missing or out of order', () => {
    expect(assistantTimingDetail(null)).toEqual({});
    expect(
      assistantTimingDetail({
        timingRecorded: false,
        stepStartTime: 1,
        firstTokenTime: 2,
        completedTime: 3,
        usageProvided: false,
        outputTokens: null,
      }),
    ).toEqual({});
    // completed before first token → not usable
    expect(
      assistantTimingDetail({
        timingRecorded: true,
        stepStartTime: 100,
        firstTokenTime: 300,
        completedTime: 200,
        usageProvided: true,
        outputTokens: 1,
      }),
    ).toEqual({});
  });

  it('converts seconds to milliseconds for the record detail', () => {
    const turns = deriveTrajectoryLayout([
      event({ eventType: 'assistant-message', turn: 1, step: 1, ts: 5000, durationMs: 1500 }),
    ]);
    const cell = turns[0].groups[0].cells[0];
    const detail = timelineRecordDetail(cell);
    expect(detail.durationMs).toBe(1500);
    expect(detail.startedAt).toBe(5000);
  });
});

describe('aggregateRequestDetail', () => {
  const events = [
    event({ seq: 1, eventType: 'user-message', turn: 1, step: 0 }),
    event({
      seq: 2,
      eventType: 'assistant-message',
      turn: 1,
      step: 1,
      inputTokens: 100,
      outputTokens: 20,
    }),
    event({ seq: 3, eventType: 'tool-call', turn: 1, step: 1, toolName: 'bash', toolCallId: 'c1' }),
    event({ seq: 4, eventType: 'tool-result', turn: 1, step: 1, toolName: 'bash', toolCallId: 'c1' }),
  ];

  it('aggregates the assistant step around a selected record', () => {
    const turns = deriveTrajectoryLayout(events);
    const target = turns[0].groups.flatMap((g) => g.cells).find((c) => c.kind === 'message')!;
    const request = aggregateRequestDetail(turns, target.index);

    expect(request).not.toBeNull();
    expect(request!.turn).toBe(1);
    expect(request!.toolCalls).toBe(1); // one call, its result is folded in
    expect(request!.usage?.input).toBe(100);
    expect(request!.usage?.output).toBe(20);
    expect(request!.state).toBe('complete');
  });

  it('reports error state when any record errored', () => {
    const turns = deriveTrajectoryLayout([
      ...events,
      event({ seq: 5, eventType: 'tool-result', turn: 1, step: 1, isError: true, toolCallId: 'c2' }),
    ]);
    const target = turns[0].groups.flatMap((g) => g.cells).find((c) => c.kind === 'message')!;
    expect(aggregateRequestDetail(turns, target.index)!.state).toBe('error');
  });

  it('returns null when the index is not in any turn group', () => {
    // deriveTrajectoryLayout always assigns a turn (derived when missing), so
    // an out-of-range index is the realistic "not in any request" case.
    const turns = deriveTrajectoryLayout(events);
    expect(aggregateRequestDetail(turns, 9999)).toBeNull();
  });
});

describe('summarizeTurnText', () => {
  it('pluralizes steps and calls', () => {
    expect(summarizeTurnText(1, 1)).toBe('1 step · 1 tool call');
    expect(summarizeTurnText(3, 5)).toBe('3 steps · 5 tool calls');
  });
});
