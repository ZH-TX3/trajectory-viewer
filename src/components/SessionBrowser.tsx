// ── Session Browser ──────────────────────────────────────────────────────
//
// Left sidebar: sessions grouped by project directory hierarchy.
// Right panel: Messages/Trajectory tabs.

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import type { FC } from 'react';
import { api } from '../api';
import { TrajectoryView } from './TrajectoryView';
import { TrajectoryErrorBoundary } from './TrajectoryErrorBoundary';
import { SidebarSearch } from './SidebarSearch';
import type { SessionMeta, SessionMessage, TrajectoryData } from '../types';
import {
  ClaudeMark, CodexMark, DshMark, OpenCodeLogoDarkAware,
} from './icons/BrandIcons';
import {
  MessageSquare, GitBranch, Clock, FileText, Loader2,
  ChevronRight, ChevronDown, Folder, FolderOpen, GripVertical,
  Pencil, Trash2, Copy, Check, RotateCw, Download,
} from 'lucide-react';

// Custom session titles are persisted locally, keyed by provider::sessionId.
const CUSTOM_TITLES_KEY = 'trajectory-viewer.custom-titles.v1';
function readCustomTitles(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(CUSTOM_TITLES_KEY) ?? '{}');
  } catch {
    return {};
  }
}
function writeCustomTitles(titles: Record<string, string>) {
  localStorage.setItem(CUSTOM_TITLES_KEY, JSON.stringify(titles));
}

/** Resume command text for a session (claude / codex); empty for others. */
function resumeCommandFor(session: SessionMeta | null): string {
  if (!session) return '';
  const id = session.sessionId;
  if (session.providerId === 'claude') return session.resumeCommand ?? `claude --resume ${id}`;
  if (session.providerId === 'codex') return session.resumeCommand ?? `codex resume ${id}`;
  return session.resumeCommand ?? '';
}

