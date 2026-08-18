// ── File Drop Zone ────────────────────────────────────────────────────────
//
// Landing page: drag-and-drop zone + file picker button.

import { useCallback, useRef, useState } from 'react';
import { Upload } from 'lucide-react';
import { cn } from '../lib/utils';

interface FileDropZoneProps {
  onFileOpen: () => void;
  isLoading?: boolean;
}

export function FileDropZone({ onFileOpen, isLoading }: FileDropZoneProps) {
  const [dragging, setDragging] = useState(false);
  const dragCounter = useRef(0);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current++;
    if (e.dataTransfer.items?.length > 0) {
      setDragging(true);
    }
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounter.current--;
    if (dragCounter.current === 0) {
      setDragging(false);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setDragging(false);
      dragCounter.current = 0;

      // Drag-and-drop is handled by Tauri's dialog plugin,
      // but we can also receive file paths via the drop event.
      // For now, we just open the dialog.
      onFileOpen();
    },
    [onFileOpen],
  );

  return (
    <div
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      className={cn(
        'flex flex-col items-center justify-center min-h-[400px] rounded-xl border-2 border-dashed transition-all',
        dragging
          ? 'border-primary bg-primary/5 scale-[1.02]'
          : 'border-border/40 hover:border-border/60',
      )}
    >
      <div className="flex flex-col items-center gap-4 p-8 text-center">
        <div className="rounded-full bg-muted p-4">
          <Upload className="size-8 text-muted-foreground" />
        </div>

        <div>
          <p className="text-sm font-medium text-foreground">
            Drop a JSONL session file here
          </p>
          <p className="text-xs text-muted-foreground mt-1">
            or click the button below to browse
          </p>
        </div>

        <button
          onClick={onFileOpen}
          disabled={isLoading}
          className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium
            bg-primary text-primary-foreground hover:bg-primary/90
            disabled:opacity-50 disabled:cursor-not-allowed
            transition-colors"
        >
          {isLoading ? (
            <span className="inline-block size-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
          ) : (
            <Upload className="size-4" />
          )}
          {isLoading ? 'Loading…' : 'Open Trajectory File'}
        </button>

        <p className="text-xs text-muted-foreground/60 max-w-xs">
          Supports Claude Code and Codex JSONL session logs
        </p>
      </div>
    </div>
  );
}