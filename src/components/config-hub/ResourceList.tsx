// ── Config Hub: resource list ────────────────────────────────────────────
//
// One row per skill/agent: its name, whether it lives in the unified store,
// a per-tool switch group, and a delete action. A row that any tool holds an
// unmanaged copy of is marked so it's obvious what still needs importing.

import { AlertTriangle, CheckCheck, Loader2, Trash2 } from 'lucide-react';
import type { ConfigResource, ToolInfo } from '../../types';
import { KIND_LABELS, hasDrift, linkableTools } from '../../utils/configHub';
import { cn } from '../../lib/utils';
import { AppToggleGroup } from './AppToggleGroup';

interface ResourceListProps {
  resources: ConfigResource[];
  tools: ToolInfo[];
  busyKey: string | null;
  onToggle: (resource: ConfigResource, toolId: string, enabled: boolean) => void;
  onToggleAll: (resource: ConfigResource, toolIds: string[], enabled: boolean) => void;
  onImport: (resource: ConfigResource, toolId: string) => void;
  onDelete: (resource: ConfigResource) => void;
}

export function ResourceList({
  resources,
  tools,
  busyKey,
  onToggle,
  onToggleAll,
  onImport,
  onDelete,
}: ResourceListProps) {
  if (resources.length === 0) {
    return (
      <div className="p-6 text-center text-xs text-muted-foreground">
        No resources match the current filters.
      </div>
    );
  }

  return (
    <div className="divide-y divide-border/40">
      {resources.map((resource) => {
        const rowTools = linkableTools(tools, resource.kind);
        const drift = hasDrift(resource);
        // Bulk actions only touch tools that are ours to change: a drifted
        // entry holds the user's own copy and must be imported first.
        const togglable = rowTools
          .filter((tool) => {
            const state = resource.apps[tool.id];
            return state === 'linked' || state === 'copied' || state === 'absent';
          })
          .map((tool) => tool.id);
        const enabledCount = rowTools.filter((tool) => {
          const state = resource.apps[tool.id];
          return state === 'linked' || state === 'copied';
        }).length;
        const allOn = togglable.length > 0 && enabledCount === togglable.length;
        const busyAll = busyKey === `all:${resource.id}`;
        return (
          <div key={resource.id} className="px-4 py-2 flex items-center gap-3">
            <span
              className={cn(
                'text-[9px] uppercase tracking-wide rounded px-1 py-0.5 border shrink-0',
                resource.kind === 'agent'
                  ? 'border-purple-500/40 text-purple-600 dark:text-purple-400'
                  : 'border-sky-500/40 text-sky-600 dark:text-sky-400',
              )}
            >
              {KIND_LABELS[resource.kind]}
            </span>

            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-1.5">
                <span className="text-xs truncate">{resource.name}</span>
                {drift && (
                  <span title="A tool holds an unmanaged copy">
                    <AlertTriangle className="size-3 text-amber-500 shrink-0" />
                  </span>
                )}
                {!resource.inSsot && (
                  <span className="text-[9px] text-muted-foreground border border-border/40 rounded px-1">
                    not in store
                  </span>
                )}
              </div>
              {resource.description && (
                <div className="text-[10px] text-muted-foreground truncate">
                  {resource.description}
                </div>
              )}
            </div>

            {togglable.length > 1 && (
              <button
                onClick={() => onToggleAll(resource, togglable, !allOn)}
                disabled={busyAll}
                title={allOn ? 'Disable for all tools' : 'Enable for all tools'}
                className={cn(
                  'inline-flex items-center gap-1 px-1.5 py-1 rounded text-[9px] border transition-colors disabled:opacity-40 shrink-0',
                  allOn
                    ? 'border-green-500/50 bg-green-500/10 text-green-700 dark:text-green-400'
                    : 'border-border/50 text-muted-foreground hover:text-foreground hover:bg-muted/40',
                )}
              >
                {busyAll ? (
                  <Loader2 className="size-3 animate-spin" />
                ) : (
                  <CheckCheck className="size-3" />
                )}
                all
              </button>
            )}

            <AppToggleGroup
              resource={resource}
              tools={rowTools}
              busyKey={busyKey}
              onToggle={onToggle}
              onImport={onImport}
            />

            <button
              onClick={() => onDelete(resource)}
              disabled={!resource.inSsot}
              title={resource.inSsot ? 'Delete (moves to trash)' : 'Only in the unified store can be deleted'}
              className="p-1 rounded text-red-500 hover:bg-red-500/10 transition-colors disabled:opacity-30 shrink-0"
            >
              <Trash2 className="size-3" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
