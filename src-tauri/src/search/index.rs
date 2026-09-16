// ── Search index (SQLite FTS5) ───────────────────────────────────────────
//
// One row per message, indexed over the tokenized body (see `tokenize`).
// `raw` keeps the original text so result snippets read naturally instead of
// showing the space-separated token form.
//
// `indexed_sessions` tracks which sessions are current so the builder can
// reindex only what changed.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::tokenize;

/// A message row about to be indexed.
pub struct MessageDoc<'a> {
    pub session_key: &'a str,
    pub provider: &'a str,
    pub session_id: &'a str,
    pub role: &'a str,
    pub ts: Option<i64>,
    pub seq: usize,
    pub title: &'a str,
    pub content: &'a str,
}

/// One search result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub session_key: String,
    pub provider: String,
    pub session_id: String,
    pub title: String,
    pub role: String,
    pub ts: Option<i64>,
    /// Original text with the matched region, trimmed to a readable window.
    pub snippet: String,
    /// Character offset of the match within `snippet` (None when the hit came
    /// from the title rather than the body).
    pub match_start: Option<usize>,
    pub match_len: usize,
}

/// Bookkeeping for one indexed session.
#[derive(Debug, Clone)]
pub struct IndexedSession {
    pub mtime: i64,
    pub doc_count: usize,
}

pub fn db_path() -> Option<PathBuf> {
    Some(home_dir()?.join(".trajectory-viewer").join("search.db"))
}

/// The user's home directory (`HOME`, or `USERPROFILE` on Windows).
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Open (creating if needed) the index database.
pub fn open() -> Result<Connection, String> {
    let path = db_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    open_at(&path)
}

/// Open a specific index database — used by tests with a temp path.
pub fn open_at(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create index directory: {e}"))?;
    }
    let conn = Connection::open(path).map_err(|e| format!("Cannot open search index: {e}"))?;
    // WAL lets the background indexer write while the UI searches.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("Cannot enable WAL: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("Cannot set synchronous: {e}"))?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open read-only, for queries when the index may not exist yet.
pub fn open_readonly() -> Result<Connection, String> {
    let path = db_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if !path.exists() {
        return Err("Search index has not been built yet".to_string());
    }
    Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Cannot open search index: {e}"))
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS messages USING fts5(
            body,
            raw         UNINDEXED,
            session_key UNINDEXED,
            provider    UNINDEXED,
            session_id  UNINDEXED,
            role        UNINDEXED,
            ts          UNINDEXED,
            seq         UNINDEXED,
            title       UNINDEXED,
            tokenize = 'unicode61'
        );

        CREATE TABLE IF NOT EXISTS indexed_sessions (
            session_key TEXT PRIMARY KEY,
            source_path TEXT NOT NULL,
            mtime       INTEGER NOT NULL,
            doc_count   INTEGER NOT NULL,
            indexed_at  INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| format!("Cannot create search schema: {e}"))
}

/// Replace every message row for a session.
///
/// The title is indexed alongside the body so a title match is found without
/// a second table; `raw` falls back to the title in that case.
pub fn replace_session(conn: &mut Connection, docs: &[MessageDoc]) -> Result<(), String> {
    let Some(first) = docs.first() else {
        return Ok(());
    };
    let session_key = first.session_key;

    let tx = conn
        .transaction()
        .map_err(|e| format!("Cannot begin index transaction: {e}"))?;

    tx.execute("DELETE FROM messages WHERE session_key = ?1", [session_key])
        .map_err(|e| format!("Cannot clear old index rows: {e}"))?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO messages
                   (body, raw, session_key, provider, session_id, role, ts, seq, title)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(|e| format!("Cannot prepare index insert: {e}"))?;

        for doc in docs {
            // Title is folded into the searchable body so title matches work.
            let body = tokenize::index_text(&format!("{} {}", doc.title, doc.content));
            stmt.execute(rusqlite::params![
                body,
                doc.content,
                doc.session_key,
                doc.provider,
                doc.session_id,
                doc.role,
                doc.ts,
                doc.seq as i64,
                doc.title,
            ])
            .map_err(|e| format!("Cannot insert index row: {e}"))?;
        }
    }

    tx.commit()
        .map_err(|e| format!("Cannot commit index transaction: {e}"))
}

