// ── Session Backup & Restore ─────────────────────────────────────────────
//
// Backs up the AI tools' real session data directories into zip archives and
// restores them back. Mirrors cc-switch's backup UX (fixed backup dir +
// auto interval + retention + restore), adapted to the externally-owned
// session directories Trajectory Viewer reads:
//   claude  → ~/.claude/projects
//   codex   → ~/.codex/sessions
//   dsh     → ~/.dsh/sessions
//   opencode→ ~/.local/share/opencode/storage (flat JSON only, not the 135MB
//              opencode.db — too large / may be locked by a running server)
//
// Backups go to ~/.trajectory-viewer/backups/session_backup_YYYYMMDD_HHMMSS.zip
// with optional auto-backup (interval) + retention pruning. Restore is
// "merge + overwrite"; the current state of each touched provider is first
// copied to a timestamped safety backup before anything is written back.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::trajectory::parser::opencode;

/// Providers whose session dirs this tool can back up (in display order).
pub const KNOWN_PROVIDERS: [&str; 4] = ["claude", "codex", "dsh", "opencode"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionInfo {
    pub provider_id: String,
    pub path: Option<String>,
    pub exists: bool,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProviderStat {
    pub provider_id: String,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub file_path: String,
    pub size_bytes: u64,
    pub providers: Vec<BackupProviderStat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub providers: Vec<BackupProviderStat>,
    /// Directory where the pre-restore state was copied (None when nothing
    /// existed to protect).
    pub safety_backup_dir: Option<String>,
    pub warnings: Vec<String>,
}

/// One entry from the fixed backups directory.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: i64,
}

/// Auto-backup preferences (persisted to ~/.trajectory-viewer/settings.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    /// 0 disables auto-backup.
    pub interval_hours: u32,
    /// Number of backups to keep (oldest pruned beyond this).
    pub retain_count: u32,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings { interval_hours: 24, retain_count: 10 }
    }
}

// ── Provider dirs ────────────────────────────────────────────────────────

pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// The session data directory for a provider, if the tool is installed.
pub fn provider_session_dir(provider_id: &str) -> Option<PathBuf> {
    match provider_id {
        "claude" => home_dir().map(|h| h.join(".claude").join("projects")),
        "codex" => home_dir().map(|h| h.join(".codex").join("sessions")),
        "dsh" => home_dir().map(|h| h.join(".dsh").join("sessions")),
        "opencode" => opencode::get_opencode_base_dir().map(|base| base.join("storage")),
        _ => None,
    }
}

fn count_files(dir: &Path, count: &mut usize, bytes: &mut u64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count_files(&path, count, bytes);
            } else if let Ok(meta) = std::fs::metadata(&path) {
                *count += 1;
                *bytes += meta.len();
            }
        }
    }
}

/// Per-provider existence/size info for the backup panel's checkboxes.
pub fn list_provider_session_info() -> Vec<ProviderSessionInfo> {
    KNOWN_PROVIDERS
        .iter()
        .map(|id| {
            let dir = provider_session_dir(id);
            let mut info = ProviderSessionInfo {
                provider_id: (*id).to_string(),
                path: dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
                exists: false,
                file_count: 0,
                total_bytes: 0,
            };
            if let Some(path) = &dir {
                if path.is_dir() {
                    info.exists = true;
                    count_files(path, &mut info.file_count, &mut info.total_bytes);
                }
            }
            info
        })
        .collect()
}

// ── Backup: provider dirs → single zip ───────────────────────────────────

