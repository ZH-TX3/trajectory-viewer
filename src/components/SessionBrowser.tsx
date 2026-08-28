// ── Session Browser ──────────────────────────────────────────────────────
//
// Left sidebar: sessions grouped by project directory hierarchy.
// Right panel: Messages/Trajectory tabs.

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import type { FC } from 'react';
import { api } from '../api';
import { TrajectoryView } from './TrajectoryView';
import type { SessionMeta, SessionMessage, TrajectoryData } from '../types';
import {
  MessageSquare, GitBranch, Clock, FileText, Loader2,
  ChevronRight, ChevronDown, Folder, FolderOpen, GripVertical,
  Pencil, Trash2, Copy, Check, RotateCw,
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
const PROVIDER_CHIPS: Array<{
  id: Exclude<ProviderFilter, 'all'>;
  label: string;
  icon: FC<{ className?: string }>;
  active: string;
}> = [
  { id: 'claude', label: 'Claude', icon: ClaudeIcon, active: 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300 shadow-sm' },
  { id: 'codex', label: 'Codex', icon: CodexIcon, active: 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 shadow-sm' },
  { id: 'dsh', label: 'DSH', icon: DshIcon, active: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 shadow-sm' },
];

interface SessionBrowserProps {
  onOpenFile: () => void;
  enabledProviders: ReadonlySet<string>;
}

type Tab = 'messages' | 'trajectory';
type ProviderFilter = 'all' | 'claude' | 'codex' | 'dsh';

// ── Provider Icons ───────────────────────────────────────────────────────

function ClaudeIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 32 32" fill="none" aria-hidden="true">
      <circle cx="16" cy="16" r="14" fill="currentColor" opacity="0.15" />
      <path d="M10 20c0-3.3 2.7-6 6-6s6 2.7 6 6" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      <circle cx="12" cy="12" r="1.5" fill="currentColor" />
      <circle cx="20" cy="12" r="1.5" fill="currentColor" />
    </svg>
  );
}

function CodexIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 32 32" fill="none" aria-hidden="true">
      <rect x="4" y="4" width="24" height="24" rx="6" fill="currentColor" opacity="0.15" />
      <path d="M12 20l4-4-4-4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M18 20h4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function DshIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 32 32" fill="none" aria-hidden="true">
      <rect x="6" y="6" width="20" height="20" rx="4" fill="currentColor" opacity="0.12" />
      <path d="M10 16l4-4 4 4-4 4-4-4z" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" fill="none" />
      <path d="M18 18h4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

// ── Group entry ──────────────────────────────────────────────────────────

interface GroupedSessions {
  groupName: string;
  providerId: string;
  sessions: SessionMeta[];
  /** Storage directories backing this group (for bulk delete). */
  storageDirs: string[];
}

export function SessionBrowser({ onOpenFile, enabledProviders }: SessionBrowserProps) {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('messages');
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>('all');
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
        // Auto-expand first group
        const groups = groupByProject(list, 'all');
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
    return groupByProject(filteredSessions, providerFilter);
  }, [filteredSessions, providerFilter]);

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
  }, []);

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
      showNotice('ok', 'Session deleted');
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
    showNotice('ok', total > 0 ? `Deleted ${total} session(s)` : 'Nothing to delete');
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

  const formatTime = (ts: number | null | undefined) => {
    if (!ts) return '';
    return new Date(ts).toLocaleString();
  };

  const providerIcon = (id: string) => {
    return id === 'claude' ? 'text-violet-600 dark:text-violet-400'
      : id === 'dsh' ? 'text-amber-600 dark:text-amber-400'
      : 'text-emerald-600 dark:text-emerald-400';
  };

  const claudeCount = sessions.filter((s) => s.providerId === 'claude').length;
  const codexCount = sessions.filter((s) => s.providerId === 'codex').length;
  const dshCount = sessions.filter((s) => s.providerId === 'dsh').length;
  const chipIdCount = (id: string) =>
    id === 'claude' ? claudeCount : id === 'codex' ? codexCount : dshCount;

  return (
    <div className="flex h-full min-h-0">
      {/* Left sidebar */}
      <aside
        className="border-r border-border/40 bg-muted/20 flex flex-col shrink-0 overflow-hidden"
        style={{ width: sidebarWidth }}
      >
        {/* Provider filter icons */}
        <div className="border-b border-border/40">
          <div className="flex items-center gap-1 px-2 py-1.5 overflow-x-auto">
            {PROVIDER_CHIPS.filter((chip) => enabledProviders.has(chip.id)).map((chip) => (
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
            <button
              onClick={refreshAll}
              title="Refresh sessions"
              className="ml-auto shrink-0 p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
            >
              <RotateCw className="size-3.5" />
            </button>
          </div>
        </div>

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
                            {session.providerId === 'claude' ? <ClaudeIcon className="size-3.5" /> : session.providerId === 'dsh' ? <DshIcon className="size-3.5" /> : <CodexIcon className="size-3.5" />}
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
                {selectedSession.providerId === 'claude' ? <ClaudeIcon className="size-4" /> : selectedSession.providerId === 'dsh' ? <DshIcon className="size-4" /> : <CodexIcon className="size-4" />}
              </div>
              <span className="text-xs font-medium truncate">{sessionTitle(selectedSession)}</span>

              <div className="flex ml-auto gap-1">
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
                  <TrajectoryView data={trajectoryData} />
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
                Permanently removes {pendingDeleteGroup.sessions.length} session(s) in{' '}
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
                Permanently removes session{' '}
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

function groupByProject(sessions: SessionMeta[], _filter: ProviderFilter): GroupedSessions[] {
  const groups = new Map<string, GroupedSessions>();

  for (const session of sessions) {
    const groupName = session.projectGroup
      ?? session.projectDir
      ?? (session.providerId === 'claude' ? 'Claude Code' : session.providerId === 'dsh' ? 'DSH' : 'Codex');

    const existing = groups.get(groupName);
    if (existing) {
      existing.sessions.push(session);
    } else {
      groups.set(groupName, { groupName, providerId: session.providerId, sessions: [session], storageDirs: [] });
    }
  }

  // Sort: Claude groups first, then Codex groups; within each, by latest session
  const result = Array.from(groups.values());
  result.sort((a, b) => {
    // Sort by provider first
    if (a.providerId !== b.providerId) {
      return a.providerId === 'claude' ? -1 : 1;
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
      if (s.sourcePath) {
        const p = s.sourcePath.replace(/\\/g, '/');
        const idx = p.lastIndexOf('/');
        if (idx > 0) dirs.add(p.slice(0, idx));
      }
    }
    group.storageDirs = [...dirs];
  }

  return result;
}