/// Record that a session is indexed at `mtime`.
pub fn set_indexed_session(
    conn: &Connection,
    session_key: &str,
    source_path: &str,
    mtime: i64,
    doc_count: usize,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO indexed_sessions (session_key, source_path, mtime, doc_count, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(session_key) DO UPDATE SET
           source_path = excluded.source_path,
           mtime       = excluded.mtime,
           doc_count   = excluded.doc_count,
           indexed_at  = excluded.indexed_at",
        rusqlite::params![session_key, source_path, mtime, doc_count as i64, now_ms()],
    )
    .map_err(|e| format!("Cannot record indexed session: {e}"))?;
    Ok(())
}

/// Drop a session's rows and its bookkeeping entry.
pub fn remove_session(conn: &Connection, session_key: &str) -> Result<(), String> {
    conn.execute("DELETE FROM messages WHERE session_key = ?1", [session_key])
        .map_err(|e| format!("Cannot delete index rows: {e}"))?;
    conn.execute(
        "DELETE FROM indexed_sessions WHERE session_key = ?1",
        [session_key],
    )
    .map_err(|e| format!("Cannot delete indexed-session row: {e}"))?;
    Ok(())
}

/// Every session currently recorded in the index.
pub fn indexed_sessions(conn: &Connection) -> Result<Vec<(String, IndexedSession)>, String> {
    let mut stmt = conn
        .prepare("SELECT session_key, mtime, doc_count FROM indexed_sessions")
        .map_err(|e| format!("Cannot prepare indexed-session query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                IndexedSession {
                    mtime: row.get(1)?,
                    doc_count: row.get::<_, i64>(2)? as usize,
                },
            ))
        })
        .map_err(|e| format!("Cannot query indexed sessions: {e}"))?;

    Ok(rows.flatten().collect())
}

/// Count of indexed sessions and message rows.
pub fn counts(conn: &Connection) -> Result<(usize, usize), String> {
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM indexed_sessions", [], |r| r.get(0))
        .unwrap_or(0);
    let docs: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
        .unwrap_or(0);
    Ok((sessions as usize, docs as usize))
}

