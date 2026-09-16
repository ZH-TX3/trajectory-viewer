// ── Cross-session search ─────────────────────────────────────────────────
//
// Search every indexed session's messages, not just the one on screen. The
// index is a local SQLite FTS5 database under ~/.trajectory-viewer/search.db,
// kept fresh incrementally by `builder`.
//
// Layout:
//   tokenize.rs  CJK-aware tokenization (index and query share it)
//   index.rs     schema, insert/delete/query
//   snippet.rs   window the matched region out of the ORIGINAL text
//   builder.rs   incremental refresh + progress

mod builder;
mod index;
mod snippet;
mod tokenize;

use serde::Serialize;

pub use builder::{IndexProgress, IndexStats};
pub use index::SearchHit;

/// How many hits a single query may return.
const DEFAULT_LIMIT: usize = 200;

/// Current state of the index, for the settings panel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub indexed_sessions: usize,
    pub total_docs: usize,
    pub exists: bool,
}

/// Run a search across every indexed session.
pub fn search(
    query: &str,
    providers: &[String],
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    let conn = index::open_readonly()?;
    index::search(&conn, query, providers, limit.unwrap_or(DEFAULT_LIMIT))
}

/// Index status (session and document counts).
pub fn status() -> IndexStatus {
    match index::open_readonly() {
        Ok(conn) => match index::counts(&conn) {
            Ok((sessions, docs)) => IndexStatus {
                indexed_sessions: sessions,
                total_docs: docs,
                exists: true,
            },
            Err(_) => IndexStatus {
                indexed_sessions: 0,
                total_docs: 0,
                exists: true,
            },
        },
        Err(_) => IndexStatus {
            indexed_sessions: 0,
            total_docs: 0,
            exists: false,
        },
    }
}

/// Rebuild/refresh the index for `providers` (empty = all).
///
/// `on_progress` receives running counts so a caller can stream them to the
/// UI; pass a no-op when progress isn't needed.
pub fn reindex<F>(providers: &[String], force: bool, on_progress: F) -> Result<IndexStats, String>
where
    F: FnMut(IndexProgress),
{
    let mut conn = index::open()?;
    builder::reindex(&mut conn, providers, force, on_progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_reported_even_without_an_index() {
        // Must not panic or error when the index has never been built.
        let s = status();
        assert!(s.total_docs == 0 || s.exists);
    }

    #[test]
    fn search_on_a_missing_index_errors_gracefully() {
        // Either an explicit error or an empty result is acceptable; it must
        // not panic.
        match search("anything", &[], None) {
            Ok(hits) => assert!(hits.len() < 10_000),
            Err(msg) => assert!(!msg.is_empty()),
        }
    }

    /// Real-data smoke test: build the index over the actual session corpus
    /// and search for known terms (run with `--ignored`).
    #[test]
    #[ignore]
    fn search_real_corpus() {
        let t0 = std::time::Instant::now();
        let stats = reindex(&[], false, |_| {}).unwrap();
        let build_ms = t0.elapsed().as_millis();

        let mut out = String::new();
        out.push_str(&format!(
            "index build: {build_ms} ms, {} sessions ({} skipped, {} removed), {} docs\n",
            stats.indexed_sessions,
            stats.skipped_sessions,
            stats.removed_sessions,
            stats.total_docs
        ));

        let st = status();
        out.push_str(&format!(
            "status: exists={} sessions={} docs={}\n",
            st.exists, st.indexed_sessions, st.total_docs
        ));

        if let Some(path) = index::db_path() {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("index size: {:.1} MB\n", size as f64 / 1e6));
        }

        // Query terms taken from real session content.
        for q in ["白屏", "轨迹", "opencode", "backup", "测试"] {
            match search(q, &[], Some(5)) {
                Ok(hits) => {
                    out.push_str(&format!("query {q:?}: {} hit(s)\n", hits.len()));
                    if let Some(h) = hits.first() {
                        out.push_str(&format!(
                            "   [{}] {} :: {}\n",
                            h.provider,
                            h.title,
                            h.snippet.chars().take(70).collect::<String>()
                        ));
                    }
                }
                Err(e) => out.push_str(&format!("query {q:?}: ERROR {e}\n")),
            }
        }

        // Second run must be a cheap no-op (mtime cache works).
        let t1 = std::time::Instant::now();
        let again = reindex(&[], false, |_| {}).unwrap();
        out.push_str(&format!(
            "second (incremental) run: {} ms, {} skipped\n",
            t1.elapsed().as_millis(),
            again.skipped_sessions
        ));

        let _ = std::fs::write(std::env::temp_dir().join("search_smoke.txt"), out);
    }
}