/// Write the session dirs of `provider_ids` into `target_path` as a zip,
/// each provider under a top-level folder named after it.
pub fn backup_providers(provider_ids: &[String], target_path: &Path) -> Result<BackupResult, String> {
    let file = File::create(target_path)
        .map_err(|e| format!("Cannot create backup file {}: {e}", target_path.display()))?;

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut writer = ZipWriter::new(BufWriter::new(file));
    let mut stats = Vec::new();

    for id in provider_ids {
        let Some(dir) = provider_session_dir(id) else {
            stats.push(BackupProviderStat { provider_id: id.clone(), file_count: 0, total_bytes: 0 });
            continue;
        };
        if !dir.is_dir() {
            stats.push(BackupProviderStat { provider_id: id.clone(), file_count: 0, total_bytes: 0 });
            continue;
        }
        let prefix = Path::new(id.as_str());
        let (file_count, total_bytes) = add_dir_to_zip(&mut writer, &dir, &dir, prefix, &options)?;
        stats.push(BackupProviderStat { provider_id: id.clone(), file_count, total_bytes });
    }

    writer.finish().map_err(|e| format!("Failed to finalize zip: {e}"))?;

    let size_bytes = std::fs::metadata(target_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(BackupResult {
        file_path: target_path.to_string_lossy().into_owned(),
        size_bytes,
        providers: stats,
    })
}

fn add_dir_to_zip<W: Write + io::Seek>(
    writer: &mut ZipWriter<W>,
    root: &Path,
    dir: &Path,
    prefix: &Path,
    options: &SimpleFileOptions,
) -> Result<(usize, u64), String> {
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|_| "Path mismatch while walking session dir".to_string())?;
        let zip_name = prefix.join(rel).to_string_lossy().replace('\\', "/");

        if path.is_dir() {
            writer
                .add_directory(format!("{zip_name}/"), *options)
                .map_err(|e| format!("Zip error on {zip_name}: {e}"))?;
            let (c, b) = add_dir_to_zip(writer, root, &path, prefix, options)?;
            file_count += c;
            total_bytes += b;
        } else {
            let meta = std::fs::metadata(&path).map_err(|e| format!("Cannot stat {}: {e}", path.display()))?;
            writer
                .start_file(zip_name.clone(), *options)
                .map_err(|e| format!("Zip error on {zip_name}: {e}"))?;
            let mut src = File::open(&path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
            let n = io::copy(&mut src, writer).map_err(|e| format!("Zip write error: {e}"))?;
            file_count += 1;
            total_bytes += meta.len();
            let _ = n;
        }
    }

    Ok((file_count, total_bytes))
}

// ── Restore: zip → provider dirs (merge + overwrite) ─────────────────────

/// Restore a backup zip back into the provider session dirs.
///
/// Safety: existing state of each touched provider dir is first copied to
/// `~/.trajectory-viewer/backups/safety-{timestamp}-{provider}/`, then zip
/// entries overwrite same-named targets while leaving extra files alone.
pub fn restore_backup(file_path: &Path) -> Result<RestoreResult, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("Cannot open backup {}: {e}", file_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Invalid zip archive: {e}"))?;

    // Group zip entries by top-level provider folder.
    let mut by_provider: std::collections::BTreeMap<String, Vec<(String, PathBuf)>> =
        std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| format!("Zip read error: {e}"))?;
        // enclosed_name() refuses any entry escaping the archive root.
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| "Backup contains an unsafe path".to_string())?
            .to_path_buf();
        let mut comps = rel.components();
        let top = comps
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or_default()
            .to_string();
        // Everything under the top-level provider folder maps 1:1 under that
        // provider's session dir (strip the provider segment before joining).
        let under = comps.as_path().to_path_buf();
        let name = entry.name().to_string();
        by_provider.entry(top).or_default().push((name, under));
    }

    let mut stats = Vec::new();
    let mut warnings = Vec::new();
    let mut safety_dirs: Vec<String> = Vec::new();

    for (provider, entries) in &by_provider {
        let Some(target_root) = provider_session_dir(provider) else {
            warnings.push(format!("Unknown provider in backup: {provider} — skipped"));
            continue;
        };
        std::fs::create_dir_all(&target_root)
            .map_err(|e| format!("Cannot create {}: {e}", target_root.display()))?;

        // Safety copy of current state (before any overwrite).
        if has_any_file(&target_root) {
            if let Some(safety) = copy_to_safety_backup(&target_root, provider) {
                safety_dirs.push(safety);
            }
        }

        let mut file_count = 0usize;
        let mut total_bytes = 0u64;
        for (zip_name, rel_path) in entries {
            let out = target_root.join(rel_path);
            if rel_path.as_os_str().is_empty() {
                std::fs::create_dir_all(&target_root).ok();
                continue;
            }
            if let Ok(mut entry) = archive.by_name(zip_name) {
                if entry.is_dir() {
                    std::fs::create_dir_all(&out)
                        .map_err(|e| format!("Cannot create {}: {e}", out.display()))?;
                } else {
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
                    }
                    let mut dest = File::create(&out)
                        .map_err(|e| format!("Cannot write {}: {e}", out.display()))?;
                    let n = io::copy(&mut entry, &mut dest)
                        .map_err(|e| format!("Restore write error: {e}"))?;
                    total_bytes += n;
                    file_count += 1;
                }
            }
        }
        stats.push(BackupProviderStat { provider_id: provider.clone(), file_count, total_bytes });
    }

    Ok(RestoreResult {
        providers: stats,
        safety_backup_dir: if safety_dirs.is_empty() { None } else { Some(safety_dirs.join(", ")) },
        warnings,
    })
}

