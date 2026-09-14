// ── Config Hub: delete confirmation ──────────────────────────────────────
//
// Deleting unlinks the resource from every tool and moves the unified-store
// entry to the trash, so it stays recoverable.

import { Loader2 } from 'lucide-react';
import type { ConfigResource } from '../../types';
import { KIND_LABELS } from '../../utils/configHub';

interface DeleteConfirmDialogProps {
  resource: ConfigResource;
  /** Tools currently linking or copying this resource. */
  linkedTools: string[];
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteConfirmDialog({
  resource,
  linkedTools,
  busy,
  onConfirm,
  onCancel,
}: DeleteConfirmDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div
        className="w-full max-w-md rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg"
      >
        <div className="text-sm font-medium mb-2">
          Delete {KIND_LABELS[resource.kind].toLowerCase()} “{resource.name}”?
        </div>
        <p className="text-[11px] text-muted-foreground leading-relaxed mb-3">
          It will be unlinked from{' '}
          {linkedTools.length > 0 ? linkedTools.join(', ') : 'every tool'} and moved to the
          trash. You can restore it from there.
        </p>

        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            disabled={busy}
            className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            className="inline-flex items-center gap-1.5 px-3 py-1 rounded text-xs bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-60"
          >
            {busy && <Loader2 className="size-3 animate-spin" />}
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
