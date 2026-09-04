// ── Backup & Restore Section (cc-switch style) ───────────────────────────
//
// Fixed backups dir (~/.trajectory-viewer/backups/) with:
//   - "Backup Now" for the selected tools
//   - auto-backup interval (hours) + retention count, persisted backend-side
//   - a list of stored backups (name / time / size) with restore & delete
// Restore is merge+overwrite after an automatic safety copy.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Download, Loader2, RotateCw, RotateCcw, Trash2, RefreshCw, Check } from 'lucide-react';
import { api } from '../api';
import type { BackupEntry, BackupSettings, ProviderSessionInfo } from '../types';
import { cn } from '../lib/utils';

const PROVIDER_LABELS: Record<string, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  dsh: 'DSH',
  opencode: 'OpenCode',
};

const INTERVAL_OPTIONS = [
  { value: 0, label: 'Disabled' },
  { value: 6, label: 'Every 6 hours' },
  { value: 12, label: 'Every 12 hours' },
  { value: 24, label: 'Every 24 hours (daily)' },
  { value: 48, label: 'Every 48 hours' },
  { value: 168, label: 'Every 168 hours (weekly)' },
];

const RETAIN_OPTIONS = [3, 5, 10, 15, 20, 30, 50];

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function backupTimeName(createdAt: number): string {
  return new Date(createdAt).toLocaleString();
}

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

