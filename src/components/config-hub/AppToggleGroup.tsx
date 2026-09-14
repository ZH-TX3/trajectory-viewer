// ── Config Hub: per-tool toggle group ────────────────────────────────────
//
// One square brand-icon button per tool that can link this resource, mirroring
// cc-switch's `AppToggleGroup`: a `w-7` chip that turns the tool's tinted
// active class when on, and dims when off.
//
// A `drifted` tool holds the user's own unmanaged copy — it can't be toggled
// off (that would delete their file), so it gets an amber Import button.

import { Loader2 } from 'lucide-react';
import type { AppState, ConfigResource, ToolInfo } from '../../types';
import { APP_STATE_LABELS, TOOL_ACTIVE_CLASSES } from '../../utils/configHub';
import { ToolBadge } from './BrandIcons';
import { cn } from '../../lib/utils';

interface AppToggleGroupProps {
  resource: ConfigResource;
  tools: ToolInfo[];
  busyKey: string | null;
  onToggle: (resource: ConfigResource, toolId: string, enabled: boolean) => void;
  onImport: (resource: ConfigResource, toolId: string) => void;
}

export function AppToggleGroup({
  resource,
  tools,
  busyKey,
  onToggle,
  onImport,
}: AppToggleGroupProps) {
  return (
    <div className="flex items-center gap-1">
      {tools.map((tool) => {
        const state: AppState = resource.apps[tool.id] ?? 'absent';
        const key = `${resource.id}@${tool.id}`;
        const busy = busyKey === key;

        const base =
          'w-7 h-7 rounded-lg flex items-center justify-center transition-all disabled:opacity-50 shrink-0';

        if (state === 'drifted') {
          return (
            <button
              key={tool.id}
              onClick={() => onImport(resource, tool.id)}
              disabled={busy}
              title={`${tool.displayName}: ${APP_STATE_LABELS.drifted} — click to import`}
              className={cn(
                base,
                'bg-amber-500/10 ring-1 ring-amber-500/20 hover:bg-amber-500/20 text-amber-600 dark:text-amber-400',
              )}
            >
              {busy ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <ToolBadge toolId={tool.id} size={14} />
              )}
            </button>
          );
        }

        const enabled = state === 'linked' || state === 'copied';
        return (
          <button
            key={tool.id}
            onClick={() => onToggle(resource, tool.id, !enabled)}
            disabled={busy}
            aria-pressed={enabled}
            title={`${tool.displayName}: ${APP_STATE_LABELS[state]} — click to ${enabled ? 'disable' : 'enable'}`}
            className={cn(
              base,
              enabled
                ? TOOL_ACTIVE_CLASSES[tool.id] ?? 'bg-muted/40 text-foreground'
                : 'opacity-40 hover:opacity-80',
            )}
          >
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : <ToolBadge toolId={tool.id} size={14} />}
          </button>
        );
      })}
    </div>
  );
}