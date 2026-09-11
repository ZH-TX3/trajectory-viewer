// ── Session Trash ────────────────────────────────────────────────────────
//
// Deletes used to be permanent. Instead, sessions are moved into
// ~/.trajectory-viewer/trash/<entry-id>/ together with a manifest recording
// where each piece came from, so a delete can be undone.
//
// Two entry kinds:
//   "files"          — real paths were moved (claude/codex/dsh files, an
//                      opencode JSON session's file + message/part dirs)
//   "opencode-sqlite"— the session's rows were dumped to JSON (they live in a
//                      database, so there is nothing to move); restore
//                      re-inserts them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

/// Where a trashed session's pieces came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    /// Absolute path the item was moved from.
    pub original_path: String,
    /// Name it is stored under inside the trash entry directory.
    pub stored_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashManifest {
    pub provider_id: String,
    pub session_id: String,
    pub title: String,
    pub deleted_at: i64,
    /// `"files"` or `"opencode-sqlite"`.
    pub kind: String,
    #[serde(default)]
    pub items: Vec<TrashItem>,
}

/// One entry shown in the trash list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub id: String,
    pub provider_id: String,
    pub session_id: String,
    pub title: String,
    pub deleted_at: i64,
    pub size_bytes: u64,
}

const MANIFEST_NAME: &str = "manifest.json";
const SQLITE_DUMP_NAME: &str = "opencode-session.json";

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

pub fn trash_root() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".trajectory-viewer")
        .join("trash")
}

/// Reject anything that isn't a plain child of the trash root, so a crafted
/// id can never read or delete outside it.
fn entry_dir(id: &str) -> Result<PathBuf, String> {
    let path = Path::new(id);
    if path.components().count() != 1 || id == "." || id == ".." {
        return Err("Invalid trash entry id".to_string());
    }
    Ok(trash_root().join(id))
}

fn entry_id(provider_id: &str, session_id: &str) -> String {
    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() {
        "session".to_string()
    } else {
        safe
    };
    format!("{ts}_{provider_id}_{safe}")
}

/// Move `from` to `to`, falling back to copy+remove when they live on
/// different volumes (rename fails with EXDEV across drives on Windows).
fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(from, to)?;
            if from.is_dir() {
                fs::remove_dir_all(from)
            } else {
                fs::remove_file(from)
            }
        }
    }
}

fn copy_recursive(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to).map(|_| ())
    }
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn write_manifest(dir: &Path, manifest: &TrashManifest) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Cannot serialize trash manifest: {e}"))?;
    fs::write(dir.join(MANIFEST_NAME), raw).map_err(|e| format!("Cannot write trash manifest: {e}"))
}

