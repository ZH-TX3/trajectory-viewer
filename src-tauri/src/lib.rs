// ── Application Entry Point ──────────────────────────────────────────────

mod backup;
mod commands;
mod export;
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
            commands::delete_sessions_in_dir,
            commands::get_session_mtime,
            commands::list_provider_session_info,
            commands::backup_providers,
            commands::restore_backup,
            commands::list_session_backups,
            commands::backup_now,
            commands::delete_session_backup,
            commands::restore_session_backup,
            commands::get_backup_settings,
            commands::set_backup_settings,
            commands::export_session,
        ])
        .setup(|_app| {
            // Background auto-backup: run a pass on startup, then every 5 min.
            crate::backup::maybe_auto_backup();
            std::thread::spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(300));
                crate::backup::maybe_auto_backup();
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running trajectory viewer");
}
