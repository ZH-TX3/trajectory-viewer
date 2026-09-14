// ── Session Provider abstraction ─────────────────────────────────────────
//
// Every supported tool implements `SessionProvider`. Adding a tool means
// writing one file in this directory and adding it to `PROVIDERS` below —
// scanning, message loading, deletion and the UI's provider list all pick it
// up from there.
//
// A provider is a zero-sized type (no state); the registry holds statics.

use std::path::{Path, PathBuf};

use super::{SessionMessage, SessionMeta};

/// One AI tool whose sessions this app can browse.
pub trait SessionProvider: Sync {
    /// Stable identifier used across the frontend and the settings UI
    /// (`"claude"`, `"codex"`, `"dsh"`, `"opencode"`).
    fn id(&self) -> &'static str;

    /// Root directory holding this tool's sessions, when the tool is
    /// installed. `None` disables the provider.
    fn sessions_dir(&self) -> Option<PathBuf>;

    /// Parse one session file into list metadata. Returning `None` skips it.
    fn parse_session(&self, path: &Path) -> Option<SessionMeta>;

    /// Load the messages shown in the Messages tab.
    fn load_messages(&self, source: &str) -> Result<Vec<SessionMessage>, String>;

    /// Move one session into the trash, returning the trash entry id.
    fn trash_session(&self, source: &str) -> Result<String, String>;

    /// Trash every session under a project directory, returning the count.
    fn trash_sessions_in_dir(&self, dir: &Path) -> Result<usize, String>;

    /// Whether this provider recognizes `source` as one of its sessions.
    ///
    /// Needed because a source isn't always a path — OpenCode's SQLite
    /// sessions are referenced as `sqlite:<db>:<session_id>`, which no path
    /// prefix check can attribute. The default handles the file-backed case.
    fn owns_source(&self, source: &str) -> bool {
        match self.sessions_dir() {
            Some(root) => Path::new(source).starts_with(root),
            None => false,
        }
    }

    /// The file extensions that make up a session, used when walking
    /// `sessions_dir` to find candidates. `None` means the provider scans
    /// something other than plain files (e.g. a database) and supplies its
    /// own session list via `scan`.
    fn file_extensions(&self) -> Option<&'static [&'static str]> {
        Some(&["jsonl"])
    }

    /// List sessions directly, for providers whose storage isn't a plain file
    /// tree. The default walks `sessions_dir` and calls `parse_session`.
    fn scan(&self) -> Vec<SessionMeta> {
        let Some(dir) = self.sessions_dir() else {
            return Vec::new();
        };
        let Some(exts) = self.file_extensions() else {
            return Vec::new();
        };
        let mut files = Vec::new();
        super::utils::collect_files(&dir, exts, &mut files);
        files.iter().filter_map(|p| self.parse_session(p)).collect()
    }
}

/// Every registered provider, in display order.
///
/// To add a tool: implement `SessionProvider` in its own module and append it
/// here. Nothing else in the app needs to change.
pub fn providers() -> &'static [&'static dyn SessionProvider] {
    static PROVIDERS: &[&dyn SessionProvider] = &[
        &super::claude::ClaudeProvider,
        &super::codex::CodexProvider,
        &super::dsh::DshProvider,
        &super::opencode::OpenCodeProvider,
    ];
    PROVIDERS
}

/// Look up a provider by its id.
pub fn provider(id: &str) -> Option<&'static dyn SessionProvider> {
    providers().iter().copied().find(|p| p.id() == id)
}

/// The ids of every registered provider, in display order.
#[cfg_attr(not(test), allow(dead_code))]
pub fn provider_ids() -> Vec<&'static str> {
    providers().iter().map(|p| p.id()).collect()
}
