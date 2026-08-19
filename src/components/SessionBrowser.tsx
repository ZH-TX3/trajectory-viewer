// ── Session Browser ──────────────────────────────────────────────────────
//
// Left sidebar: sessions grouped by project directory hierarchy.
// Right panel: Messages/Trajectory tabs.

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { api } from '../api';
import { TrajectoryView } from './TrajectoryView';
import type { SessionMeta, SessionMessage, TrajectoryData } from '../types';
import {
  MessageSquare, GitBranch, Clock, FileText, Loader2,
  ChevronRight, ChevronDown, Folder, FolderOpen, GripVertical,
} from 'lucide-react';

interface SessionBrowserProps {
  onOpenFile: () => void;
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
}

export function SessionBrowser({ onOpenFile }: SessionBrowserProps) {
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

  // Filter sessions by provider
  const filteredSessions = useMemo(() => {
    if (providerFilter === 'all') return sessions;
    return sessions.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter]);

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

  // Load messages when session changes
  useEffect(() => {
    if (!selectedSession?.sourcePath) return;
    setMessagesLoading(true);
    setMessages([]);
    api.getSessionMessages(selectedSession.providerId, selectedSession.sourcePath)
      .then(setMessages)
      .catch(console.error)
      .finally(() => setMessagesLoading(false));
  }, [selectedSession?.providerId, selectedSession?.sourcePath]);

  // Load trajectory when tab switches to trajectory
  useEffect(() => {
    if (activeTab !== 'trajectory' || !selectedSession?.sourcePath) return;
    setTrajectoryLoading(true);
    api.getSessionTrajectory(selectedSession.providerId, selectedSession.sourcePath)
      .then(setTrajectoryData)
      .catch(console.error)
      .finally(() => setTrajectoryLoading(false));
  }, [activeTab, selectedSession?.providerId, selectedSession?.sourcePath]);

  const handleSelectSession = useCallback((key: string) => {
    setSelectedKey(key);
    setActiveTab('messages');
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

  const formatTime = (ts: number | null | undefined) => {
    if (!ts) return '';
    return new Date(ts).toLocaleString();
  };

  const sessionTitle = (s: SessionMeta) => {
    return s.title ?? s.sessionId.slice(0, 8) + '…';
  };

  const providerIcon = (id: string) => {
    return id === 'claude' ? 'text-violet-600 dark:text-violet-400'
      : id === 'dsh' ? 'text-amber-600 dark:text-amber-400'
      : 'text-emerald-600 dark:text-emerald-400';
  };

  const claudeCount = sessions.filter((s) => s.providerId === 'claude').length;
  const codexCount = sessions.filter((s) => s.providerId === 'codex').length;
  const dshCount = sessions.filter((s) => s.providerId === 'dsh').length;

  return (
    <div className="flex h-full min-h-0">
      {/* Left sidebar */}
      <aside
        className="border-r border-border/40 bg-muted/20 flex flex-col shrink-0 overflow-hidden"
        style={{ width: sidebarWidth }}
      >
        {/* Provider filter icons */}
        <div className="border-b border-border/40">
          <div className="flex items-center gap-1 px-2 py-1.5">
            <button
              onClick={() => setProviderFilter('all')}
              className={`flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium transition-colors ${
                providerFilter === 'all'
                  ? 'bg-muted/80 text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
              }`}
            >
              <MessageSquare className="size-3" />
              <span>All</span>
              <span className="text-[10px] opacity-60">{sessions.length}</span>
            </button>
            <button
              onClick={() => setProviderFilter('claude')}
              className={`flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium transition-colors ${
                providerFilter === 'claude'
                  ? 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300 shadow-sm'
                  : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
              }`}
            >
              <ClaudeIcon className="size-3.5" />
              <span>Claude</span>
              <span className="text-[10px] opacity-60">{claudeCount}</span>
            </button>
            <button
              onClick={() => setProviderFilter('codex')}
              className={`flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium transition-colors ${
                providerFilter === 'codex'
                  ? 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 shadow-sm'
                  : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
              }`}
            >
              <CodexIcon className="size-3.5" />
              <span>Codex</span>
              <span className="text-[10px] opacity-60">{codexCount}</span>
            </button>
            {/* DSH */}
            <button
              onClick={() => setProviderFilter('dsh')}
              className={`flex items-center gap-1 px-2 py-1 rounded text-[11px] font-medium transition-colors ${
                providerFilter === 'dsh'
                  ? 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 shadow-sm'
                  : 'text-muted-foreground hover:text-foreground hover:bg-muted/40'
              }`}
            >
              <DshIcon className="size-3.5" />
              <span>DSH</span>
              <span className="text-[10px] opacity-60">{dshCount}</span>
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
                      className="flex items-center gap-1.5 px-2 py-1.5 cursor-pointer hover:bg-muted/40 transition-colors text-[11px] font-medium text-muted-foreground"
                    >
                      {isExpanded ? <ChevronDown className="size-3 shrink-0" /> : <ChevronRight className="size-3 shrink-0" />}
                      {isExpanded ? <FolderOpen className="size-3.5 shrink-0" /> : <Folder className="size-3.5 shrink-0" />}
                      <span className="truncate">{group.groupName}</span>
                      <span className="text-[10px] opacity-60 ml-auto">{group.sessions.length}</span>
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
                            flex items-start gap-2 pl-7 pr-3 py-2 cursor-pointer transition-all
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
                            <div className="text-xs font-medium truncate">
                              {sessionTitle(session)}
                            </div>
                            <div className="text-[10px] text-muted-foreground/60 flex items-center gap-1 mt-0.5">
                              <Clock className="size-2.5 shrink-0" />
                              <span className="truncate">{formatTime(session.lastActiveAt ?? session.createdAt)}</span>
                            </div>
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
              <div className="flex-1 overflow-auto p-4 space-y-3">
                {messagesLoading ? (
                  <div className="flex items-center justify-center h-20 text-xs text-muted-foreground">
                    <Loader2 className="size-3 animate-spin mr-2" />Loading messages…
                  </div>
                ) : messages.length === 0 ? (
                  <div className="text-xs text-muted-foreground text-center pt-8">No messages</div>
                ) : (
                  messages.map((msg, i) => (
                    <div key={i} className="flex gap-3">
                      <div className={`
                        shrink-0 w-16 text-[10px] font-mono text-right pt-1
                        ${msg.role === 'user' ? 'text-emerald-600 dark:text-emerald-400' : ''}
                        ${msg.role === 'assistant' ? 'text-violet-600 dark:text-violet-400' : ''}
                        ${msg.role === 'tool' ? 'text-amber-600 dark:text-amber-400' : ''}
                      `}>
                        {msg.role}
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
                  ))
                )}
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
      groups.set(groupName, { groupName, providerId: session.providerId, sessions: [session] });
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

  // Sort sessions within each group by last_active_at descending
  for (const group of result) {
    group.sessions.sort((a, b) => {
      const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
      const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
      return bTs - aTs;
    });
  }

  return result;
}