fn has_any_file(dir: &Path) -> bool {
    let mut n = 0usize;
    let mut b = 0u64;
    count_files(dir, &mut n, &mut b);
    n > 0
}

fn copy_to_safety_backup(dir: &Path, provider: &str) -> Option<String> {
    let base = home_dir()?.join(".trajectory-viewer").join("backups");
    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let dest = base.join(format!("safety-{ts}-{provider}"));
    if copy_dir_recursive(dir, &dest).is_ok() {
        Some(dest.to_string_lossy().into_owned())
    } else {
        None
    }
}

fn copy_dir_recursive(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

// ── Fixed backup dir + auto settings ─────────────────────────────────────

fn data_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".trajectory-viewer")
}

fn backups_dir() -> PathBuf {
    data_dir().join("backups")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn backup_prefix(ts: &str) -> String {
    format!("session_backup_{ts}")
}

/// Sanitize a stored backup filename so restore/delete cannot escape the
/// backups directory.
fn backup_file_path(filename: &str) -> Result<PathBuf, String> {
    let name = Path::new(filename);
    if name.components().count() != 1 {
        return Err("Invalid backup filename".to_string());
    }
    let name_str = name
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid backup filename".to_string())?;
    if !name_str.starts_with("session_backup_") || !name_str.ends_with(".zip") {
        return Err("Invalid backup filename".to_string());
    }
    Ok(backups_dir().join(name_str))
}

/// List backups from the fixed directory, newest first.
pub fn list_backups() -> Vec<BackupEntry> {
    let dir = backups_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().into_owned();
            if !fname.starts_with("session_backup_") || !fname.ends_with(".zip") {
                continue;
            }
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let created_at = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.created().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            entries.push(BackupEntry { filename: fname, size_bytes, created_at });
        }
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    entries
}

/// Create a fresh backup of `provider_ids` into the fixed backups dir.
pub fn backup_now(provider_ids: &[String]) -> Result<BackupResult, String> {
    std::fs::create_dir_all(backups_dir())
        .map_err(|e| format!("Cannot create backups dir: {e}"))?;
    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("{}.zip", backup_prefix(&ts.to_string()));
    let target = backups_dir().join(&filename);
    let mut res = backup_providers(provider_ids, &target)?;
    res.file_path = target.to_string_lossy().into_owned();
    Ok(res)
}

