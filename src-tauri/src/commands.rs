// ── Tauri Commands ───────────────────────────────────────────────────────

use crate::session_manager;
use crate::trajectory;

#[tauri::command]
pub fn parse_trajectory_file(source_path: String) -> Result<trajectory::TrajectoryData, String> {
    trajectory::parse_trajectory_file(&source_path)
}

#[tauri::command]
pub fn get_session_trajectory(
    provider_id: String,
    source_path: String,
) -> Result<trajectory::TrajectoryData, String> {
    trajectory::parse_trajectory(&provider_id, &source_path)
}

#[tauri::command]
pub fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    Ok(session_manager::scan_sessions())
}

#[tauri::command]
pub fn get_session_messages(
    provider_id: String,
    source_path: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    session_manager::load_messages(&provider_id, &source_path)
}

/// Delete a session file (and its now-empty parent dir). Only paths inside the
/// managed session directories are accepted.
#[tauri::command]
pub fn delete_session(source_path: String) -> Result<(), String> {
    session_manager::delete_session_file(&source_path)
}

/// Delete every session file inside a managed project directory.
#[tauri::command]
pub fn delete_sessions_in_dir(dir: String) -> Result<usize, String> {
    session_manager::delete_sessions_in_dir(&dir)
}

/// Last-modified timestamp (ms) of a session file, for change polling.
#[tauri::command]
pub fn get_session_mtime(source_path: String) -> Result<i64, String> {
    let path = std::path::Path::new(&source_path);
    let meta = std::fs::metadata(path).map_err(|e| format!("Cannot stat session file: {e}"))?;
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .ok_or_else(|| "Cannot read modification time".to_string())
}

// ── Session backup / restore ─────────────────────────────────────────────

/// Existence + size of every tool's session directory (for the backup panel).
#[tauri::command]
pub fn list_provider_session_info() -> Vec<crate::backup::ProviderSessionInfo> {
    crate::backup::list_provider_session_info()
}

/// Pack the selected providers' session dirs into a single zip at `target_path`.
#[tauri::command]
pub fn backup_providers(
    providers: Vec<String>,
    target_path: String,
) -> Result<crate::backup::BackupResult, String> {
    crate::backup::backup_providers(&providers, std::path::Path::new(&target_path))
}

/// Restore a session backup zip back into the tool session dirs (merge +
/// overwrite, with an automatic safety copy of the current state first).
#[tauri::command]
pub fn restore_backup(file_path: String) -> Result<crate::backup::RestoreResult, String> {
    crate::backup::restore_backup(std::path::Path::new(&file_path))
}

/// List stored backups (newest first) from the fixed backups directory.
#[tauri::command]
pub fn list_session_backups() -> Vec<crate::backup::BackupEntry> {
    crate::backup::list_backups()
}

/// Create a fresh backup of the selected providers into the fixed directory.
#[tauri::command]
pub fn backup_now(providers: Vec<String>) -> Result<crate::backup::BackupResult, String> {
    crate::backup::backup_now(&providers)
}

/// Delete a stored backup by filename.
#[tauri::command]
pub fn delete_session_backup(filename: String) -> Result<(), String> {
    crate::backup::delete_backup(&filename)
}

/// Restore a stored backup by filename.
#[tauri::command]
pub fn restore_session_backup(filename: String) -> Result<crate::backup::RestoreResult, String> {
    crate::backup::restore_backup_by_name(&filename)
}

/// Current auto-backup preferences.
#[tauri::command]
pub fn get_backup_settings() -> crate::backup::BackupSettings {
    crate::backup::get_backup_settings()
}

/// Persist auto-backup preferences (interval hours + retain count).
#[tauri::command]
pub fn set_backup_settings(settings: crate::backup::BackupSettings) -> Result<(), String> {
    crate::backup::set_backup_settings(&settings)
}
