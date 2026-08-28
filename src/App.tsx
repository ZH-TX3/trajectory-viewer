// ── App Root ─────────────────────────────────────────────────────────────
//
// Two views: session browser (default) or standalone file trajectory view.

import { useState, useCallback } from 'react';
import { SessionBrowser } from './components/SessionBrowser';
import { SettingsView } from './components/SettingsView';
import { TrajectoryView } from './components/TrajectoryView';
import { api } from './api';
import type { TrajectoryData } from './types';
import { ArrowLeft, FileText, Settings } from 'lucide-react';

const AVAILABLE_PROVIDERS = ['claude', 'codex', 'dsh', 'opencode'] as const;
const SETTINGS_KEY = 'trajectory-viewer.providers.enabled-v1';

/** Enabled session providers, persisted locally. Defaults to all supported. */
function loadEnabledProviders(): Set<string> {
  try {
    const saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? 'null');
    if (Array.isArray(saved)) {
      return new Set(
        saved.filter(
          (p): p is string =>
            typeof p === 'string' && (AVAILABLE_PROVIDERS as readonly string[]).includes(p),
        ),
      );
    }
  } catch {
    /* ignore */
  }
  return new Set(AVAILABLE_PROVIDERS);
}

export function App() {
  const [mode, setMode] = useState<'browser' | 'settings' | 'file'>('browser');
  const [enabledProviders, setEnabledProviders] = useState<Set<string>>(loadEnabledProviders);
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

  const handleToggleProvider = useCallback((id: string) => {
    setEnabledProviders((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      localStorage.setItem(
        SETTINGS_KEY,
        JSON.stringify([...next].filter((p) => (AVAILABLE_PROVIDERS as readonly string[]).includes(p))),
      );
      return next;
    });
  }, []);

  return (
    <div className="h-screen flex flex-col bg-background text-foreground">
      {/* Header bar */}
      {/* Global header — hidden on the settings page, which has its own header */}
      {mode !== 'settings' && (
        <header className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-background/95 backdrop-blur-sm relative z-50">
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

          <div className="ml-auto flex items-center gap-1 relative">
            <button
              onClick={() => setMode('settings')}
              title="Settings"
              className="p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
            >
              <Settings className="size-4" />
            </button>
          </div>
        </header>
      )}

      {/* Main content */}
      <main className="flex-1 min-h-0">
        {mode === 'browser' && (
          <SessionBrowser onOpenFile={handleFileOpen} enabledProviders={enabledProviders} />
        )}

        {mode === 'settings' && (
          <SettingsView
            enabledProviders={enabledProviders}
            onToggleProvider={handleToggleProvider}
            onBack={handleBack}
          />
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