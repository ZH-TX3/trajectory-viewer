// ── Config Hub: import preview dialog ────────────────────────────────────
//
// Importing moves a tool's copy into the unified store, so it is never done
// blind: this shows exactly which file moves where, and asks how to handle a
// same-named entry already in the store.

import { Loader2 } from 'lucide-react';
import type { ImportPreview } from '../../types';
import { KIND_LABELS } from '../../utils/configHub';

interface ImportPreviewDialogProps {
  preview: ImportPreview;
  busy: boolean;
  onConfirm: (overwrite: boolean) => void;
  onCancel: () => void;
}

export function ImportPreviewDialog({
  preview,
  busy,
  onConfirm,
  onCancel,
}: ImportPreviewDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div
        className="w-full max-w-lg rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg"
      >
        <div className="text-sm font-medium mb-2">
          Import {KIND_LABELS[preview.kind]} “{preview.name}”
        </div>
        <p className="text-[11px] text-muted-foreground leading-relaxed mb-3">
          {preview.action}.
        </p>

        <dl className="text-[11px] space-y-1.5 mb-3">
          <div className="flex gap-2">
            <dt className="text-muted-foreground w-24 shrink-0">From</dt>
            <dd className="font-mono break-all">{preview.sourcePath}</dd>
          </div>
          <div className="flex gap-2">
            <dt className="text-muted-foreground w-24 shrink-0">To</dt>
            <dd className="font-mono break-all">{preview.targetPath}</dd>
          </div>
        </dl>

        {preview.conflict && (
          <div className="rounded border border-amber-300/60 bg-amber-50 dark:bg-amber-900/15 px-3 py-2 text-[11px] text-amber-800 dark:text-amber-200 mb-3">
            The unified store already has a {KIND_LABELS[preview.kind].toLowerCase()} named “
            {preview.name}”. Overwrite it (the old copy goes to the trash) or skip this import.
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            disabled={busy}
            className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          {preview.conflict && (
            <button
              onClick={() => onConfirm(false)}
              disabled={busy}
              className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors disabled:opacity-50"
            >
              Skip
            </button>
          )}
          <button
            onClick={() => onConfirm(preview.conflict)}
            disabled={busy}
            className="inline-flex items-center gap-1.5 px-3 py-1 rounded text-xs bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-60"
          >
            {busy && <Loader2 className="size-3 animate-spin" />}
            {preview.conflict ? 'Overwrite' : 'Import'}
          </button>
        </div>
      </div>
    </div>
  );
}