// Filter chips: same shape as the settings toggle, rendered from one array.
// Icons are the shared brand marks; each chip tints the active state.
const PROVIDER_CHIPS: Array<{
  id: Exclude<ProviderFilter, 'all'>;
  label: string;
  icon: FC<{ className?: string }>;
  active: string;
}> = [
  { id: 'claude', label: 'Claude', icon: ClaudeMark, active: 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300 shadow-sm' },
  { id: 'codex', label: 'Codex', icon: CodexMark, active: 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 shadow-sm' },
  { id: 'dsh', label: 'DSH', icon: DshMark, active: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 shadow-sm' },
  { id: 'opencode', label: 'OpenCode', icon: OpenCodeLogoDarkAware, active: 'bg-sky-100 dark:bg-sky-900/30 text-sky-700 dark:text-sky-300 shadow-sm' },
];

interface SessionBrowserProps {
  onOpenFile: () => void;
  enabledProviders: ReadonlySet<string>;
  /** Provider display order (from settings drag-reorder). */
  providerOrder: readonly string[];
}

type Tab = 'messages' | 'trajectory';
type ProviderFilter = 'all' | 'claude' | 'codex' | 'dsh' | 'opencode';

// ── Group entry ──────────────────────────────────────────────────────────

interface GroupedSessions {
  groupName: string;
  providerId: string;
  sessions: SessionMeta[];
  /** Storage directories backing this group (for bulk delete). */
  storageDirs: string[];
}

export function SessionBrowser({ onOpenFile, enabledProviders, providerOrder = ['claude', 'codex', 'dsh', 'opencode'] }: SessionBrowserProps) {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('messages');
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>(() => {
    // Default to the first enabled provider tab so we don't scan/load every
    // tool's sessions on startup; fall back to 'all' only if none is enabled.
    const first = providerOrder.find((id) => enabledProviders.has(id));
    return (first ?? 'all') as ProviderFilter;
  });
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  const [sidebarWidth, setSidebarWidth] = useState(280);
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [trajectoryData, setTrajectoryData] = useState<TrajectoryData | null>(null);
  const [trajectoryLoading, setTrajectoryLoading] = useState(false);
  const dragRef = useRef<HTMLDivElement>(null);
  const isDragging = useRef(false);
  const [customTitles, setCustomTitles] = useState<Record<string, string>>(readCustomTitles);
  const [renameFor, setRenameFor] = useState<string | null>(null);
  const [renameText, setRenameText] = useState('');
  const [pendingDeleteSession, setPendingDeleteSession] = useState<SessionMeta | null>(null);
  const [pendingDeleteGroup, setPendingDeleteGroup] = useState<GroupedSessions | null>(null);
  const [deletingGroup, setDeletingGroup] = useState(false);
  const [notice, setNotice] = useState<{ type: 'ok' | 'error'; text: string } | null>(null);
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showNotice = useCallback((type: 'ok' | 'error', text: string) => {
    setNotice({ type, text });
    if (noticeTimer.current) clearTimeout(noticeTimer.current);
    noticeTimer.current = setTimeout(() => setNotice(null), 3200);
  }, []);
  const [copied, setCopied] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportMenuOpen, setExportMenuOpen] = useState(false);
  // Timestamp of the matched message to focus after opening a cross-session
  // result — the table scrolls to and opens that record, instead of drowning
  // the whole session in highlights.
  const [focusTs, setFocusTs] = useState<number | null>(null);
  // Cross-session search query; owned here so ESC can clear it and return the
  // sidebar to the normal session list.
  const [searchQuery, setSearchQuery] = useState('');

  // ESC cancels the cross-session search and any seeded in-session highlight,
  // returning the sidebar to the normal session list. TrajectoryView has its
  // own ESC for the timeline/table selection.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setSearchQuery('');
        setFocusTs(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // ── Sidebar resize handling ────────────────────────────────────────
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const newWidth = Math.max(180, Math.min(600, e.clientX));
      setSidebarWidth(newWidth);
    };
    const handleMouseUp = () => {
      if (isDragging.current) {
        isDragging.current = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      }
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  // Load sessions on mount
  useEffect(() => {
    api.listSessions()
      .then((list) => {
        setSessions(list);
        // Auto-expand + select within the initially active provider (not 'all',
        // so we don't hop across tools before the real filter/group runs).
        const groups = groupByProject(list, providerFilter, providerOrder);
        if (groups.length > 0) {
          setExpandedGroups(new Set([groups[0].groupName]));
          setSelectedKey(`${groups[0].sessions[0].providerId}::${groups[0].sessions[0].sessionId}`);
        }
      })
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  // Filter sessions: first by enabled tools (settings), then by the filter chip.
  const filteredSessions = useMemo(() => {
    const list = sessions.filter((s) => enabledProviders.has(s.providerId));
    if (providerFilter === 'all') return list;
    return list.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter, enabledProviders]);

  // Group by project directory
  const groupedSessions = useMemo(() => {
    return groupByProject(filteredSessions, providerFilter, providerOrder);
  }, [filteredSessions, providerFilter, providerOrder]);

  // Auto-select when filter changes
  useEffect(() => {
    if (groupedSessions.length === 0) {
      setSelectedKey(null);
      return;
    }
    // Ensure first group is expanded
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (groupedSessions.length > 0) {
        next.add(groupedSessions[0].groupName);
      }
      return next;
    });
    // Check if selected still exists
    const exists = filteredSessions.some((s) => `${s.providerId}::${s.sessionId}` === selectedKey);
    if (!exists) {
      setSelectedKey(`${groupedSessions[0].sessions[0].providerId}::${groupedSessions[0].sessions[0].sessionId}`);
    }
  }, [groupedSessions, filteredSessions, selectedKey]);

  const selectedSession = useMemo(() => {
    if (!selectedKey) return null;
    return sessions.find((s) => `${s.providerId}::${s.sessionId}` === selectedKey) ?? null;
  }, [sessions, selectedKey]);

  // Load messages for the selected session
  const loadMessages = useCallback(async () => {
    const sp = selectedSession?.sourcePath;
    if (!sp) return;
    setMessagesLoading(true);
    try {
      const list = await api.getSessionMessages(selectedSession.providerId, sp);
      setMessages(list);
    } catch (err) {
      console.error('load messages failed', err);
    } finally {
      setMessagesLoading(false);
    }
  }, [selectedSession?.providerId, selectedSession?.sourcePath]);

  // Load trajectory for the selected session
  const loadTrajectory = useCallback(async () => {
    const sp = selectedSession?.sourcePath;
    if (!sp) return;
    setTrajectoryLoading(true);
    try {
      const data = await api.getSessionTrajectory(selectedSession.providerId, sp);
      setTrajectoryData(data);
    } catch (err) {
      console.error('load trajectory failed', err);
    } finally {
      setTrajectoryLoading(false);
    }
  }, [selectedSession?.providerId, selectedSession?.sourcePath]);

  // Load messages when the selected session changes
  useEffect(() => {
    setMessages([]);
    loadMessages();
  }, [loadMessages]);

  // Load trajectory when the tab switches to trajectory
  useEffect(() => {
    if (activeTab !== 'trajectory') return;
    loadTrajectory();
  }, [activeTab, loadTrajectory]);

  // Refresh the session list periodically so new / updated sessions surface.
  useEffect(() => {
    const timer = setInterval(() => {
      api.listSessions().then(setSessions).catch(console.error);
    }, 30000);
    return () => clearInterval(timer);
  }, []);

  // Watch the selected session file: when its mtime changes, reload the tab.
  const mtimeRef = useRef(0);
  useEffect(() => {
    const sp = selectedSession?.sourcePath;
    mtimeRef.current = 0;
    if (!sp) return;
    const tick = async () => {
      try {
        const m = await api.getSessionMtime(sp);
        if (mtimeRef.current !== 0 && m !== mtimeRef.current) {
          if (activeTab === 'trajectory') loadTrajectory();
          else loadMessages();
        }
        mtimeRef.current = m;
      } catch {
        // file may be gone — ignore
      }
    };
    void tick();
    const timer = setInterval(tick, 5000);
    return () => clearInterval(timer);
  }, [selectedSession?.sourcePath, activeTab, loadMessages, loadTrajectory]);

  // Manual refresh: session list + current tab.
  const refreshAll = useCallback(async () => {
    try {
      setSessions(await api.listSessions());
    } catch (err) {
      console.error('refresh list failed', err);
    }
    if (selectedSession?.sourcePath) {
      if (activeTab === 'trajectory') await loadTrajectory();
      else await loadMessages();
    }
  }, [selectedSession?.sourcePath, activeTab, loadMessages, loadTrajectory]);

  const handleSelectSession = useCallback((key: string) => {
    setSelectedKey(key);
    // Keep the current tab (Messages / Trajectory) across session switches.
    setTrajectoryData(null);
    setFocusTs(null);
  }, []);

  /**
   * Jump to a cross-session search result.
   *
   * Order matters: the auto-select effect below resets `selectedKey` when the
   * target isn't in the currently filtered list, so the provider filter and
   * group expansion must be updated in the same batch as the selection.
   */
  const handleOpenSearchResult = useCallback(
    (group: { provider: string; sessionId: string; sessionKey: string }, focusTs: number | null) => {
      setProviderFilter(group.provider as ProviderFilter);
      setExpandedGroups((prev) => {
        const next = new Set(prev);
        // Expand whichever project group contains the target session.
        const target = sessions.find(
          (s) => `${s.providerId}::${s.sessionId}` === group.sessionKey,
        );
        if (target) next.add(groupNameFor(target));
        return next;
      });
      setSelectedKey(group.sessionKey);
      setTrajectoryData(null);
      // Land on Trajectory and jump to the exact matched message.
      setActiveTab('trajectory');
      setFocusTs(focusTs);
    },
    [sessions],
  );

  const toggleGroup = useCallback((name: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const startRename = useCallback((key: string, session: SessionMeta) => {
    setRenameText(sessionTitle(session));
    setRenameFor(key);
  }, [sessionTitle]);

  const commitRename = useCallback(() => {
    if (renameFor === null) return;
    const key = renameFor;
    setCustomTitles((prev) => {
      const next = { ...prev };
      if (renameText.trim()) next[key] = renameText.trim();
      else delete next[key];
      writeCustomTitles(next);
      return next;
    });
    setRenameFor(null);
  }, [renameFor, renameText]);

  const deleteSessionHandler = useCallback(async (session: SessionMeta) => {
    if (!session.sourcePath) return;
    setPendingDeleteSession(null);
    try {
      await api.deleteSession(session.sourcePath);
      const key = `${session.providerId}::${session.sessionId}`;
      setSessions((prev) => prev.filter((s) => `${s.providerId}::${s.sessionId}` !== key));
      setSelectedKey((prev) => (prev === key ? null : prev));
      showNotice('ok', 'Moved to trash — restore any time in Settings → Advanced');
    } catch (err) {
      showNotice('error', `Delete failed: ${String(err)}`);
    }
  }, [showNotice]);

  // Bulk-delete every conversation in a project group's storage dirs.
  const confirmDeleteGroup = useCallback(async () => {
    const group = pendingDeleteGroup;
    if (!group) return;
    setDeletingGroup(true);
    let total = 0;
    for (const dir of group.storageDirs) {
      try {
        total += await api.deleteSessionsInDir(dir);
      } catch (err) {
        showNotice('error', `Delete failed: ${String(err)}`);
      }
    }
    try {
      setSessions(await api.listSessions());
    } catch (err) {
      console.error('refresh after group delete failed', err);
    }
    setDeletingGroup(false);
    setPendingDeleteGroup(null);
    showNotice('ok', total > 0 ? `Moved ${total} session(s) to trash` : 'Nothing to delete');
  }, [pendingDeleteGroup, showNotice]);

  const copyResume = useCallback(async () => {
    if (!selectedSession) return;
    const cmd = resumeCommandFor(selectedSession);
    if (!cmd) return;
    try {
      await navigator.clipboard.writeText(cmd);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      // clipboard unavailable — ignore
    }
  }, [selectedSession]);

  function sessionTitle(s: SessionMeta) {
    return (
      customTitles[`${s.providerId}::${s.sessionId}`]
      ?? s.title
      ?? s.sessionId.slice(0, 8) + '…'
    );
  }

  /** Export the selected session as Markdown or JSONL via a save dialog. */
  const handleExport = useCallback(
    async (format: 'md' | 'jsonl') => {
      setExportMenuOpen(false);
      const session = selectedSession;
      if (!session?.sourcePath) return;
      const title = sessionTitle(session);
      // Keep the filename filesystem-safe.
      const safeTitle = title.replace(/[\\/:*?"<>|]/g, '_').slice(0, 60).trim() || session.sessionId;
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const target = await save({
          title: `Export session as ${format.toUpperCase()}`,
          defaultPath: `${safeTitle}.${format}`,
          filters: [
            format === 'md'
              ? { name: 'Markdown', extensions: ['md'] }
              : { name: 'JSON Lines', extensions: ['jsonl'] },
          ],
        });
        if (!target) return;
        setExporting(true);
        const count = await api.exportSession(
          session.providerId,
          session.sourcePath,
          title,
          format,
          target,
        );
        showNotice('ok', `Exported ${count} record(s) to ${target}`);
      } catch (err) {
        showNotice('error', `Export failed: ${String(err)}`);
      } finally {
        setExporting(false);
      }
    },
    [selectedSession, customTitles, showNotice],
  );

  const formatTime = (ts: number | null | undefined) => {
    if (!ts) return '';
    return new Date(ts).toLocaleString();
  };

  const providerIcon = (id: string) => {
    return id === 'claude' ? 'text-violet-600 dark:text-violet-400'
      : id === 'dsh' ? 'text-amber-600 dark:text-amber-400'
      : id === 'opencode' ? 'text-sky-600 dark:text-sky-400'
      : 'text-emerald-600 dark:text-emerald-400';
  };

  const providerGlyph = (id: string, size = 'size-3.5') => {
    if (id === 'claude') return <ClaudeMark className={size} />;
    if (id === 'dsh') return <DshMark className={size} />;
    if (id === 'opencode') return <OpenCodeLogoDarkAware className={size} />;
    return <CodexMark className={size} />;
  };

  // Filter chips follow the settings-defined display order.
  const orderedChips = useMemo(() => {
    const byId = new Map<string, (typeof PROVIDER_CHIPS)[number]>(
      PROVIDER_CHIPS.map((chip) => [chip.id, chip]),
    );
    return providerOrder
      .map((id) => byId.get(id))
      .filter((chip): chip is (typeof PROVIDER_CHIPS)[number] => chip !== undefined);
  }, [providerOrder]);

  const claudeCount = sessions.filter((s) => s.providerId === 'claude').length;
  const codexCount = sessions.filter((s) => s.providerId === 'codex').length;
  const dshCount = sessions.filter((s) => s.providerId === 'dsh').length;
  const opencodeCount = sessions.filter((s) => s.providerId === 'opencode').length;
  const chipIdCount = (id: string) =>
    id === 'claude' ? claudeCount : id === 'codex' ? codexCount : id === 'dsh' ? dshCount : opencodeCount;

  // The provider filter bar scrolls horizontally when it overflows; remap the
  // vertical wheel into horizontal scrolling there (like a scroll-tab bar).
  const filterBarRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = filterBarRef.current;
    if (el === null) return;
    const onWheel = (event: WheelEvent) => {
      if (el.scrollWidth > el.clientWidth) {
        event.preventDefault();
        el.scrollLeft += event.deltaY;
      }
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  return (
    <div className="flex h-full min-h-0">
      {/* Left sidebar */}
      <aside
        className="border-r border-border/40 bg-muted/20 flex flex-col shrink-0 overflow-hidden"
        style={{ width: sidebarWidth }}
      >
        {/* Provider filter icons */}
        <div className="relative border-b border-border/40">
          <div ref={filterBarRef} className="flex items-center gap-1 px-2 py-1.5 overflow-x-auto pr-9">
            {orderedChips.filter((chip) => enabledProviders.has(chip.id)).map((chip) => (
              <button
                key={chip.id}
                onClick={() => setProviderFilter(chip.id)}
                className={`flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium transition-colors ${
                  providerFilter === chip.id
                    ? chip.active
                    : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
                }`}
              >
                <chip.icon className="size-3.5" />
                <span>{chip.label}</span>
                <span className="text-[10px] opacity-60">{chipIdCount(chip.id)}</span>
              </button>
            ))}
          </div>
          {/* Floating refresh — a sibling of the scroll container, so it is
              pinned to the visible right edge and never scrolls with the chips. */}
          <button
            onClick={refreshAll}
            title="Refresh sessions"
            className="absolute right-1 top-1/2 -translate-y-1/2 p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 bg-muted/80 shadow-sm backdrop-blur-sm transition-colors"
          >
            <RotateCw className="size-3.5" />
          </button>
        </div>

        {/* Session list — replaced by grouped search results while a query is active */}
        <SidebarSearch
          providers={providerFilter === 'all' ? [] : [providerFilter]}
          query={searchQuery}
          onQueryChange={setSearchQuery}
          onOpenResult={handleOpenSearchResult}
        >
        {/* Grouped session list */}
        <div className="flex-1 overflow-auto">
          {loading ? (
            <div className="flex items-center justify-center h-20 text-xs text-muted-foreground">
              <Loader2 className="size-3 animate-spin mr-2" />Loading sessions…
            </div>
          ) : groupedSessions.length === 0 ? (
            <div className="p-4 text-xs text-muted-foreground text-center">
              <p className="mb-2">No sessions found</p>
              <p className="text-[10px]">Check ~/.claude/projects/ and ~/.codex/sessions/</p>
            </div>
          ) : (
            <div className="py-1">
              {groupedSessions.map((group) => {
                const isExpanded = expandedGroups.has(group.groupName);
                return (
                  <div key={group.groupName}>
                    {/* Group header */}
                    <div
                      onClick={() => toggleGroup(group.groupName)}
                      className="group relative flex items-center gap-1.5 px-2 py-1.5 cursor-pointer hover:bg-muted/40 transition-colors text-[11px] font-medium text-muted-foreground"
                    >
                      {isExpanded ? <ChevronDown className="size-3 shrink-0" /> : <ChevronRight className="size-3 shrink-0" />}
                      {isExpanded ? <FolderOpen className="size-3.5 shrink-0" /> : <Folder className="size-3.5 shrink-0" />}
                      <span className="truncate">{group.groupName}</span>
                      <span className="text-[10px] opacity-60 ml-auto">{group.sessions.length}</span>

                      {/* Bulk-delete: remove all conversations in this project dir */}
                      <div className="absolute right-4 top-0 bottom-0 opacity-0 group-hover:opacity-100 transition-opacity flex items-center">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setPendingDeleteGroup(group);
                          }}
                          title="Delete all conversations in this project"
                          className="p-1 rounded text-red-500 hover:text-red-600 hover:bg-red-500/10 transition-colors"
                        >
                          <Trash2 className="size-3" />
                        </button>
                      </div>
                    </div>

                    {/* Sessions in group */}
                    {isExpanded && group.sessions.map((session) => {
                      const key = `${session.providerId}::${session.sessionId}`;
                      const isSelected = key === selectedKey;
                      return (
                        <div
                          key={key}
                          onClick={() => handleSelectSession(key)}
                          className={`
                            relative group flex items-start gap-2 pl-7 pr-6 py-2 cursor-pointer transition-all
                            ${isSelected
                              ? 'bg-blue-100 dark:bg-blue-900/50 border-l-[3px] border-blue-500 dark:border-blue-400 font-medium text-blue-900 dark:text-blue-100'
                              : 'hover:bg-muted/60 border-l-2 border-transparent'
                            }
                          `}
                        >
                          {/* Provider icon */}
                          <div className={`shrink-0 mt-0.5 ${providerIcon(session.providerId)}`}>
                            {providerGlyph(session.providerId)}
                          </div>

                          <div className="flex-1 min-w-0">
                            {renameFor === key ? (
                              <input
                                value={renameText}
                                onChange={(e) => setRenameText(e.target.value)}
                                onBlur={commitRename}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') commitRename();
                                  else if (e.key === 'Escape') setRenameFor(null);
                                }}
                                autoFocus
                                placeholder="Session title"
                                className="w-full text-xs bg-muted/40 rounded px-1 py-0.5 outline-none border border-border/40"
                              />
                            ) : (
                              <div className="text-xs font-medium truncate">{sessionTitle(session)}</div>
                            )}
                            {session.summary && (
                              <div className="text-[10px] text-muted-foreground/60 truncate mt-0.5">
                                {session.summary}
                              </div>
                            )}
                            <div className="text-[10px] text-muted-foreground/60 flex items-center gap-1 mt-0.5">
                              <Clock className="size-2.5 shrink-0" />
                              <span className="truncate">{formatTime(session.lastActiveAt ?? session.createdAt)}</span>
                            </div>
                          </div>

                          {/* Row actions (hover) */}
                          <div className="absolute right-1 top-1 opacity-0 group-hover:opacity-100 transition-opacity flex items-center gap-0.5">
                            <button
                              onClick={(e) => { e.stopPropagation(); startRename(key, session); }}
                              title="Rename"
                              className="p-1 rounded hover:bg-muted/60 text-muted-foreground hover:text-foreground"
                            >
                              <Pencil className="size-3" />
                            </button>
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                setPendingDeleteSession(session);
                              }}
                              title="Delete this conversation"
                              className="p-1 rounded text-red-500 hover:text-red-600 hover:bg-red-500/10 transition-colors"
                            >
                              <Trash2 className="size-3" />
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        </SidebarSearch>

        {/* Open file button */}
        <div className="p-2 border-t border-border/40">
          <button
            onClick={onOpenFile}
            className="w-full flex items-center gap-1.5 px-2 py-1.5 rounded text-[11px] text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors"
          >
            <FileText className="size-3.5" />
            Open trajectory file…
          </button>
        </div>
      </aside>

      {/* Drag handle */}
      <div
        ref={dragRef}
        onMouseDown={handleMouseDown}
        className="w-1.5 shrink-0 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 transition-colors relative group"
      >
        <div className="absolute inset-y-0 -left-1 -right-1" />
        <GripVertical className="absolute top-1/2 -translate-y-1/2 -left-[3px] size-3 text-muted-foreground/0 group-hover:text-muted-foreground/50 transition-colors" />
      </div>

      {/* Right panel */}
      <main className="flex-1 flex flex-col min-w-0">
        {!selectedSession ? (
          <div className="flex-1 flex items-center justify-center text-xs text-muted-foreground">
            Select a session to view
          </div>
        ) : (
          <>
            {/* Session header */}
            <div className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-background/95">
              <div className={`${providerIcon(selectedSession.providerId)}`}>
                {providerGlyph(selectedSession.providerId, 'size-4')}
              </div>
              <span className="text-xs font-medium truncate">{sessionTitle(selectedSession)}</span>

              <div className="flex ml-auto gap-1">
                {/* Export menu */}
                <div className="relative">
                  <button
                    onClick={() => setExportMenuOpen((v) => !v)}
                    disabled={exporting}
                    title="Export this session"
                    className="flex items-center gap-1 px-2 py-1.5 rounded-md text-[11px] text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors disabled:opacity-60"
                  >
                    {exporting ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
                  </button>
                  {exportMenuOpen && (
                    <>
                      <div className="fixed inset-0 z-40" onClick={() => setExportMenuOpen(false)} />
                      <div className="absolute right-0 top-full mt-1 z-50 w-40 rounded-lg border border-border/40 bg-white dark:bg-gray-900 shadow-lg overflow-hidden">
                        <button
                          onClick={() => handleExport('md')}
                          className="w-full text-left px-3 py-2 text-[11px] hover:bg-muted/40 transition-colors"
                        >
                          Markdown transcript
                        </button>
                        <button
                          onClick={() => handleExport('jsonl')}
                          className="w-full text-left px-3 py-2 text-[11px] hover:bg-muted/40 transition-colors border-t border-border/40"
                        >
                          JSONL events
                        </button>
                      </div>
                    </>
                  )}
                </div>

                <button
                  onClick={() => setActiveTab('messages')}
                  className={`flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] transition-all ${
                    activeTab === 'messages'
                      ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 font-medium shadow-sm ring-1 ring-blue-300 dark:ring-blue-700'
                      : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
                  }`}
                >
                  <MessageSquare className="size-3.5" />
                  Messages
                </button>
                <button
                  onClick={() => setActiveTab('trajectory')}
                  className={`flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] transition-all ${
                    activeTab === 'trajectory'
                      ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 font-medium shadow-sm ring-1 ring-blue-300 dark:ring-blue-700'
                      : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
                  }`}
                >
                  <GitBranch className="size-3.5" />
                  Trajectory
                </button>
              </div>
            </div>

            {/* Tab content */}
            {activeTab === 'messages' && (
              <div className="flex-1 flex flex-col min-h-0">
                {/* Resume command header (fixed) */}
                {selectedSession && resumeCommandFor(selectedSession) && (
                  <div className="flex items-center gap-1.5 px-4 py-1.5 border-b border-border/40 bg-background/95 backdrop-blur-sm shrink-0">
                    <span className="text-[10px] font-mono text-foreground/90 truncate select-all">
                      {resumeCommandFor(selectedSession)}
                    </span>
                    <button
                      onClick={copyResume}
                      title="Copy resume command"
                      className="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
                    >
                      {copied ? <Check className="size-3 text-emerald-500" /> : <Copy className="size-3" />}
                      {copied ? 'Copied' : 'Copy'}
                    </button>
                  </div>
                )}
                <div className="flex-1 overflow-auto p-4 space-y-3">
                {messagesLoading ? (
                  <div className="flex items-center justify-center h-20 text-xs text-muted-foreground">
                    <Loader2 className="size-3 animate-spin mr-2" />Loading messages…
                  </div>
                ) : messages.length === 0 ? (
                  <div className="text-xs text-muted-foreground text-center pt-8">No messages</div>
                ) : (
                  messages.map((msg, i) => {
                    // Injected workspace/system reminders are user-role payloads;
                    // surface them as "system" like DSH does.
                    const role =
                      msg.role === 'user' && msg.content.includes('<system-reminder>')
                        ? 'system'
                        : msg.role;
                    return (
                    <div key={i} className="flex gap-3">
                      <div className={`
                        shrink-0 w-16 text-[10px] font-mono text-right pt-1
                        ${role === 'system' ? 'text-cyan-600 dark:text-cyan-400' : ''}
                        ${role === 'user' ? 'text-emerald-600 dark:text-emerald-400' : ''}
                        ${role === 'assistant' ? 'text-violet-600 dark:text-violet-400' : ''}
                        ${role === 'tool' ? 'text-amber-600 dark:text-amber-400' : ''}
                      `}>
                        {role}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="text-xs text-foreground/80 whitespace-pre-wrap break-words leading-relaxed">
                          {msg.content.length > 2000 ? msg.content.slice(0, 2000) + '…' : msg.content}
                        </div>
                        {msg.ts && (
                          <div className="text-[10px] text-muted-foreground/50 mt-1">{formatTime(msg.ts)}</div>
                        )}
                      </div>
                    </div>
                    );
                  })
                )}
                </div>
              </div>
            )}

            {activeTab === 'trajectory' && (
              <div className="flex-1 min-h-0">
                {trajectoryLoading ? (
                  <div className="flex items-center justify-center h-20 text-xs text-muted-foreground">
                    <Loader2 className="size-3 animate-spin mr-2" />Loading trajectory…
                  </div>
                ) : trajectoryData ? (
                  <TrajectoryErrorBoundary>
                    <TrajectoryView data={trajectoryData} focusTs={focusTs} />
                  </TrajectoryErrorBoundary>
                ) : (
                  <div className="flex items-center justify-center h-20 text-xs text-muted-foreground">
                    No trajectory data available
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </main>

      {pendingDeleteGroup && (
        <>
          <div
            className="fixed inset-0 z-40 bg-black/30"
            onClick={() => !deletingGroup && setPendingDeleteGroup(null)}
          />
          <div className="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
            <div className="pointer-events-auto w-80 rounded-xl border border-border/40 bg-white dark:bg-gray-900 p-4 shadow-xl">
              <div className="text-sm font-medium text-red-600 dark:text-red-400">
                Delete all conversations?
              </div>
              <p className="text-xs text-muted-foreground mt-1 break-all">
                Moves {pendingDeleteGroup.sessions.length} session(s) to the trash in{' '}
                <span className="font-mono text-[10px]">
                  {pendingDeleteGroup.storageDirs.join(', ')}
                </span>
              </p>
              <div className="flex justify-end gap-2 mt-4">
                <button
                  onClick={() => setPendingDeleteGroup(null)}
                  disabled={deletingGroup}
                  className="px-3 py-1.5 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={confirmDeleteGroup}
                  disabled={deletingGroup}
                  className="px-3 py-1.5 rounded text-xs bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-60"
                >
                  {deletingGroup ? 'Deleting…' : 'Delete'}
                </button>
              </div>
            </div>
          </div>
        </>
      )}

      {pendingDeleteSession && (
        <>
          <div
            className="fixed inset-0 z-40 bg-black/30"
            onClick={() => setPendingDeleteSession(null)}
          />
          <div className="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
            <div className="pointer-events-auto w-80 rounded-xl border border-border/40 bg-white dark:bg-gray-900 p-4 shadow-xl">
              <div className="text-sm font-medium text-red-600 dark:text-red-400">
                Delete this conversation?
              </div>
              <p className="text-xs text-muted-foreground mt-1 break-all">
                Moves session{' '}
                <span className="font-mono text-[10px]">{pendingDeleteSession.sessionId}</span>
                {pendingDeleteSession.title ? ` (${pendingDeleteSession.title})` : ''}
              </p>
              <div className="flex justify-end gap-2 mt-4">
                <button
                  onClick={() => setPendingDeleteSession(null)}
                  className="px-3 py-1.5 rounded text-xs border border-border/40 hover:bg-muted/40 transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={() => {
                    const session = pendingDeleteSession;
                    setPendingDeleteSession(null);
                    deleteSessionHandler(session);
                  }}
                  className="px-3 py-1.5 rounded text-xs bg-red-600 text-white hover:bg-red-700 transition-colors"
                >
                  Delete
                </button>
              </div>
            </div>
          </div>
        </>
      )}

      {notice && (
        <div
          className={`fixed top-12 left-1/2 -translate-x-1/2 z-[60] px-3 py-1.5 rounded-lg text-xs shadow-lg ${
            notice.type === 'ok' ? 'bg-black/85 text-white' : 'bg-red-600/90 text-white'
          }`}
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}

// ── Grouping helper ──────────────────────────────────────────────────────

/** The project group a session belongs to (shared by grouping and jumping). */
function groupNameFor(session: SessionMeta): string {
  return (
    session.projectGroup ??
    session.projectDir ??
    (session.providerId === 'claude'
      ? 'AI Code'
      : session.providerId === 'dsh'
        ? 'DSH'
        : session.providerId === 'opencode'
          ? 'OpenCode'
          : 'Codex')
  );
}

function groupByProject(
  sessions: SessionMeta[],
  _filter: ProviderFilter,
  order: readonly string[],
): GroupedSessions[] {
  const groups = new Map<string, GroupedSessions>();

  for (const session of sessions) {
    const groupName = session.projectGroup
      ?? session.projectDir
      ?? (session.providerId === 'claude' ? 'Claude Code' : session.providerId === 'dsh' ? 'DSH' : session.providerId === 'opencode' ? 'OpenCode' : 'Codex');

    const existing = groups.get(groupName);
    if (existing) {
      existing.sessions.push(session);
    } else {
      groups.set(groupName, { groupName, providerId: session.providerId, sessions: [session], storageDirs: [] });
    }
  }

  // Sort: providers in the user-defined display order, then each group by latest session.
  const result = Array.from(groups.values());
  result.sort((a, b) => {
    // Sort by provider first
    if (a.providerId !== b.providerId) {
      const ia = order.indexOf(a.providerId);
      const ib = order.indexOf(b.providerId);
      return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
    }
    // Within same provider, sort by latest session
    const aLatest = Math.max(...a.sessions.map((s) => s.lastActiveAt ?? s.createdAt ?? 0));
    const bLatest = Math.max(...b.sessions.map((s) => s.lastActiveAt ?? s.createdAt ?? 0));
    return bLatest - aLatest;
  });

  // Sort sessions within each group by last_active_at descending, and collect
  // the backing storage dirs (used by the group's bulk-delete).
  for (const group of result) {
    group.sessions.sort((a, b) => {
      const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
      const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
      return bTs - aTs;
    });
    const dirs = new Set<string>();
    for (const s of group.sessions) {
      // SQLite-backed OpenCode sessions have no directory to sweep.
      if (s.sourcePath && !s.sourcePath.startsWith('sqlite:')) {
        const p = s.sourcePath.replace(/\\/g, '/');
        const idx = p.lastIndexOf('/');
        if (idx > 0) dirs.add(p.slice(0, idx));
      }
    }
    group.storageDirs = [...dirs];
  }

  return result;
}