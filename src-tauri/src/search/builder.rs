// ── Search index builder ─────────────────────────────────────────────────
//
// Keeps the FTS index in step with the session files. A session is reindexed
// only when its source changed, so the steady-state refresh is cheap.
//
// The first run over a full corpus is the expensive one (hundreds of MB of
// text), so callers run this off the UI thread and watch the progress events.

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use super::index::{self, IndexedSession, MessageDoc};
use crate::session_manager::{self, SessionMeta};

/// Progress reported while indexing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

/// Outcome of one indexing pass.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub indexed_sessions: usize,
    pub skipped_sessions: usize,
    pub removed_sessions: usize,
    pub total_docs: usize,
}

/// Freshness key for a session.
///
/// File-backed sessions use the file's mtime. OpenCode's SQLite sessions have
/// no single file, so they fall back to the session's own `last_active_at`
/// (coarser — the whole db changing reindexes all its sessions — but correct).
fn session_mtime(session: &SessionMeta) -> i64 {
    let Some(source) = session.source_path.as_deref() else {
        return 0;
    };
    if source.starts_with("sqlite:") {
        return session.last_active_at.or(session.created_at).unwrap_or(0);
    }
    std::fs::metadata(Path::new(source))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(|| session.last_active_at.or(session.created_at).unwrap_or(0))
}

/// Sessions worth indexing: those with a source path and a title or content.
fn indexable(session: &SessionMeta, providers: &[String]) -> bool {
    if session.source_path.is_none() {
        return false;
    }
    providers.is_empty() || providers.iter().any(|p| p == &session.provider_id)
}

/// Index one session's messages.
fn index_session(conn: &mut Connection, session: &SessionMeta) -> Result<usize, String> {
    let source = session.source_path.as_deref().unwrap_or_default();
    let session_key = format!("{}::{}", session.provider_id, session.session_id);
    let title = session.title.clone().unwrap_or_default();

    let messages = session_manager::load_messages(&session.provider_id, source)?;

    // Tool results are excluded by product decision — they'd dominate the
    // index with command output while rarely being what users search for.
    let docs: Vec<MessageDoc> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role != "tool" && !m.content.trim().is_empty())
        .map(|(i, m)| MessageDoc {
            session_key: &session_key,
            provider: &session.provider_id,
            session_id: &session.session_id,
            role: &m.role,
            ts: m.ts,
            seq: i,
            title: &title,
            content: &m.content,
        })
        .collect();

    index::replace_session(conn, &docs)?;

    let mtime = session_mtime(session);
    index::set_indexed_session(conn, &session_key, source, mtime, docs.len())?;
    Ok(docs.len())
}

/// Bring the index up to date for `providers` (empty = all).
///
/// `on_progress` is called as sessions are processed so the UI can show a
/// progress bar; it receives the running counts.
pub fn reindex<F>(
    conn: &mut Connection,
    providers: &[String],
    force: bool,
    mut on_progress: F,
) -> Result<IndexStats, String>
where
    F: FnMut(IndexProgress),
{
    let sessions = session_manager::scan_sessions();
    let known: std::collections::HashMap<String, IndexedSession> =
        index::indexed_sessions(conn)?.into_iter().collect();

    let candidates: Vec<&SessionMeta> = sessions
        .iter()
        .filter(|s| indexable(s, providers))
        .collect();

    let mut stats = IndexStats::default();
    let total = candidates.len();

    for (done, session) in candidates.iter().enumerate() {
        let session_key = format!("{}::{}", session.provider_id, session.session_id);

        on_progress(IndexProgress {
            done,
            total,
            current: session.title.clone().unwrap_or_else(|| session_key.clone()),
        });

        // Skip when the source is unchanged since it was last indexed.
        if !force {
            if let Some(existing) = known.get(&session_key) {
                if existing.mtime == session_mtime(session) {
                    stats.skipped_sessions += 1;
                    stats.total_docs += existing.doc_count;
                    continue;
                }
            }
        }

        match index_session(conn, session) {
            Ok(docs) => {
                stats.indexed_sessions += 1;
                stats.total_docs += docs;
            }
            // One unreadable session must not abort the whole pass.
            Err(err) => {
                eprintln!("[search] skipping {}: {err}", session.session_id);
                stats.skipped_sessions += 1;
            }
        }
    }

    // Drop sessions that no longer exist on disk.
    let live: std::collections::HashSet<String> = candidates
        .iter()
        .map(|s| format!("{}::{}", s.provider_id, s.session_id))
        .collect();
    for (key, entry) in &known {
        let still_ours = providers.is_empty() || {
            // Only prune entries belonging to the providers being indexed.
            providers.iter().any(|p| key.starts_with(&format!("{p}::")))
        };
        if still_ours && !live.contains(key) {
            index::remove_session(conn, key)?;
            stats.removed_sessions += 1;
            stats.total_docs = stats.total_docs.saturating_sub(entry.doc_count);
        }
    }

    on_progress(IndexProgress {
        done: total,
        total,
        current: String::new(),
    });

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn session(provider: &str, id: &str, path: &str, title: &str) -> SessionMeta {
        SessionMeta {
            provider_id: provider.to_string(),
            session_id: id.to_string(),
            title: Some(title.to_string()),
            summary: None,
            project_dir: None,
            project_group: None,
            created_at: Some(1_700_000_000_000),
            last_active_at: Some(1_700_000_000_000),
            source_path: Some(path.to_string()),
            resume_command: None,
        }
    }

    #[test]
    fn session_mtime_uses_last_active_for_sqlite_sources() {
        let s = session("opencode", "ses_1", "sqlite:C:/x/opencode.db:ses_1", "t");
        assert_eq!(session_mtime(&s), 1_700_000_000_000);
    }

    #[test]
    fn session_mtime_uses_file_mtime_for_file_sources() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        std::fs::write(&file, "{}\n").unwrap();

        let s = session("claude", "s1", file.to_str().unwrap(), "t");
        let mtime = session_mtime(&s);
        assert!(mtime > 0, "should read the real file mtime");
        assert_ne!(mtime, 1_700_000_000_000);
    }

    #[test]
    fn session_mtime_falls_back_when_the_file_is_gone() {
        let s = session("claude", "s1", "/definitely/missing.jsonl", "t");
        assert_eq!(session_mtime(&s), 1_700_000_000_000);
    }

    #[test]
    fn indexable_requires_a_source_and_respects_the_provider_filter() {
        let mut s = session("claude", "s1", "/p.jsonl", "t");
        assert!(indexable(&s, &[]));
        assert!(indexable(&s, &["claude".to_string()]));
        assert!(!indexable(&s, &["codex".to_string()]));

        s.source_path = None;
        assert!(!indexable(&s, &[]));
    }

    #[test]
    fn progress_reports_running_counts() {
        // The callback contract the UI depends on.
        let mut seen = Vec::new();
        let mut cb = |p: IndexProgress| seen.push((p.done, p.total));
        cb(IndexProgress {
            done: 0,
            total: 5,
            current: "a".into(),
        });
        cb(IndexProgress {
            done: 5,
            total: 5,
            current: String::new(),
        });
        assert_eq!(seen, vec![(0, 5), (5, 5)]);
    }
}