/// Run a search. `providers` empty means "no provider filter".
pub fn search(
    conn: &Connection,
    query: &str,
    providers: &[String],
    limit: usize,
) -> Result<Vec<SearchHit>, String> {
    let Some(expr) = tokenize::query_expr(query) else {
        return Ok(Vec::new());
    };

    // Restrict to the requested providers, when any were given.
    let provider_clause = if providers.is_empty() {
        String::new()
    } else {
        let list = providers
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        format!(" AND provider IN ({list})")
    };

    let sql = format!(
        "SELECT session_key, provider, session_id, title, role, ts, raw
           FROM messages
          WHERE messages MATCH ?1{provider_clause}
          ORDER BY rank
          LIMIT ?2"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Cannot prepare search query: {e}"))?;

    let rows = stmt
        .query_map(rusqlite::params![expr, limit as i64], |row| {
            let raw: String = row.get(6)?;
            Ok((
                SearchHit {
                    session_key: row.get(0)?,
                    provider: row.get(1)?,
                    session_id: row.get(2)?,
                    title: row.get(3)?,
                    role: row.get(4)?,
                    ts: row.get(5)?,
                    snippet: String::new(),
                    match_start: None,
                    match_len: 0,
                },
                raw,
            ))
        })
        .map_err(|e| format!("Cannot run search query: {e}"))?;

    let mut hits = Vec::new();
    for (mut hit, raw) in rows.flatten() {
        let (snippet, start, len) = super::snippet::build(&raw, &hit.title, query);
        hit.snippet = snippet;
        hit.match_start = start;
        hit.match_len = len;
        hits.push(hit);
    }
    Ok(hits)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn temp_conn() -> (tempfile::TempDir, Connection) {
        let dir = tempdir().unwrap();
        let conn = open_at(&dir.path().join("search.db")).unwrap();
        (dir, conn)
    }

    fn doc<'a>(key: &'a str, role: &'a str, seq: usize, content: &'a str) -> MessageDoc<'a> {
        MessageDoc {
            session_key: key,
            provider: "claude",
            session_id: "s1",
            role,
            ts: Some(1_700_000_000_000),
            seq,
            title: "My Session",
            content,
        }
    }

    #[test]
    fn indexes_and_finds_chinese_two_char_terms() {
        let (_dir, mut conn) = temp_conn();
        replace_session(
            &mut conn,
            &[doc("claude::s1", "user", 1, "修复轨迹查看器的白屏问题")],
        )
        .unwrap();

        for q in ["白屏", "轨迹", "查看器"] {
            let hits = search(&conn, q, &[], 10).unwrap();
            assert_eq!(hits.len(), 1, "query {q:?} should match");
            assert!(
                hits[0].snippet.contains(q),
                "snippet: {:?}",
                hits[0].snippet
            );
        }
    }

    #[test]
    fn indexes_and_finds_english() {
        let (_dir, mut conn) = temp_conn();
        replace_session(
            &mut conn,
            &[doc("claude::s1", "user", 1, "fix the white screen")],
        )
        .unwrap();

        assert_eq!(search(&conn, "white", &[], 10).unwrap().len(), 1);
        assert_eq!(search(&conn, "screen", &[], 10).unwrap().len(), 1);
        assert_eq!(search(&conn, "missing", &[], 10).unwrap().len(), 0);
    }

    #[test]
    fn matches_the_session_title() {
        let (_dir, mut conn) = temp_conn();
        let mut d = doc("claude::s1", "user", 1, "unrelated body text");
        d.title = "重构计划";
        replace_session(&mut conn, &[d]).unwrap();

        let hits = search(&conn, "重构", &[], 10).unwrap();
        assert_eq!(hits.len(), 1, "title match should be found");
        assert_eq!(hits[0].title, "重构计划");
    }

    #[test]
    fn all_terms_must_match() {
        let (_dir, mut conn) = temp_conn();
        replace_session(
            &mut conn,
            &[
                doc("claude::s1", "user", 1, "白屏问题"),
                doc("claude::s2", "user", 1, "构建问题"),
            ],
        )
        .unwrap();

        assert_eq!(search(&conn, "白屏 问题", &[], 10).unwrap().len(), 1);
        assert_eq!(search(&conn, "白屏 构建", &[], 10).unwrap().len(), 0);
    }

    #[test]
    fn filters_by_provider() {
        let (_dir, mut conn) = temp_conn();
        let mut a = doc("claude::s1", "user", 1, "shared keyword");
        a.provider = "claude";
        let mut b = doc("codex::s2", "user", 1, "shared keyword");
        b.provider = "codex";
        replace_session(&mut conn, &[a]).unwrap();
        replace_session(&mut conn, &[b]).unwrap();

        assert_eq!(search(&conn, "keyword", &[], 10).unwrap().len(), 2);
        assert_eq!(
            search(&conn, "keyword", &["claude".to_string()], 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn replacing_a_session_does_not_duplicate_rows() {
        let (_dir, mut conn) = temp_conn();
        replace_session(&mut conn, &[doc("claude::s1", "user", 1, "hello world")]).unwrap();
        replace_session(&mut conn, &[doc("claude::s1", "user", 1, "hello world")]).unwrap();
        assert_eq!(search(&conn, "hello", &[], 10).unwrap().len(), 1);
    }

    #[test]
    fn removing_a_session_drops_its_hits() {
        let (_dir, mut conn) = temp_conn();
        replace_session(&mut conn, &[doc("claude::s1", "user", 1, "hello world")]).unwrap();
        set_indexed_session(&conn, "claude::s1", "/p", 1, 1).unwrap();
        assert_eq!(search(&conn, "hello", &[], 10).unwrap().len(), 1);

        remove_session(&conn, "claude::s1").unwrap();
        assert_eq!(search(&conn, "hello", &[], 10).unwrap().len(), 0);
        assert!(indexed_sessions(&conn).unwrap().is_empty());
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let (_dir, mut conn) = temp_conn();
        replace_session(&mut conn, &[doc("claude::s1", "user", 1, "hello")]).unwrap();
        assert!(search(&conn, "   ", &[], 10).unwrap().is_empty());
    }

    #[test]
    fn counts_report_index_size() {
        let (_dir, mut conn) = temp_conn();
        replace_session(
            &mut conn,
            &[
                doc("claude::s1", "user", 1, "a"),
                doc("claude::s1", "assistant", 2, "b"),
            ],
        )
        .unwrap();
        set_indexed_session(&conn, "claude::s1", "/p", 1, 2).unwrap();
        assert_eq!(counts(&conn).unwrap(), (1, 2));
    }
}
