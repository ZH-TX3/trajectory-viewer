// ── Config Hub: configuration profiles ───────────────────────────────────
//
// A profile is a named snapshot of which resources are enabled for which
// tools. Two ways to author one: snapshot the current setup, or build it by
// hand in the editor. Applying a profile reconciles the live state to match.

import { useCallback, useEffect, useState } from 'react';
import { Loader2, Pencil, Play, Plus, Save, Trash2 } from 'lucide-react';
import { api } from '../../api';
import type { ApplyReport, McpServer, Profile, ToolInfo } from '../../types';
import { cn } from '../../lib/utils';
import { ProfileEditor } from './ProfileEditor';

interface Notice {
  type: 'ok' | 'error';
  text: string;
}

interface ProfilesSectionProps {
  tools: ToolInfo[];
  resources: Array<{ id: string; name: string; kind: 'skill' | 'agent' }>;
}

export function ProfilesSection({ tools, resources }: ProfilesSectionProps) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [newName, setNewName] = useState('');
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  /** null = closed; '' = new profile; otherwise the name being edited. */
  const [editing, setEditing] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [list, mcp] = await Promise.all([
        api.configHubProfiles(),
        api.configHubMcpList(),
      ]);
      setProfiles(list);
      setMcpServers(mcp);
    } catch (err) {
      setNotice({ type: 'error', text: `Failed to load profiles: ${String(err)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveCurrent = useCallback(async () => {
    const name = newName.trim();
    if (!name) return;
    setBusy('save');
    setNotice(null);
    try {
      await api.configHubSaveProfile(name);
      setNewName('');
      setNotice({ type: 'ok', text: `Saved current setup as “${name}”` });
      await refresh();
    } catch (err) {
      setNotice({ type: 'error', text: String(err) });
    } finally {
      setBusy(null);
    }
  }, [newName, refresh]);

  const apply = useCallback(
    async (name: string) => {
      setBusy(`apply:${name}`);
      setNotice(null);
      try {
        const report = await api.configHubApplyProfile(name);
        setNotice({ type: 'ok', text: describeReport(name, report) });
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  const remove = useCallback(
    async (name: string) => {
      setBusy(`delete:${name}`);
      setNotice(null);
      try {
        await api.configHubDeleteProfile(name);
        setPendingDelete(null);
        await refresh();
      } catch (err) {
        setNotice({ type: 'error', text: String(err) });
      } finally {
        setBusy(null);
      }
    },
    [refresh],
  );

  return (
    <div>
      <div className="text-[11px] text-muted-foreground mb-3">
        A profile records which skills, agents and MCP servers are enabled for which tools. Save
        the current setup, build one by hand, or apply a profile to reconcile every tool to match it.
      </div>

      {/* Save current state / create new */}
      <div className="flex items-center gap-2 mb-3">
        <input
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void saveCurrent();
          }}
          placeholder="Name this setup…"
          className="flex-1 min-w-0 text-xs bg-transparent border border-[hsl(var(--border))] rounded px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-blue-400"
        />
        <button
          onClick={() => void saveCurrent()}
          disabled={busy !== null || !newName.trim()}
          title="Snapshot the current setup under this name"
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded text-[11px] border border-[hsl(var(--border))] hover:bg-muted/40 transition-colors disabled:opacity-50 shrink-0"
        >
          {busy === 'save' ? <Loader2 className="size-3 animate-spin" /> : <Save className="size-3" />}
          Save current
        </button>
        <button
          onClick={() => setEditing('')}
          disabled={busy !== null}
          title="Create a profile by hand"
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded text-[11px] bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50 shrink-0"
        >
          <Plus className="size-3" />
          New
        </button>
      </div>

      {notice && (
        <div
          className={cn(
            'mb-3 px-3 py-2 rounded-lg text-[11px] leading-relaxed break-all',
            notice.type === 'ok'
              ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'
              : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300',
          )}
        >
          {notice.text}
        </div>
      )}

      {/* Profile list */}
      <div className="divide-y divide-[hsl(var(--border))]">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin mr-2 inline" />
            Loading…
          </div>
        ) : profiles.length === 0 ? (
          <div className="p-4 text-xs text-muted-foreground text-center">
            No profiles yet. Set up your tools, then save the state above.
          </div>
        ) : (
          profiles.map((profile) => {
            const resourceCount = Object.keys(profile.resources).length;
            const mcpCount = Object.keys(profile.mcpServers).length;
            return (
              <div key={profile.name} className="px-1 py-2 flex items-center gap-3">
                <div className="flex-1 min-w-0">
                  <div className="text-xs font-medium truncate">{profile.name}</div>
                  <div className="text-[10px] text-muted-foreground">
                    {resourceCount} resource{resourceCount === 1 ? '' : 's'}
                    {mcpCount > 0 && ` · ${mcpCount} MCP`}
                    {profile.createdAt > 0 &&
                      ` · ${new Date(profile.createdAt).toLocaleDateString()}`}
                  </div>
                </div>

                <div className="flex items-center gap-1 shrink-0">
                  <button
                    onClick={() => void apply(profile.name)}
                    disabled={busy !== null}
                    title="Apply this profile"
                    className="inline-flex items-center gap-1 px-2 py-1 rounded text-[10px] border border-[hsl(var(--border))] hover:bg-muted/40 transition-colors disabled:opacity-50"
                  >
                    {busy === `apply:${profile.name}` ? (
                      <Loader2 className="size-3 animate-spin" />
                    ) : (
                      <Play className="size-3" />
                    )}
                    Apply
                  </button>
                  <button
                    onClick={() => setEditing(profile.name)}
                    disabled={busy !== null}
                    title="Edit profile contents"
                    className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-50"
                  >
                    <Pencil className="size-3" />
                  </button>
                  <button
                    onClick={() => setPendingDelete(profile.name)}
                    disabled={busy !== null}
                    title="Delete profile"
                    className="p-1 rounded text-red-500 hover:bg-red-500/10 transition-colors disabled:opacity-50"
                  >
                    <Trash2 className="size-3" />
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>

      {editing !== null && (
        <ProfileEditor
          profileName={editing === '' ? null : editing}
          tools={tools}
          resources={resources}
          mcpServers={mcpServers}
          onSaved={() => {
            setEditing(null);
            setNotice({ type: 'ok', text: 'Profile saved' });
            void refresh();
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {pendingDelete && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
          <div className="w-full max-w-md rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--background))] p-4 shadow-lg">
            <div className="text-sm font-medium mb-2">Delete profile “{pendingDelete}”?</div>
            <p className="text-[11px] text-muted-foreground leading-relaxed mb-3">
              This only removes the saved snapshot; your tools' current setup is unchanged.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPendingDelete(null)}
                disabled={busy !== null}
                className="px-3 py-1 rounded text-xs border border-[hsl(var(--border))] hover:bg-muted/40 transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={() => void remove(pendingDelete)}
                disabled={busy !== null}
                className="px-3 py-1 rounded text-xs bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-60"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** One-line summary of what an apply did. */
function describeReport(name: string, report: ApplyReport): string {
  const parts: string[] = [];
  if (report.enabled > 0) parts.push(`${report.enabled} enabled`);
  if (report.disabled > 0) parts.push(`${report.disabled} disabled`);
  if (parts.length === 0) parts.push('already matched');

  let text = `Applied “${name}”: ${parts.join(', ')}`;
  if (report.skipped.length > 0) {
    text += ` · skipped ${report.skipped.length} unmanaged cop${
      report.skipped.length === 1 ? 'y' : 'ies'
    }`;
  }
  if (report.errors.length > 0) {
    text += ` · ${report.errors.length} error${report.errors.length === 1 ? '' : 's'}: ${report.errors.join('; ')}`;
  }
  return text;
}