/// Move the given paths into the trash and record how to put them back.
pub fn trash_paths(
    provider_id: &str,
    session_id: &str,
    title: &str,
    paths: &[PathBuf],
) -> Result<String, String> {
    let id = entry_id(provider_id, session_id);
    let dir = trash_root().join(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create trash entry: {e}"))?;

    let mut items: Vec<TrashItem> = Vec::new();
    for (index, original) in paths.iter().enumerate() {
        if !original.exists() {
            continue;
        }
        let stored_name = format!("item{index}");
        let target = dir.join(&stored_name);
        if let Err(err) = move_path(original, &target) {
            // Roll the moved pieces back so a partial trash never loses data.
            for moved in &items {
                let back = Path::new(&moved.original_path);
                let _ = move_path(&dir.join(&moved.stored_name), back);
            }
            let _ = fs::remove_dir_all(&dir);
            return Err(format!(
                "Cannot move {} to trash: {err}",
                original.display()
            ));
        }
        items.push(TrashItem {
            original_path: original.to_string_lossy().into_owned(),
            stored_name,
        });
    }

    if items.is_empty() {
        let _ = fs::remove_dir_all(&dir);
        return Err("Nothing to delete (paths not found)".to_string());
    }

    write_manifest(
        &dir,
        &TrashManifest {
            provider_id: provider_id.to_string(),
            session_id: session_id.to_string(),
            title: title.to_string(),
            deleted_at: now_ms(),
            kind: "files".to_string(),
            items,
        },
    )?;
    Ok(id)
}

/// Store an opencode SQLite session's rows as a JSON dump in the trash.
pub fn trash_sqlite_dump(
    provider_id: &str,
    session_id: &str,
    title: &str,
    dump: &serde_json::Value,
) -> Result<String, String> {
    let id = entry_id(provider_id, session_id);
    let dir = trash_root().join(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create trash entry: {e}"))?;

    let raw = serde_json::to_string_pretty(dump)
        .map_err(|e| format!("Cannot serialize session dump: {e}"))?;
    fs::write(dir.join(SQLITE_DUMP_NAME), raw)
        .map_err(|e| format!("Cannot write session dump: {e}"))?;

    write_manifest(
        &dir,
        &TrashManifest {
            provider_id: provider_id.to_string(),
            session_id: session_id.to_string(),
            title: title.to_string(),
            deleted_at: now_ms(),
            kind: "opencode-sqlite".to_string(),
            items: Vec::new(),
        },
    )?;
    Ok(id)
}

/// Read one entry's manifest, or `None` when the entry is unreadable.
pub fn read_manifest(id: &str) -> Option<TrashManifest> {
    let dir = entry_dir(id).ok()?;
    let raw = fs::read_to_string(dir.join(MANIFEST_NAME)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// The stored dump for an `opencode-sqlite` entry.
pub fn read_sqlite_dump(id: &str) -> Result<serde_json::Value, String> {
    let dir = entry_dir(id)?;
    let raw = fs::read_to_string(dir.join(SQLITE_DUMP_NAME))
        .map_err(|e| format!("Cannot read trashed session dump: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid trashed session dump: {e}"))
}

/// List trashed sessions, newest first.
pub fn list_trash() -> Vec<TrashEntry> {
    let root = trash_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let Some(manifest) = read_manifest(&id) else {
            continue;
        };
        out.push(TrashEntry {
            id,
            provider_id: manifest.provider_id,
            session_id: manifest.session_id,
            title: manifest.title,
            deleted_at: manifest.deleted_at,
            size_bytes: dir_size(&path),
        });
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.deleted_at));
    out
}

/// Restore an entry: move its files back, or re-insert the row dump.
///
/// Returns the provider id it was restored for.
pub fn restore_entry(id: &str) -> Result<TrashManifest, String> {
    let dir = entry_dir(id)?;
    let manifest = read_manifest(id).ok_or_else(|| format!("Trash entry not found: {id}"))?;

    match manifest.kind.as_str() {
        "files" => {
            // Refuse up-front if anything would be overwritten, so a restore
            // never clobbers a session the user recreated in the meantime.
            for item in &manifest.items {
                let original = Path::new(&item.original_path);
                if original.exists() {
                    return Err(format!(
                        "Cannot restore: {} already exists",
                        original.display()
                    ));
                }
            }
            for item in &manifest.items {
                let stored = dir.join(&item.stored_name);
                if !stored.exists() {
                    continue;
                }
                move_path(&stored, Path::new(&item.original_path))
                    .map_err(|e| format!("Cannot restore {}: {e}", item.original_path))?;
            }
        }
        "opencode-sqlite" => {
            let dump = read_sqlite_dump(id)?;
            crate::trajectory::parser::opencode::restore_session_rows(&dump)?;
        }
        other => return Err(format!("Unknown trash entry kind: {other}")),
    }

    let _ = fs::remove_dir_all(&dir);
    Ok(manifest)
}

/// Permanently remove one trash entry.
pub fn delete_entry(id: &str) -> Result<(), String> {
    let dir = entry_dir(id)?;
    if !dir.is_dir() {
        return Err(format!("Trash entry not found: {id}"));
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("Cannot delete trash entry: {e}"))
}

/// Permanently remove every trash entry; returns how many were removed.
pub fn empty_trash() -> Result<usize, String> {
    let root = trash_root();
    if !root.is_dir() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(&root)
        .map_err(|e| format!("Cannot read trash: {e}"))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if delete_entry(&id).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Drop entries older than `days` (called on startup). Returns the count.
pub fn purge_old_entries(days: i64) -> usize {
    if days <= 0 {
        return 0;
    }
    let cutoff = now_ms() - days * 24 * 3600 * 1000;
    let mut removed = 0;
    for entry in list_trash() {
        if entry.deleted_at > 0 && entry.deleted_at < cutoff && delete_entry(&entry.id).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap();
        let _ = fs::create_dir_all(home);
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", home);
        f();
        match original {
            Some(value) => {
                #[allow(deprecated)]
                std::env::set_var("HOME", value);
            }
            None => {
                #[allow(deprecated)]
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn trash_and_restore_a_file() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let session = home.join(".claude/projects/proj/sess-1.jsonl");
            fs::create_dir_all(session.parent().unwrap()).unwrap();
            fs::write(&session, b"{}\n").unwrap();

            let id = trash_paths(
                "claude",
                "sess-1",
                "My Session",
                std::slice::from_ref(&session),
            )
            .unwrap();
            assert!(!session.exists(), "file was moved out of place");
            assert_eq!(list_trash().len(), 1);

            let manifest = restore_entry(&id).unwrap();
            assert_eq!(manifest.provider_id, "claude");
            assert!(session.exists(), "file came back");
            assert!(list_trash().is_empty(), "entry removed after restore");
        });
    }

    #[test]
    fn trash_and_restore_a_directory() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let session_dir = home.join(".dsh/sessions/proj/sess-1");
            fs::create_dir_all(&session_dir).unwrap();
            fs::write(session_dir.join("session.jsonl.zstd"), b"data").unwrap();

            let id = trash_paths(
                "dsh",
                "sess-1",
                "DSH Session",
                std::slice::from_ref(&session_dir),
            )
            .unwrap();
            assert!(!session_dir.exists());
            restore_entry(&id).unwrap();
            assert!(session_dir.join("session.jsonl.zstd").exists());
        });
    }

    #[test]
    fn restore_refuses_to_overwrite_an_existing_path() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let session = home.join(".claude/projects/proj/sess-1.jsonl");
            fs::create_dir_all(session.parent().unwrap()).unwrap();
            fs::write(&session, b"original").unwrap();

            let id = trash_paths("claude", "sess-1", "t", std::slice::from_ref(&session)).unwrap();
            // A new session appears at the same path before the restore.
            fs::write(&session, b"newer").unwrap();

            let err = restore_entry(&id).unwrap_err();
            assert!(err.contains("already exists"), "got: {err}");
            assert_eq!(fs::read(&session).unwrap(), b"newer");
        });
    }

    #[test]
    fn empty_trash_removes_every_entry() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let a = home.join("a.jsonl");
            let b = home.join("b.jsonl");
            fs::write(&a, b"a").unwrap();
            fs::write(&b, b"b").unwrap();
            trash_paths("claude", "a", "A", &[a]).unwrap();
            trash_paths("claude", "b", "B", &[b]).unwrap();
            assert_eq!(list_trash().len(), 2);
            assert_eq!(empty_trash().unwrap(), 2);
            assert!(list_trash().is_empty());
        });
    }

    #[test]
    fn entry_dir_rejects_traversal() {
        assert!(entry_dir("../escape").is_err());
        assert!(entry_dir("a/b").is_err());
    }

    #[test]
    fn purge_removes_only_old_entries() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let fresh = home.join("fresh.jsonl");
            fs::write(&fresh, b"x").unwrap();
            let id = trash_paths("claude", "fresh", "Fresh", &[fresh]).unwrap();

            // Backdate the manifest well past the cutoff.
            let manifest_path = trash_root().join(&id).join(MANIFEST_NAME);
            let mut manifest: TrashManifest =
                serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
            manifest.deleted_at = now_ms() - 40 * 24 * 3600 * 1000;
            fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

            assert_eq!(purge_old_entries(30), 1);
            assert!(list_trash().is_empty());
        });
    }
}
