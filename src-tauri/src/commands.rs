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
