// ── Application Entry Point ──────────────────────────────────────────────

mod commands;
mod session_manager;
mod trajectory;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::parse_trajectory_file,
            commands::get_session_trajectory,
            commands::list_sessions,
            commands::get_session_messages,
            commands::delete_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running trajectory viewer");
}
