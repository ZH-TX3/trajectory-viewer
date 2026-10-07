// ── Subagent Card ────────────────────────────────────────────────────────
//
// One dispatched subagent, as shown in the Messages tab. Mirrors the
// Trajectory tab's dispatch row — agent type, status, cost, spawn depth and
// worktree — and expands in place to read the run's own conversation.
//
// The run's transcript is loaded on demand: one session can hold a dozen runs
// and thousands of messages between them.

import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { api } from '../api';
import { cn } from '../lib/utils';
import { formatDurationMs, formatTokenCount } from '../utils/format';
import type { SessionMessage, SubagentRun } from '../types';

/** One-line cost summary: tools · tokens · duration. */
function costSummary(run: SubagentRun): string {
  const parts: string[] = [];
  const tools =
    run.toolUseCount ??
    (run.stats != null
      ? run.stats.readCount +
        run.stats.searchCount +
        run.stats.bashCount +
        run.stats.editFileCount +
        run.stats.otherToolCount
      : null);
  if (tools != null && tools > 0) parts.push(`${tools} tools`);
  if (run.totalTokens != null) parts.push(`${formatTokenCount(run.totalTokens)} tok`);
  if (run.durationMs != null) parts.push(formatDurationMs(run.durationMs));
  return parts.join(' · ');
}

/** Status pill styling: failures red, user-stopped amber, rest neutral. */
function statusClass(status: string): string {
  if (status === 'failed' || status === 'killed') {
    return 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300';
  }
  if (status === 'stopped') {
    return 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300';
  }
  if (status === 'async_launched') {
    return 'bg-cyan-100 dark:bg-cyan-900/30 text-cyan-700 dark:text-cyan-300';
  }
  return 'bg-muted text-muted-foreground';
}

interface SubagentCardProps {
  run: SubagentRun;
  /** Main session file — the run's transcript lives beside it. */
  sourcePath?: string | null;
  /** Nesting level, for indenting a transcript's own dispatches. */
  depth?: number;
}

export function SubagentCard({ run, sourcePath, depth = 0 }: SubagentCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [messages, setMessages] = useState<SessionMessage[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const cost = costSummary(run);
  const canExpand = run.hasTranscript && sourcePath != null;

  const toggle = async () => {
    if (expanded) {
      setExpanded(false);
      return;
    }
    setExpanded(true);
    if (messages !== null || !canExpand) return;
    setLoading(true);
    setError(null);
    try {
      setMessages(await api.getSubagentMessages(sourcePath, run.agentId));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      className="rounded border border-cyan-200/60 dark:border-cyan-900/50 bg-cyan-50/40 dark:bg-cyan-950/20 px-2 py-1.5"
      style={depth > 0 ? { marginLeft: depth * 12 } : undefined}
    >
      <div className="flex items-center gap-2 min-w-0">
        {canExpand ? (
          <button
            type="button"
            onClick={toggle}
            aria-expanded={expanded}
            title={expanded ? 'Collapse subagent conversation' : 'Expand subagent conversation'}
            className="shrink-0 px-0.5 leading-none text-cyan-600 dark:text-cyan-400 hover:text-cyan-800 dark:hover:text-cyan-200"
          >
            {expanded ? '▾' : '▸'}
          </button>
        ) : (
          <span className="shrink-0 w-[11px]" aria-hidden />
        )}

        <span className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] font-mono bg-cyan-100 dark:bg-cyan-900/30 text-cyan-700 dark:text-cyan-300">
          {run.agentType ?? 'Agent'}
        </span>

        {run.spawnDepth > 1 && (
          <span
            title={`Nested subagent — spawn depth ${run.spawnDepth}`}
            className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] font-mono bg-muted text-muted-foreground"
          >
            d{run.spawnDepth}
          </span>
        )}

        {run.worktreeBranch != null && (
          <span
            title={`Isolated worktree: ${run.worktreePath ?? ''} (${run.worktreeBranch})`}
            className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] font-mono bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300"
          >
            ⎇
          </span>
        )}

        {run.isFork && (
          <span
            title="Forked from the parent conversation"
            className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] bg-muted text-muted-foreground"
          >
            fork
          </span>
        )}

        {run.stoppedByUser && (
          <span
            title="Stopped by the user"
            className="shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300"
          >
            ⏹
          </span>
        )}

        {run.status != null && (
          <span
            className={cn(
              'shrink-0 inline-flex items-center px-1 py-0.5 rounded text-[10px] font-mono',
              statusClass(run.status),
            )}
          >
            {run.status}
          </span>
        )}

        <span className="text-xs text-foreground/80 truncate min-w-0">
          {run.description ?? run.name ?? run.agentId}
        </span>

        {cost !== '' && (
          <span className="shrink-0 ml-auto text-[10px] text-muted-foreground font-mono">{cost}</span>
        )}
      </div>

      {expanded && (
        <div className="mt-1.5 pl-3 border-l border-cyan-300/50 dark:border-cyan-800/50">
          {loading && (
            <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground py-1">
              <Loader2 className="size-3 animate-spin" />
              Loading subagent conversation…
            </div>
          )}
          {error !== null && (
            <div className="text-[11px] text-red-600 dark:text-red-400 py-1">
              Could not load subagent conversation: {error}
            </div>
          )}
          {!loading && error === null && messages !== null && messages.length === 0 && (
            <div className="text-[11px] text-muted-foreground py-1">No messages recorded</div>
          )}
          {!loading &&
            error === null &&
            messages?.map((message, index) => (
              <SubagentMessage
                key={index}
                message={message}
                sourcePath={sourcePath}
                depth={depth + 1}
              />
            ))}
        </div>
      )}
    </div>
  );
}

/**
 * One message inside an expanded transcript. Nested dispatches recurse back
 * into `SubagentCard`, so a subagent's own subagents expand the same way.
 */
function SubagentMessage({
  message,
  sourcePath,
  depth,
}: {
  message: SessionMessage;
  sourcePath?: string | null;
  depth: number;
}) {
  // A dispatch line has no prose of its own — the card says it all, so the
  // bare `[Tool: Agent]` text would only be noise.
  if (message.subagents != null && message.subagents.length > 0) {
    return (
      <div className="py-0.5">
        {message.subagents.map((run) => (
          <SubagentCard key={run.agentId} run={run} sourcePath={sourcePath} depth={depth} />
        ))}
      </div>
    );
  }

  return (
    <div className="flex gap-2 py-0.5">
      <div
        className={cn(
          'shrink-0 w-14 text-[10px] font-mono text-right pt-0.5',
          message.role === 'user' && 'text-emerald-600 dark:text-emerald-400',
          message.role === 'assistant' && 'text-violet-600 dark:text-violet-400',
          message.role === 'tool' && 'text-amber-600 dark:text-amber-400',
        )}
      >
        {message.role}
      </div>
      <div className="flex-1 min-w-0 text-xs text-foreground/80 whitespace-pre-wrap break-words leading-relaxed">
        {message.content}
      </div>
    </div>
  );
}
