// ── Update Check (Settings → About) ──────────────────────────────────────
//
// Shows the running version and a manual "Check for updates" button, backed by
// src-tauri/src/update.rs. The check only *reports* — it compares this build's
// version against this repo's newest GitHub release and offers a link. Nothing
// is downloaded or installed here; that needs a signed updater manifest.
//
// An automatic check fires when this panel opens, throttled backend-side to one
// every few hours and silenced by the toggle below.

import { useCallback, useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { ExternalLink, Loader2, RefreshCw, RotateCw } from 'lucide-react';
import { api } from '../api';
import type { UpdateCheck, UpdateSettings } from '../types';

type Result =
  | { kind: 'upToDate'; check: UpdateCheck }
  | { kind: 'available'; check: UpdateCheck }
  | { kind: 'error'; message: string };

/** Newer + not skipped is the only case worth a call to action. */
function toResult(check: UpdateCheck): Result {
  return check.hasUpdate ? { kind: 'available', check } : { kind: 'upToDate', check };
}

function publishDate(iso: string | null): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? null : date.toLocaleDateString();
}

export function UpdateSection() {
  const [version, setVersion] = useState<string | null>(null);
  const [settings, setSettings] = useState<UpdateSettings | null>(null);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const [opening, setOpening] = useState(false);

  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
    void api
      .getUpdateSettings()
      .then(setSettings)
      .catch(() => setSettings(null));
  }, []);

  // Quiet check on open. A failure here stays silent — it is background noise,
  // and the manual button is right there if the user wants an error.
  useEffect(() => {
    let cancelled = false;
    void api
      .autoCheckForUpdates()
      .then((check) => {
        if (!cancelled && check !== null) setResult(toResult(check));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const runCheck = useCallback(async () => {
    setChecking(true);
    setResult(null);
    try {
      setResult(toResult(await api.checkForUpdates()));
    } catch (err) {
      setResult({ kind: 'error', message: String(err) });
    } finally {
      setChecking(false);
    }
  }, []);

  const changeSettings = useCallback(
    async (patch: Partial<UpdateSettings>) => {
      if (settings === null) return;
      const previous = settings;
      const next = { ...settings, ...patch };
      setSettings(next);
      try {
        await api.setUpdateSettings(next);
      } catch {
        setSettings(previous); // revert — the toggle must not lie about state
      }
    },
    [settings],
  );

  const openRelease = useCallback(async (url: string) => {
    setOpening(true);
    try {
      await api.openReleasePage(url);
    } catch {
      // The browser failing to launch is not actionable from here.
    } finally {
      setOpening(false);
    }
  }, []);

  const skip = useCallback(
    async (latest: string | null) => {
      if (latest === null) return;
      await api.skipUpdateVersion(latest).catch(() => undefined);
      void api
        .getUpdateSettings()
        .then(setSettings)
        .catch(() => undefined);
      setResult(null);
    },
    [],
  );

  const status = (() => {
    if (checking) return 'Asking GitHub for the newest release…';
    if (result === null) return 'Not checked yet';
    if (result.kind === 'error') return result.message;
    if (result.kind === 'available') return `v${result.check.latestVersion} is available`;
    if (result.check.skipped) {
      return `v${result.check.latestVersion} exists, but you asked to skip it`;
    }
    return 'Up to date';
  })();

  return (
    <div>
      <div className="px-4 py-3 flex items-center gap-3">
        <div className="flex-1 min-w-0">
          <div className="text-xs font-medium">
            Trajectory Viewer
            {version !== null && (
              <span className="ml-2 font-mono text-[11px] text-muted-foreground">
                v{version}
              </span>
            )}
          </div>
          <div
            className={
              result?.kind === 'error'
                ? 'text-[10px] mt-0.5 text-red-600 dark:text-red-400 break-all'
                : 'text-[10px] mt-0.5 text-muted-foreground'
            }
          >
            {status}
          </div>
        </div>
        <button
          onClick={runCheck}
          disabled={checking}
          className="shrink-0 inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-60"
        >
          {checking ? <Loader2 className="size-3 animate-spin" /> : <RotateCw className="size-3" />}
          {checking ? 'Checking…' : 'Check for updates'}
        </button>
      </div>

      {result?.kind === 'available' && (
        <div className="mx-4 mb-3 rounded-lg border border-emerald-300/60 bg-emerald-50 dark:bg-emerald-900/15 p-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[11px] font-medium text-emerald-800 dark:text-emerald-200">
              v{result.check.latestVersion} is available
            </span>
            {publishDate(result.check.publishedAt) !== null && (
              <span className="text-[10px] text-emerald-700/70 dark:text-emerald-300/70">
                {publishDate(result.check.publishedAt)}
              </span>
            )}
          </div>

          {result.check.releaseNotes !== null && result.check.releaseNotes.trim() !== '' && (
            <div className="mt-1.5 text-[10px] leading-relaxed whitespace-pre-wrap line-clamp-3 text-emerald-800/80 dark:text-emerald-200/80">
              {result.check.releaseNotes.trim()}
            </div>
          )}

          <div className="flex items-center gap-3 mt-2">
            <button
              onClick={() => openRelease(result.check.releaseUrl)}
              disabled={opening}
              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded text-[11px] font-medium bg-emerald-600 text-white hover:bg-emerald-700 transition-colors disabled:opacity-60"
            >
              {opening ? <Loader2 className="size-3 animate-spin" /> : <ExternalLink className="size-3" />}
              Open download page
            </button>
            <button
              onClick={() => skip(result.check.latestVersion)}
              className="text-[10px] text-emerald-800/70 dark:text-emerald-200/70 hover:text-emerald-900 dark:hover:text-emerald-100 underline transition-colors"
            >
              Skip this version
            </button>
          </div>
        </div>
      )}

      <div className="flex items-center gap-2 px-4 py-2 border-t border-border/40">
        <label className="inline-flex items-center gap-2 text-[11px] text-muted-foreground cursor-pointer">
          <input
            type="checkbox"
            checked={settings?.autoCheck ?? false}
            disabled={settings === null}
            onChange={(event) => changeSettings({ autoCheck: event.target.checked })}
            className="accent-blue-500"
          />
          Check for updates when the app starts
        </label>
        <div className="flex-1" />
        {settings?.skippedVersion != null && (
          <button
            onClick={() => changeSettings({ skippedVersion: null })}
            title={`Ask about v${settings.skippedVersion} again`}
            className="inline-flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground transition-colors"
          >
            <RefreshCw className="size-3" />
            Skipping v{settings.skippedVersion}
          </button>
        )}
      </div>
    </div>
  );
}