/// Delete a backup from the fixed directory by filename.
pub fn delete_backup(filename: &str) -> Result<(), String> {
    let path = backup_file_path(filename)?;
    if !path.exists() {
        return Err(format!("Backup not found: {filename}"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Cannot delete backup: {e}"))
}

/// Restore a backup from the fixed directory by filename.
pub fn restore_backup_by_name(filename: &str) -> Result<RestoreResult, String> {
    let path = backup_file_path(filename)?;
    if !path.exists() {
        return Err(format!("Backup not found: {filename}"));
    }
    restore_backup(&path)
}

/// Read persisted backup preferences (with defaults).
pub fn get_backup_settings() -> BackupSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<BackupSettings>(&raw).ok())
        .unwrap_or_default()
}

/// Persist backup preferences, clamped to sane ranges.
pub fn set_backup_settings(settings: &BackupSettings) -> Result<(), String> {
    let clamped = BackupSettings {
        interval_hours: settings.interval_hours.min(24 * 30),
        retain_count: settings.retain_count.clamp(1, 100),
    };
    std::fs::create_dir_all(data_dir()).map_err(|e| format!("Cannot create data dir: {e}"))?;
    let raw = serde_json::to_string_pretty(&clamped)
        .map_err(|e| format!("Cannot serialize settings: {e}"))?;
    std::fs::write(settings_path(), raw).map_err(|e| format!("Cannot save settings: {e}"))
}

/// One auto-backup + retention pass; runs on a background timer.
pub fn maybe_auto_backup() {
    let settings = get_backup_settings();
    let last = list_backups().first().map(|b| b.created_at);
    let due = match last {
        Some(created) if settings.interval_hours > 0 => {
            let elapsed_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0) - created;
            elapsed_ms >= (settings.interval_hours as i64) * 3600 * 1000
        }
        Some(_) => false,   // auto-backup disabled
        None => settings.interval_hours > 0, // no backups yet → create one
    };

    if due {
        let providers: Vec<String> = KNOWN_PROVIDERS.iter().map(|s| s.to_string()).collect();
        if let Err(err) = backup_now(&providers) {
            eprintln!("[backup] auto-backup failed: {err}");
        }
    }

    // Prune old backups beyond the retention count.
    let mut backups = list_backups();
    while backups.len() > settings.retain_count.max(1) as usize {
        if let Some(oldest) = backups.pop() {
            let _ = delete_backup(&oldest.filename);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// All env-mutating tests (this module + opencode) share one lock so the
    /// process-global HOME/XDG mutations never race across parallel tests.
    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap();
        let original_home = std::env::var_os("HOME");
        let original_xdg = std::env::var_os("XDG_DATA_HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", home);
        // Force opencode's base-dir resolution to use HOME (not a stale XDG).
        #[allow(deprecated)]
        std::env::remove_var("XDG_DATA_HOME");
        f();
        match original_home {
            Some(value) => {
                #[allow(deprecated)]
                std::env::set_var("HOME", value);
            }
            None => {
                #[allow(deprecated)]
                std::env::remove_var("HOME");
            }
        }
        match original_xdg {
            Some(value) => {
                #[allow(deprecated)]
                std::env::set_var("XDG_DATA_HOME", value);
            }
            None => {
                #[allow(deprecated)]
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
    }

    fn write_file(path: &Path, contents: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            // Real-looking session dirs for three providers.
            write_file(&home.join(".claude/projects/proj-x/sess-1.jsonl"), b"{}\n{}\n");
            write_file(&home.join(".codex/sessions/2026/08/31/rollout-a.jsonl"), b"line\n");
            write_file(
                &home.join(".local/share/opencode/storage/message/ses_1/msg_1.json"),
                br#"{"role":"user"}"#,
            );
            // opencode.db must NOT be included in the backup.
            write_file(&home.join(".local/share/opencode/opencode.db"), b"raw db bytes");

            let target = dir.path().join("backup.zip");
            let res = backup_providers(
                &["claude".to_string(), "codex".to_string(), "opencode".to_string()],
                &target,
            ).unwrap();
            assert_eq!(res.providers.len(), 3);
            assert_eq!(res.providers[0].provider_id, "claude");
            assert_eq!(res.providers[0].file_count, 1);

            // opencode.db excluded from the archive.
            let mut archive = zip::ZipArchive::new(File::open(&target).unwrap()).unwrap();
            for i in 0..archive.len() {
                let name = archive.by_index(i).unwrap().name().to_string();
                assert!(!name.contains("opencode.db"), "db leaked into archive: {name}");
            }
            drop(archive);

            // Wipe the session dirs, then restore.
            let claude_dir = home.join(".claude/projects/proj-x");
            let codex_dir = home.join(".codex/sessions/2026/08/31");
            std::fs::remove_dir_all(&claude_dir).unwrap();
            std::fs::remove_dir_all(&codex_dir).unwrap();

            let rr = restore_backup(&target).unwrap();
            assert!(rr.providers.iter().any(|s| s.provider_id == "claude" && s.file_count == 1));
            assert!(rr.safety_backup_dir.is_some(), "pre-restore safety copy created");

            assert!(home.join(".claude/projects/proj-x/sess-1.jsonl").exists());
            assert!(home.join(".codex/sessions/2026/08/31/rollout-a.jsonl").exists());
            assert!(home.join(".local/share/opencode/storage/message/ses_1/msg_1.json").exists());
        });
    }

    #[test]
    fn restore_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("evil.zip");
        let file = File::create(&target).unwrap();
        let mut writer = ZipWriter::new(BufWriter::new(file));
        writer
            .start_file("../escape.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"boom").unwrap();
        writer.finish().unwrap();

        let err = restore_backup(&target).unwrap_err();
        assert!(err.contains("unsafe"), "expected unsafe-path rejection, got: {err}");
    }
}