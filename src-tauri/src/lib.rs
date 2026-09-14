// ── Application Entry Point ──────────────────────────────────────────────

mod backup;
mod commands;
mod config_hub;
mod export;
mod session_manager;
mod trajectory;
mod trash;

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
            commands::list_trash,
            commands::restore_trash_entry,
            commands::delete_trash_entry,
            commands::empty_trash,
            config_hub::config_hub_snapshot,
            config_hub::config_hub_toggle,
            config_hub::config_hub_import_preview,
            config_hub::config_hub_import,
            config_hub::config_hub_undo_import,
            config_hub::config_hub_delete,
            config_hub::config_hub_save_config,
            config_hub::config_hub_mcp_list,
            config_hub::config_hub_mcp_upsert,
            config_hub::config_hub_mcp_toggle,
            config_hub::config_hub_mcp_delete,
            config_hub::config_hub_mcp_import,
        ])
        .setup(|_app| {
            // Background auto-backup: run a pass on startup, then every 5 min.
            crate::backup::maybe_auto_backup();
            std::thread::spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(300));
                crate::backup::maybe_auto_backup();
            });
            // Drop trash entries older than 30 days (trash is still directly
            // manageable in the UI; this is just unbounded-growth protection).
            let _ = crate::trash::purge_old_entries(30);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running trajectory viewer");
}
