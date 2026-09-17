// ── Session Manager ──────────────────────────────────────────────────────
//
// Discovers, reads and trashes AI tool sessions. Each tool lives in its own
// module implementing `SessionProvider`; this module owns the shared types,
// the dispatch entry points, and the delete/trash policy that applies to
// every file-backed provider.
//
// Adding a tool:
//   1. create `session_manager/<tool>.rs` with a `SessionProvider` impl
//   2. register it in `provider::providers()`
//   3. add a `mod` line below
// Everything else (scanning, messages, trash, the UI's tool list) follows.

mod claude;
mod codex;
mod dsh;
mod opencode;
mod provider;
mod utils;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use provider::{provider, providers, SessionProvider};

/// Root directories session deletion is allowed inside.
fn managed_session_dirs() -> Vec<PathBuf> {
    providers()
        .iter()
        .filter_map(|p| p.sessions_dir())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

/// Scan every registered provider and merge the results, newest first.
pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions: Vec<SessionMeta> = providers().iter().flat_map(|p| p.scan()).collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active_at.or(s.created_at).unwrap_or(0)));
    sessions
}

/// Load the messages of one session.
pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    let provider =
        provider(provider_id).ok_or_else(|| format!("Unsupported provider: {provider_id}"))?;
    provider.load_messages(source_path)
}

// ── Deletion ─────────────────────────────────────────────────────────────

/// Trash a session identified by `source_path`.
///
/// Returns the trash entry id so the caller can offer an undo.
pub fn delete_session_file(source_path: &str) -> Result<String, String> {
    // A source isn't always a filesystem path (OpenCode's SQLite sessions are
    // `sqlite:<db>:<id>`), so let each provider claim it.
    let provider = providers()
        .iter()
        .find(|p| p.owns_source(source_path))
        .ok_or_else(|| "Refusing to delete outside managed session directories".to_string())?;
    provider.trash_session(source_path)
}

/// Trash every session in a project directory (all its conversations).
pub fn delete_sessions_in_dir(dir: &str) -> Result<usize, String> {
    let path = Path::new(dir);
    if !path.is_dir() {
        return Err("Not a directory".to_string());
    }

    // Allow any directory strictly inside a managed root (project dirs), but
    // never the managed roots themselves.
    let managed = managed_session_dirs();
    if !managed
        .iter()
        .any(|root| path.starts_with(root) && path != root)
    {
        return Err("Refusing to delete outside managed session directories".to_string());
    }

    // Delegate to whichever provider owns this directory. OpenCode's storage
    // layout nests differently, so ask it first.
    for p in providers() {
        if p.id() == "opencode" && crate::trajectory::parser::opencode::is_session_store_dir(path) {
            return p.trash_sessions_in_dir(path);
        }
    }

    let provider_id = provider_of_path(path)
        .ok_or_else(|| "Refusing to delete outside managed session directories".to_string())?;
    let provider =
        provider(provider_id).ok_or_else(|| format!("Unsupported provider: {provider_id}"))?;
    provider.trash_sessions_in_dir(path)
}

/// Which provider owns `path`, based on the managed root it sits under.
///
/// Returns `None` for paths outside every managed directory, so callers can
/// refuse rather than act on an arbitrary file.
pub fn provider_of_path(path: &Path) -> Option<&'static str> {
    providers()
        .iter()
        .find(|p| p.sessions_dir().is_some_and(|root| path.starts_with(root)))
        .map(|p| p.id())
}