export function BackupSection() {
  const [providers, setProviders] = useState<ProviderSessionInfo[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [settings, setSettings] = useState<BackupSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [working, setWorking] = useState<'backup' | 'restore' | 'delete' | null>(null);
  const [confirm, setConfirm] = useState<{ kind: 'restore' | 'delete'; filename: string } | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [prov, list] = await Promise.all([
        api.listProviderSessionInfo(),
        api.listSessionBackups(),
      ]);
      setProviders(prov);
      setBackups(list);
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to load backups: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    api.getBackupSettings().then(setSettings).catch(() => setSettings(null));
    void refresh();
  }, [refresh]);

  // Default-select every found tool once.
  useEffect(() => {
    if (selected.size === 0 && providers.length > 0) {
      setSelected(new Set(providers.map((p) => p.providerId)));
    }
  }, [providers, selected.size]);

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const doBackup = useCallback(async () => {
    const chosen = providers.map((p) => p.providerId).filter((id) => selected.has(id));
    if (chosen.length === 0) {
      setNotice({ type: 'error', text: 'Select at least one tool to back up.' });
      return;
    }
    setWorking('backup');
    setNotice(null);
    try {
      const res = await api.backupNow(chosen);
      setNotice({
        type: 'ok',
        text: `Backed up ${res.providers.length} tool(s) → ${formatBytes(res.sizeBytes)}`,
      });
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: `Backup failed: ${String(err)}` });
    } finally {
      setWorking(null);
    }
  }, [providers, selected, refresh]);

  const changeSettings = useCallback(
    async (patch: Partial<BackupSettings>) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      setSettings(next);
      try {
        await api.setBackupSettings(next);
        // Brief "saved" flash so edits don't look silently unpersisted.
        setSavedFlash(true);
        if (savedTimer.current) clearTimeout(savedTimer.current);
        savedTimer.current = setTimeout(() => setSavedFlash(false), 1500);
      } catch (err) {
        setNotice({ type: 'error', text: `Failed to save backup settings: ${String(err)}` });
      }
    },
    [settings],
  );

  const doDelete = useCallback(async (filename: string) => {
    setWorking('delete');
    setNotice(null);
    try {
      await api.deleteSessionBackup(filename);
      setNotice({ type: 'ok', text: `Deleted ${filename}` });
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: `Delete failed: ${String(err)}` });
    } finally {
      setWorking(null);
      setConfirm(null);
    }
  }, [refresh]);

  const doRestore = useCallback(async (filename: string) => {
    setWorking('restore');
    setNotice(null);
    try {
      const res = await api.restoreSessionBackup(filename);
      const parts = res.providers
        .filter((s) => s.fileCount > 0)
        .map((s) => `${PROVIDER_LABELS[s.providerId] ?? s.providerId} (${s.fileCount} files)`)
        .join(', ');
      const safety = res.safetyBackupDir ? ` Safety copy: ${res.safetyBackupDir}` : '';
      const warn = res.warnings.length ? ` Warnings: ${res.warnings.join('; ')}` : '';
      setNotice({ type: 'ok', text: `Restored: ${parts || 'nothing'}.${safety}${warn}` });
    } catch (err) {
      setNotice({ type: 'error', text: `Restore failed: ${String(err)}` });
    } finally {
      setWorking(null);
      setConfirm(null);
    }
  }, []);

  const selectedCount = useMemo(
    () => providers.filter((p) => selected.has(p.providerId)).length,
    [providers, selected],
  );

  const settingsBlock = settings !== null && (
    <div className="relative grid grid-cols-2 gap-3 px-4 py-3 border-t border-border/40">
      {savedFlash && (
        <span className="absolute right-3 top-2 inline-flex items-center gap-0.5 text-[10px] text-emerald-600 dark:text-emerald-400">
          <Check className="size-3" /> Saved
        </span>
      )}
      <label className="min-w-0">
        <span className="block text-[10px] text-muted-foreground mb-1">Auto backup interval</span>
        <select
          value={settings.intervalHours}
          onChange={(e) => changeSettings({ intervalHours: Number(e.target.value) })}
          className="w-full text-xs bg-muted/40 border border-border/40 rounded px-2 py-1 outline-none"
        >
          {INTERVAL_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </label>
      <label className="min-w-0">
        <span className="block text-[10px] text-muted-foreground mb-1">Keep backups</span>
        <select
          value={settings.retainCount}
          onChange={(e) => changeSettings({ retainCount: Number(e.target.value) })}
          className="w-full text-xs bg-muted/40 border border-border/40 rounded px-2 py-1 outline-none"
        >
          {RETAIN_OPTIONS.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </label>
    </div>
  );

  return (
    <div>
      {/* Provider selection */}
      <div className="divide-y divide-border/40">
        {providers.map((p) => {
          const checked = selected.has(p.providerId);
          return (
            <div key={p.providerId} className="px-4 py-2 flex items-center gap-3">
              <input
                type="checkbox"
                checked={checked}
                onChange={() => toggle(p.providerId)}
                className="accent-blue-500"
              />
              <div className="flex-1 min-w-0">
                <div className="text-xs font-medium">{PROVIDER_LABELS[p.providerId] ?? p.providerId}</div>
                <div className="text-[10px] text-muted-foreground truncate">
                  {p.exists ? `${p.fileCount} files · ${formatBytes(p.totalBytes)}` : 'not found on this machine'}
                  <span className="opacity-60"> — {p.path}</span>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Backup Now */}
      <div className="flex items-center gap-2 px-4 py-3 border-t border-border/40">
        <button
          onClick={doBackup}
          disabled={working !== null}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-60"
        >
          {working === 'backup' ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
          {working === 'backup' ? 'Backing up…' : 'Backup Now'}
        </button>
        <span className="text-[10px] text-muted-foreground">{selectedCount} tool(s) selected</span>
        <div className="flex-1" />
        <button
          onClick={refresh}
          disabled={working !== null}
          title="Refresh backup list"
          className="p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-60"
        >
          <RotateCw className="size-3.5" />
        </button>
      </div>

      {/* Auto settings */}
      {settingsBlock}

      {/* Backup list */}
      <div className="border-t border-border/40 divide-y divide-border/40">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin mr-2 inline" />Loading backups…
          </div>
        ) : backups.length === 0 ? (
          <div className="p-4 text-xs text-muted-foreground text-center">
            No backups yet — hit “Backup Now” to create one.
          </div>
        ) : (
          backups.map((b) => (
            <div key={b.filename} className="px-4 py-2 flex items-center gap-3">
              <RefreshCw className="size-3 text-muted-foreground/50 shrink-0" />
              <div className="flex-1 min-w-0">
                <div className="text-xs truncate">{backupTimeName(b.createdAt)}</div>
                <div className="text-[10px] text-muted-foreground truncate font-mono">{b.filename}</div>
              </div>
              <div className="shrink-0 text-[10px] text-muted-foreground font-mono">{formatBytes(b.sizeBytes)}</div>
              <div className="shrink-0 flex items-center gap-0.5">
                <button
                  onClick={() => setConfirm({ kind: 'restore', filename: b.filename })}
                  disabled={working !== null}
                  title="Restore this backup"
                  className="p-1 rounded text-blue-600 dark:text-blue-400 hover:bg-blue-500/10 transition-colors disabled:opacity-50"
                >
                  <RotateCcw className="size-3" />
                </button>
                <button
                  onClick={() => setConfirm({ kind: 'delete', filename: b.filename })}
                  disabled={working !== null}
                  title="Delete this backup"
                  className="p-1 rounded text-red-500 hover:text-red-600 hover:bg-red-500/10 transition-colors disabled:opacity-50"
                >
                  <Trash2 className="size-3" />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* Confirm dialog */}
      {confirm && (
        <div className="mx-4 my-3 rounded-lg border border-amber-300/60 bg-amber-50 dark:bg-amber-900/15 p-3">
          <div className="text-[11px] text-amber-800 dark:text-amber-200 leading-relaxed">
            <div className="font-medium mb-1">
              {confirm.kind === 'restore' ? 'Restore this backup?' : 'Delete this backup?'}
            </div>
            {confirm.kind === 'restore' ? (
              <div className="opacity-80">
                {confirm.filename} — files in the archive overwrite same-named session files
                (unrelated files are kept). Current state is first copied to a safety backup
                under <span className="font-mono">~/.trajectory-viewer/backups/</span>.
              </div>
            ) : (
              <div className="opacity-80">{confirm.filename} will be permanently removed.</div>
            )}
          </div>
          <div className="flex justify-end gap-2 mt-2">
            <button
              onClick={() => setConfirm(null)}
              disabled={working !== null}
              className="px-3 py-1 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={() =>
                confirm.kind === 'restore' ? doRestore(confirm.filename) : doDelete(confirm.filename)
              }
              disabled={working !== null}
              className={cn(
                'px-3 py-1 rounded text-xs text-white transition-colors disabled:opacity-60',
                confirm.kind === 'restore' ? 'bg-amber-500 hover:bg-amber-600' : 'bg-red-600 hover:bg-red-700',
              )}
            >
              {working === 'restore' && confirm.kind === 'restore'
                ? 'Restoring…'
                : working === 'delete' && confirm.kind === 'delete'
                  ? 'Deleting…'
                  : confirm.kind === 'restore'
                    ? 'Restore'
                    : 'Delete'}
            </button>
          </div>
        </div>
      )}

      {/* Result notice */}
      {notice && (
        <div
          className={cn(
            'mx-4 mb-3 px-3 py-2 rounded-lg text-[11px] leading-relaxed break-all',
            notice.type === 'ok'
              ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
              : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
          )}
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}