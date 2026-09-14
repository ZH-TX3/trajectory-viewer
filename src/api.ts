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
  TrashEntry,
  TrashManifest,
  HubSnapshot,
  ImportPreview,
  McpServer,
  McpServerSpec,
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

  /** Export one session as Markdown or JSONL; returns the record count. */
  async exportSession(
    providerId: string,
    sourcePath: string,
    title: string,
    format: 'md' | 'jsonl',
    targetPath: string,
  ): Promise<number> {
    return await invoke('export_session', { providerId, sourcePath, title, format, targetPath });
  },

  /** List trashed sessions (newest first). */
  async listTrash(): Promise<TrashEntry[]> {
    return await invoke('list_trash');
  },

  /** Restore a trashed session to its original location. */
  async restoreTrashEntry(id: string): Promise<TrashManifest> {
    return await invoke('restore_trash_entry', { id });
  },

  /** Permanently delete one trash entry. */
  async deleteTrashEntry(id: string): Promise<void> {
    return await invoke('delete_trash_entry', { id });
  },

  /** Permanently delete every trash entry. */
  async emptyTrash(): Promise<number> {
    return await invoke('empty_trash');
  },

  // ── Config Hub ─────────────────────────────────────────────────────────

  /** Tools, resources and read-only config files, in one call. */
  async configHubSnapshot(): Promise<HubSnapshot> {
    return await invoke('config_hub_snapshot');
  },

  /** Enable or disable a resource for one tool. */
  async configHubToggle(
    id: string,
    toolId: string,
    enabled: boolean,
    method?: 'auto' | 'symlink' | 'copy',
  ): Promise<void> {
    return await invoke('config_hub_toggle', { id, toolId, enabled, method });
  },

  /** Describe what importing a drifted copy would do. */
  async configHubImportPreview(id: string, toolId: string): Promise<ImportPreview> {
    return await invoke('config_hub_import_preview', { id, toolId });
  },

  /** Move a tool's copy into the unified store and link it back. */
  async configHubImport(id: string, toolId: string, overwrite: boolean): Promise<void> {
    return await invoke('config_hub_import', { id, toolId, overwrite });
  },

  /** Undo an import, moving the copy back to the tool. */
  async configHubUndoImport(id: string, toolId: string): Promise<void> {
    return await invoke('config_hub_undo_import', { id, toolId });
  },

  /** Delete a resource (unlink every tool, then trash the entry). */
  async configHubDelete(id: string): Promise<string> {
    return await invoke('config_hub_delete', { id });
  },

  /** Write one of a tool's own prompt/config files. */
  async configHubSaveConfig(toolId: string, path: string, content: string): Promise<void> {
    return await invoke('config_hub_save_config', { toolId, path, content });
  },

  // ── Config Hub: MCP ────────────────────────────────────────────────────

  /** List every MCP server in the unified store. */
  async configHubMcpList(): Promise<McpServer[]> {
    return await invoke('config_hub_mcp_list');
  },

  /** Create or update one MCP server and reconcile the tool configs. */
  async configHubMcpUpsert(
    id: string,
    name: string,
    description: string | null,
    server: McpServerSpec,
    apps: Record<string, boolean>,
  ): Promise<void> {
    return await invoke('config_hub_mcp_upsert', { id, name, description, server, apps });
  },

  /** Enable or disable an MCP server for one tool. */
  async configHubMcpToggle(id: string, toolId: string, enabled: boolean): Promise<void> {
    return await invoke('config_hub_mcp_toggle', { id, toolId, enabled });
  },

  /** Delete an MCP server (store + every tool that had it enabled). */
  async configHubMcpDelete(id: string): Promise<void> {
    return await invoke('config_hub_mcp_delete', { id });
  },

  /** Import live MCP configs from all installed tools; returns the count. */
  async configHubMcpImport(): Promise<number> {
    return await invoke('config_hub_mcp_import');
  },
};