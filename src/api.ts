// ── Tauri API Layer ──────────────────────────────────────────────────────

import { invoke } from '@tauri-apps/api/core';
import type {
  TrajectoryData,
  SessionMeta,
  SessionMessage,
  ProviderSessionInfo,
  BackupResult,
  RestoreResult,
  BackupEntry,
  BackupSettings,
} from './types';

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

  /** Existence/size of every tool's session directory. */
  async listProviderSessionInfo(): Promise<ProviderSessionInfo[]> {
    return await invoke('list_provider_session_info');
  },

  /** Pack the selected providers' session dirs into a zip. */
  async backupProviders(providers: string[], targetPath: string): Promise<BackupResult> {
    return await invoke('backup_providers', { providers, targetPath });
  },

  /** Restore a session backup zip (merge + overwrite, safety copy first). */
  async restoreBackup(filePath: string): Promise<RestoreResult> {
    return await invoke('restore_backup', { filePath });
  },

  /** List stored backups (newest first). */
  async listSessionBackups(): Promise<BackupEntry[]> {
    return await invoke('list_session_backups');
  },

  /** Create a fresh backup of the selected providers into the fixed dir. */
  async backupNow(providers: string[]): Promise<BackupResult> {
    return await invoke('backup_now', { providers });
  },

  /** Delete a stored backup by filename. */
  async deleteSessionBackup(filename: string): Promise<void> {
    return await invoke('delete_session_backup', { filename });
  },

  /** Restore a stored backup by filename. */
  async restoreSessionBackup(filename: string): Promise<RestoreResult> {
    return await invoke('restore_session_backup', { filename });
  },

  /** Current auto-backup preferences. */
  async getBackupSettings(): Promise<BackupSettings> {
    return await invoke('get_backup_settings');
  },

  /** Persist auto-backup preferences (interval hours + retain count). */
  async setBackupSettings(settings: BackupSettings): Promise<void> {
    return await invoke('set_backup_settings', { settings });
  },
};