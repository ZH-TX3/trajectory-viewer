// ── OpenCode sessions ────────────────────────────────────────────────────
//
// OpenCode keeps sessions in two places and the parser module already owns
// both layouts, so this provider is a thin adapter: scanning and message
// loading delegate to `trajectory::parser::opencode`, which knows the flat
// JSON files and the SQLite database.

use std::path::{Path, PathBuf};

use crate::session_manager::provider::SessionProvider;
use crate::session_manager::{SessionMessage, SessionMeta};
use crate::trajectory::parser::opencode as oc;

pub struct OpenCodeProvider;

impl SessionProvider for OpenCodeProvider {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn sessions_dir(&self) -> Option<PathBuf> {
        oc::get_opencode_base_dir().map(|base| base.join("storage"))
    }

    /// OpenCode supplies its own session list (JSON storage + SQLite), so the
    /// default file walk doesn't apply.
    fn file_extensions(&self) -> Option<&'static [&'static str]> {
        None
    }

    fn scan(&self) -> Vec<SessionMeta> {
        oc::scan_sessions()
    }

    fn parse_session(&self, _path: &Path) -> Option<SessionMeta> {
        // Sessions are discovered by `scan`; individual file parsing happens
        // inside the opencode module.
        None
    }

    fn load_messages(&self, source: &str) -> Result<Vec<SessionMessage>, String> {
        oc::load_messages(source)
    }

    /// OpenCode sessions are either a JSON file under `storage/` or a
    /// `sqlite:<db>:<session_id>` reference, so the default path-prefix check
    /// can't attribute the latter.
    fn owns_source(&self, source: &str) -> bool {
        if source.starts_with("sqlite:") {
            return true;
        }
        self.sessions_dir()
            .is_some_and(|root| std::path::Path::new(source).starts_with(root))
    }

    fn trash_session(&self, source: &str) -> Result<String, String> {
        oc::trash_session(source)
    }

    fn trash_sessions_in_dir(&self, dir: &Path) -> Result<usize, String> {
        oc::delete_sessions_in_dir(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_stable() {
        assert_eq!(OpenCodeProvider.id(), "opencode");
    }

    #[test]
    fn scan_is_delegated_not_file_based() {
        // No default file walk: opencode storage is a directory tree plus a db.
        assert!(OpenCodeProvider.file_extensions().is_none());
    }
}