/// Trash a session that lives in a plain file (every provider but OpenCode's
/// SQLite store). Shared so each provider's `trash_session` stays one line.
pub fn delete_file_backed_session(provider_id: &str, path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let provider =
        provider(provider_id).ok_or_else(|| format!("Unsupported provider: {provider_id}"))?;
    let allowed = provider.file_extensions().unwrap_or(&[]);
    if !allowed.contains(&ext.as_str()) {
        return Err("Refusing to delete non-session file".to_string());
    }

    // Prefer the parsed metadata so the trash list shows the real conversation
    // title instead of the file name (a uuid/hash for Claude). Fall back to the
    // file name only when the file can't be parsed (e.g. a partial write).
    let metadata = provider.parse_session(path);
    let fallback = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("session")
        .to_string();
    let session_id = metadata
        .as_ref()
        .map(|m| m.session_id.clone())
        .unwrap_or_else(|| fallback.clone());
    let title = metadata
        .as_ref()
        .and_then(|m| m.title.clone())
        .unwrap_or(fallback);

    utils::trash_file_session(provider_id, &session_id, &title, path)
}

/// Trash every session file under `dir` for a file-backed provider.
pub fn trash_files_in_dir(provider_id: &str, dir: &Path) -> Result<usize, String> {
    let provider =
        provider(provider_id).ok_or_else(|| format!("Unsupported provider: {provider_id}"))?;
    let exts = provider.file_extensions().unwrap_or(&[]);

    let mut files = Vec::new();
    utils::collect_files(dir, exts, &mut files);

    let mut trashed = 0;
    for file in &files {
        if delete_file_backed_session(provider_id, file).is_ok() {
            trashed += 1;
        }
    }
    remove_empty_dirs(dir);
    Ok(trashed)
}

