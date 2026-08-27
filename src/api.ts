// ── Tauri API Layer ──────────────────────────────────────────────────────

import { invoke } from '@tauri-apps/api/core';
import type { TrajectoryData, SessionMeta, SessionMessage } from './types';

export const api = {
  /** Parse a trajectory file from disk, auto-detecting the provider. */
  async parseTrajectoryFile(sourcePath: string): Promise<TrajectoryData> {
    return await invoke('parse_trajectory_file', { sourcePath });
  },

  /** Get trajectory data for a known session. */
  async getSessionTrajectory(
    providerId: string,
    sourcePath: string,
  ): Promise<TrajectoryData> {
    return await invoke('get_session_trajectory', { providerId, sourcePath });
  },

  /** List all sessions (Claude Code + Codex). */
  async listSessions(): Promise<SessionMeta[]> {
    return await invoke('list_sessions');
  },

  /** Load messages for a session. */
  async getSessionMessages(
    providerId: string,
    sourcePath: string,
  ): Promise<SessionMessage[]> {
    return await invoke('get_session_messages', { providerId, sourcePath });
  },

  /** Delete a session file directly. */
  async deleteSession(sourcePath: string): Promise<void> {
    return await invoke('delete_session', { sourcePath });
  },

  /** Delete every session file under a managed project directory. */
  async deleteSessionsInDir(dir: string): Promise<number> {
    return await invoke('delete_sessions_in_dir', { dir });
  },

  /** Last-modified ms timestamp of a session file (for change polling). */
  async getSessionMtime(sourcePath: string): Promise<number> {
    return await invoke('get_session_mtime', { sourcePath });
  },
};