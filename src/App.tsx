// ── App Root ─────────────────────────────────────────────────────────────
//
// Two views: session browser (default) or standalone file trajectory view.

import { useState, useCallback } from 'react';
import { SessionBrowser } from './components/SessionBrowser';
import { TrajectoryView } from './components/TrajectoryView';
import { api } from './api';
import type { TrajectoryData } from './types';
import { ArrowLeft, FileText } from 'lucide-react';

export function App() {
  const [mode, setMode] = useState<'browser' | 'file'>('browser');
  const [trajectoryData, setTrajectoryData] = useState<TrajectoryData | null>(null);
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleFileOpen = useCallback(async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'JSONL', extensions: ['jsonl'] }],
      });
      const path = selected as string | null;
      if (!path) return;

      setIsLoading(true);
      setError(null);
      const data = await api.parseTrajectoryFile(path);
      setTrajectoryData(data);
      setSourcePath(path);
      setMode('file');
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  const handleBack = useCallback(() => {
    setMode('browser');
    setTrajectoryData(null);
    setSourcePath(null);
    setError(null);
  }, []);

  return (
    <div className="h-screen flex flex-col bg-background text-foreground">
      {/* Header bar */}
      <header className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-background/95 backdrop-blur-sm">
        {mode === 'file' && (
          <button
            onClick={handleBack}
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowLeft className="size-3.5" />
            Back
          </button>
        )}
        <div className="flex items-center gap-1.5 text-sm">
          <FileText className="size-4" />
          Trajectory Viewer
        </div>
        {sourcePath && (
          <span className="text-[10px] text-muted-foreground truncate max-w-[300px] ml-2" title={sourcePath}>
            {sourcePath}
          </span>
        )}
      </header>

      {/* Main content */}
      <main className="flex-1 min-h-0">
        {mode === 'browser' && (
          <SessionBrowser onOpenFile={handleFileOpen} />
        )}

        {mode === 'file' && trajectoryData && (
          <TrajectoryView data={trajectoryData} />
        )}

        {mode === 'file' && !trajectoryData && (
          <div className="p-6 h-full flex flex-col items-center justify-center">
            {isLoading ? (
              <div className="text-xs text-muted-foreground">Loading…</div>
            ) : error ? (
              <div className="p-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 text-xs text-red-600 dark:text-red-400">
                {error}
              </div>
            ) : null}
          </div>
        )}
      </main>
    </div>
  );
}