/// Remove directories bottom-up (succeeds only when they are empty).
fn remove_empty_dirs(dir: &Path) {
    fn walk(p: &Path) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for entry in rd.flatten() {
                if entry.path().is_dir() {
                    walk(&entry.path());
                }
            }
        }
        let _ = std::fs::remove_dir(p);
    }
    walk(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::provider::provider_ids;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap();
        std::fs::create_dir_all(home).unwrap();
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
    fn every_registered_provider_has_a_unique_id() {
        let ids = provider_ids();
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"dsh"));
        assert!(ids.contains(&"opencode"));
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "provider ids must be unique");
    }

    #[test]
    fn provider_lookup_finds_registered_and_rejects_unknown() {
        assert_eq!(provider("claude").map(|p| p.id()), Some("claude"));
        assert!(provider("nope").is_none());
    }

    #[test]
    fn load_messages_rejects_an_unknown_provider() {
        let err = load_messages("nope", "/tmp/x").unwrap_err();
        assert!(err.contains("Unsupported provider"), "got: {err}");
    }

    #[test]
    fn delete_trashes_with_the_parsed_title_not_the_file_name() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let file = home.join(".claude/projects/proj/abc-123.jsonl");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(
                &file,
                concat!(
                    r#"{"sessionId":"abc-123","timestamp":"2026-03-06T10:00:00Z"}"#,
                    "\n",
                    r#"{"type":"custom-title","customTitle":"Refactor the parser"}"#,
                ),
            )
            .unwrap();

            let id = delete_file_backed_session("claude", &file).unwrap();
            let manifest = crate::trash::read_manifest(&id).expect("manifest exists");
            assert_eq!(manifest.title, "Refactor the parser");
            assert_eq!(manifest.session_id, "abc-123");
        });
    }

    #[test]
    fn delete_falls_back_to_the_file_name_when_unparseable() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let file = home.join(".claude/projects/proj/xyz.jsonl");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"definitely-not-json\n").unwrap();

            let id = delete_file_backed_session("claude", &file).unwrap();
            let manifest = crate::trash::read_manifest(&id).expect("manifest exists");
            assert_eq!(manifest.title, "xyz.jsonl");
        });
    }

    #[test]
    fn provider_of_path_attributes_each_tool() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            // Roots must exist for sessions_dir() to report them.
            for sub in [".claude/projects", ".codex/sessions", ".dsh/sessions"] {
                std::fs::create_dir_all(home.join(sub)).unwrap();
            }
            assert_eq!(
                provider_of_path(&home.join(".claude/projects/p/s.jsonl")),
                Some("claude")
            );
            assert_eq!(
                provider_of_path(&home.join(".codex/sessions/2026/01/01/r.jsonl")),
                Some("codex")
            );
            assert_eq!(
                provider_of_path(&home.join(".dsh/sessions/p/s/session.jsonl.zstd")),
                Some("dsh")
            );
            assert_eq!(provider_of_path(Path::new("/tmp/elsewhere.jsonl")), None);
        });
    }

    #[test]
    fn delete_session_file_moves_to_trash_and_restores() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let session = home.join(".claude/projects/proj/sess-1.jsonl");
            std::fs::create_dir_all(session.parent().unwrap()).unwrap();
            std::fs::write(&session, b"{\"sessionId\":\"sess-1\"}\n").unwrap();

            let trash_id = delete_session_file(&session.to_string_lossy()).unwrap();
            assert!(!session.exists(), "session moved out");
            assert_eq!(crate::trash::list_trash().len(), 1);

            let manifest = crate::trash::restore_entry(&trash_id).unwrap();
            assert_eq!(manifest.provider_id, "claude");
            assert_eq!(manifest.session_id, "sess-1");
            assert_eq!(
                std::fs::read(&session).unwrap(),
                b"{\"sessionId\":\"sess-1\"}\n",
                "restored byte-for-byte"
            );
        });
    }

    // A DSH session is a directory; trashing must take the whole thing so the
    // restore leaves no orphaned payload.
    #[test]
    fn delete_dsh_session_trashes_its_directory() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let session_dir = home.join(".dsh/sessions/proj/ses-9");
            std::fs::create_dir_all(&session_dir).unwrap();
            let zstd = session_dir.join("session.jsonl.zstd");
            std::fs::write(&zstd, b"compressed").unwrap();

            delete_session_file(&zstd.to_string_lossy()).unwrap();
            assert!(!session_dir.exists(), "whole session dir moved");

            let entry = crate::trash::list_trash().into_iter().next().unwrap();
            crate::trash::restore_entry(&entry.id).unwrap();
            assert!(zstd.exists(), "zstd file restored inside its dir");
        });
    }

    #[test]
    fn delete_refuses_paths_outside_managed_dirs() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let outsider = home.join("somewhere-else/sess.jsonl");
            std::fs::create_dir_all(outsider.parent().unwrap()).unwrap();
            std::fs::write(&outsider, b"{}\n").unwrap();

            let err = delete_session_file(&outsider.to_string_lossy()).unwrap_err();
            assert!(err.contains("outside managed"), "got: {err}");
            assert!(outsider.exists(), "untouched");
        });
    }

    #[test]
    fn delete_refuses_non_session_extensions() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let txt = home.join(".claude/projects/proj/notes.txt");
            std::fs::create_dir_all(txt.parent().unwrap()).unwrap();
            std::fs::write(&txt, b"x").unwrap();

            let err = delete_session_file(&txt.to_string_lossy()).unwrap_err();
            assert!(err.contains("non-session"), "got: {err}");
            assert!(txt.exists());
        });
    }

    // Regression: an OpenCode SQLite session is referenced as
    // `sqlite:<db>:<session_id>`, which is not a path — a path-prefix check
    // can't attribute it, so such sessions became undeletable.
    #[test]
    fn opencode_sqlite_source_is_owned_by_the_opencode_provider() {
        let oc = provider("opencode").unwrap();
        let source = "sqlite:C:\\Users\\x\\.local\\share\\opencode\\opencode.db:ses_abc";
        assert!(oc.owns_source(source), "sqlite reference must be claimed");
        // And no other provider should claim it.
        for p in providers() {
            if p.id() != "opencode" {
                assert!(!p.owns_source(source), "{} claimed a sqlite source", p.id());
            }
        }
    }

    #[test]
    fn a_path_outside_every_root_is_claimed_by_no_provider() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let outsider = home
                .join("elsewhere/sess.jsonl")
                .to_string_lossy()
                .into_owned();
            assert!(providers().iter().all(|p| !p.owns_source(&outsider)));
        });
    }
}
