// ── Trajectory Toolbar ────────────────────────────────────────────────────

import { Search, ChevronUp, ChevronDown, Clock } from 'lucide-react';
import { cn } from '../lib/utils';

interface TrajectoryToolbarProps {
  searchQuery: string;
  onSearchChange: (query: string) => void;
  onToggleAllTurns: () => void;
  onToggleAllAssistants: () => void;
  allTurnsCollapsed: boolean;
  allAssistantsCollapsed: boolean;
  actualDuration: boolean;
  onToggleDuration: () => void;
  turnCount: number;
  callCount: number;
}

export function TrajectoryToolbar({
  searchQuery,
  onSearchChange,
  onToggleAllTurns,
  onToggleAllAssistants,
  allTurnsCollapsed,
  allAssistantsCollapsed,
  actualDuration,
  onToggleDuration,
  turnCount,
  callCount,
}: TrajectoryToolbarProps) {
  return (
    <div className="sticky top-0 z-10 h-8 border-b border-border/40 bg-background/95 backdrop-blur-sm flex items-center gap-2 px-2">
      {/* Turn collapse toggle */}
      <button
        onClick={onToggleAllTurns}
        title={allTurnsCollapsed ? 'Expand turns' : 'Collapse turns'}
        className="h-5 px-1.5 text-[11px] text-muted-foreground hover:text-foreground rounded transition-colors flex items-center gap-1 shrink-0"
      >
        {allTurnsCollapsed ? <ChevronDown className="size-3" /> : <ChevronUp className="size-3" />}
        <span className="hidden sm:inline">Turns</span>
        <span className="text-[10px] opacity-60">{turnCount}</span>
      </button>

      {/* Call collapse toggle */}
      <button
        onClick={onToggleAllAssistants}
        title={allAssistantsCollapsed ? 'Expand calls' : 'Collapse calls'}
        className="h-5 px-1.5 text-[11px] text-muted-foreground hover:text-foreground rounded transition-colors flex items-center gap-1 shrink-0"
      >
        {allAssistantsCollapsed ? <ChevronDown className="size-3" /> : <ChevronUp className="size-3" />}
        <span className="hidden sm:inline">Calls</span>
        <span className="text-[10px] opacity-60">{callCount}</span>
      </button>

      {/* Duration toggle */}
      <button
        onClick={onToggleDuration}
        title={actualDuration ? 'Use equal-width operations' : 'Use actual duration'}
        className={cn(
          'h-5 px-1.5 text-[11px] rounded transition-colors flex items-center gap-1 shrink-0',
          actualDuration
            ? 'text-foreground bg-muted/60'
            : 'text-muted-foreground hover:text-foreground',
        )}
      >
        <Clock className="size-3" />
        <span className="hidden sm:inline">Duration</span>
      </button>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Search */}
      <div className="flex items-center gap-1 h-5">
        <Search className="size-3 text-muted-foreground shrink-0" />
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="Search"
          className="h-5 w-24 sm:w-32 bg-transparent text-[11px] text-foreground placeholder:text-muted-foreground/50 border-none outline-none"
        />
        {searchQuery && (
          <button
            onClick={() => onSearchChange('')}
            className="text-muted-foreground hover:text-foreground text-[10px] px-1"
          >
            ×
          </button>
        )}
      </div>
    </div>
  );
}