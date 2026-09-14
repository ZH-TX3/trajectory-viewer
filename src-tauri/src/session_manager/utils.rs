// ── Session Manager: shared helpers ──────────────────────────────────────
//
// Small utilities every provider needs: locating the home directory, walking
// a session tree, and the text/truncation conventions the list view uses.

use std::path::{Path, PathBuf};

/// The user's home directory (`HOME`, or `USERPROFILE` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Collect every file under `root` whose extension is one of `exts`.
pub fn collect_files(root: &Path, exts: &[&str], files: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, exts, files);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if exts.contains(&ext) {
                files.push(path);
            }
        }
    }
}

/// Flatten a provider's message content into plain text for the list view.
///
/// Handles the shapes the JSONL providers use: a bare string, an array of
/// typed blocks (text / tool_use / tool_result), or an object with text.
///
/// Note the recursion: a message object's `content` is itself usually an
/// array of blocks (Claude Code's real format), so an object must delegate to
/// the array branch rather than only accepting a string `content`.
pub fn extract_text(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if item_type == "tool_use" {
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    Some(format!("[Tool: {name}]"))
                } else if item_type == "tool_result" {
                    item.get("content").map(extract_text)
                } else {
                    item.get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| item.get("content").map(extract_text))
                }
            })
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            // Prefer an explicit `text`; otherwise recurse into `content`
            // (which is an array of blocks for most providers).
            match map.get("text").and_then(|v| v.as_str()) {
                Some(text) => text.to_string(),
                None => map.get("content").map(extract_text).unwrap_or_default(),
            }
        }
        _ => String::new(),
    }
}

/// Trim to `max_chars`, appending an ellipsis when truncated.
pub fn truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut result: String = trimmed.chars().take(max_chars).collect();
    result.push_str("...");
    result
}

/// Decode a project directory name like `D--DSH` → `D:/DSH`.
///
/// Both Claude Code and DSH encode absolute paths this way.
pub fn decode_project_name(name: &str) -> String {
    let mut result = name.to_string();
    if let Some(idx) = result.find("--") {
        let drive = &result[..idx];
        let rest = &result[idx + 2..];
        result = format!("{}:/{}", drive, rest.replace("--", "/").replace("---", "/"));
    }
    result
}

/// The last path segment of a directory, used to label a project group.
pub fn dir_basename(dir: &str) -> Option<String> {
    dir.trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Whether `dir` is a per-session directory (the DSH layout), i.e. it holds
/// the session payload itself rather than more sessions.
///
/// Trashing such a session must take the whole directory, or restoring it
/// would leave an orphaned payload behind.
pub fn is_session_dir(dir: &Path) -> bool {
    dir.join("session.jsonl.zstd").is_file()
}

/// Trash a file-backed session, taking its enclosing session directory when
/// the layout uses one. Shared by the file-based providers.
pub fn trash_file_session(
    provider_id: &str,
    session_id: &str,
    title: &str,
    path: &Path,
) -> Result<String, String> {
    let target = match path.parent() {
        Some(parent) if is_session_dir(parent) => parent.to_path_buf(),
        _ => path.to_path_buf(),
    };
    crate::trash::trash_paths(provider_id, session_id, title, &[target])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collect_files_filters_by_extension() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("a.jsonl"), b"{}").unwrap();
        std::fs::write(dir.path().join("nested/b.jsonl"), b"{}").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"x").unwrap();

        let mut files = Vec::new();
        collect_files(dir.path(), &["jsonl"], &mut files);
        assert_eq!(files.len(), 2, "recurses and skips other extensions");
    }

    #[test]
    fn collect_files_accepts_several_extensions() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.zstd"), b"x").unwrap();
        std::fs::write(dir.path().join("b.zst"), b"x").unwrap();

        let mut files = Vec::new();
        collect_files(dir.path(), &["zstd", "zst"], &mut files);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn collect_files_on_a_missing_root_is_empty() {
        let mut files = Vec::new();
        collect_files(Path::new("/definitely/not/here"), &["jsonl"], &mut files);
        assert!(files.is_empty());
    }

    #[test]
    fn truncate_appends_ellipsis_only_when_needed() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("  padded  ", 10), "padded");
        assert_eq!(truncate("abcdefghij", 4), "abcd...");
    }

    #[test]
    fn decode_project_name_rebuilds_windows_paths() {
        assert_eq!(decode_project_name("D--DSH"), "D:/DSH");
        assert_eq!(decode_project_name("plain"), "plain");
    }

    #[test]
    fn is_session_dir_detects_the_dsh_layout() {
        let dir = tempdir().unwrap();
        assert!(!is_session_dir(dir.path()));
        std::fs::write(dir.path().join("session.jsonl.zstd"), b"x").unwrap();
        assert!(is_session_dir(dir.path()));
    }

    #[test]
    fn dir_basename_handles_trailing_separators() {
        assert_eq!(dir_basename("D:/Work/app/").as_deref(), Some("app"));
        assert_eq!(dir_basename("D:\\Work\\app").as_deref(), Some("app"));
        assert_eq!(dir_basename(""), None);
    }

    // Regression: a message object's `content` is an array of blocks in the
    // real Claude Code format. extract_text used to only accept a string
    // there, so nearly every message came back empty and was dropped from the
    // Messages tab.
    #[test]
    fn extract_text_recurses_into_object_content_arrays() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": [
                { "type": "text", "text": "checking" },
                { "type": "tool_use", "name": "Bash" }
            ]
        });
        let text = extract_text(&message);
        assert!(text.contains("checking"), "got: {text:?}");
        assert!(text.contains("[Tool: Bash]"), "got: {text:?}");
    }

    #[test]
    fn extract_text_handles_string_content() {
        let message = serde_json::json!({ "role": "user", "content": "plain text" });
        assert_eq!(extract_text(&message), "plain text");
    }

    #[test]
    fn extract_text_handles_a_bare_array() {
        let content = serde_json::json!([{ "type": "text", "text": "hi" }]);
        assert_eq!(extract_text(&content), "hi");
    }

    #[test]
    fn extract_text_reads_tool_result_blocks() {
        let message = serde_json::json!({
            "role": "user",
            "content": [{ "type": "tool_result", "content": "a.txt" }]
        });
        assert_eq!(extract_text(&message), "a.txt");
    }

    #[test]
    fn extract_text_of_unrelated_values_is_empty() {
        assert_eq!(extract_text(&serde_json::json!(null)), "");
        assert_eq!(extract_text(&serde_json::json!(42)), "");
        assert_eq!(extract_text(&serde_json::json!({})), "");
    }
}
