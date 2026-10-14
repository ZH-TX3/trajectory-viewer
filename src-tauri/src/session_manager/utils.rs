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
                    // A subagent dispatch is worth naming: the agent type and its
                    // one-line description say what was delegated, which a bare
                    // `[Tool: Agent]` does not.
                    match subagent_dispatch_detail(name, item.get("input")) {
                        Some(detail) => Some(format!("[Tool: {name}] {detail}")),
                        None => Some(format!("[Tool: {name}]")),
                    }
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

/// One-line description of a subagent dispatch, for the `[Tool: Agent]` marker.
///
/// Returns `None` for every other tool, so the caller emits the bare marker.
fn subagent_dispatch_detail(name: &str, input: Option<&serde_json::Value>) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    if lower != "agent" && lower != "task" {
        return None;
    }
    let input = input?;
    let field = |key: &str| input.get(key).and_then(|v| v.as_str());
    let description = field("description").unwrap_or("").trim();
    let agent_type = field("subagent_type").unwrap_or("").trim();

    let mut parts: Vec<String> = Vec::new();
    if !agent_type.is_empty() {
        parts.push(format!("{agent_type}:"));
    }
    if !description.is_empty() {
        parts.push(truncate(description, 80));
    }
    if input.get("run_in_background").and_then(|v| v.as_bool()) == Some(true) {
        parts.push("(background)".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
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
///
/// This is a LOSSY encoding: every character outside `[A-Za-z0-9]` becomes
/// `-`, so a name cannot distinguish a real separator from a dash, a dot, or
/// a CJK character. Treat the result as a label, not a path; prefer
/// `project_display_path`, which checks the filesystem.
pub fn decode_project_name(name: &str) -> String {
    let mut result = name.to_string();
    if let Some(idx) = result.find("--") {
        let drive = &result[..idx];
        let rest = &result[idx + 2..];
        result = format!("{}:/{}", drive, rest.replace("--", "/").replace("---", "/"));
    }
    result
}

/// Recover a real directory from an encoded project name, checking the disk.
///
/// Because the encoding is lossy (see `decode_project_name`), the name is split
/// at every `-` and a candidate is kept only when that directory exists. That
/// resolves the ambiguity: `D--DSH-trajectory-viewer-src-tauri` could be
/// `D:/DSH/trajectory-viewer-src-tauri` or `D:/DSH/trajectory-viewer/src-tauri`,
/// and only the one on disk is returned.
///
/// Both naming shapes are understood: Windows `D--DSH-proj`, where the drive is
/// the leading letter, and POSIX `-Users-me-proj`, where the leading `-` is the
/// root slash.
///
/// Returns `None` when nothing matches - the project may simply have been
/// deleted, in which case the caller falls back to the decoded label.
pub fn resolve_project_path(encoded: &str) -> Option<String> {
    let (root, rest) = split_encoded_name(encoded)?;
    let segments: Vec<&str> = rest.split('-').collect();

    // Greedy walk: at each depth take the SHORTEST segment that exists on disk,
    // so the split points are decided by what is really there rather than by
    // guessing where the dashes were.
    fn walk(segments: &[&str], prefix: &str, depth: usize) -> Option<String> {
        if depth == segments.len() {
            return Some(prefix.trim_end_matches('/').to_string());
        }
        for end in depth + 1..=segments.len() {
            let segment = segments[depth..end].join("-");
            if segment.is_empty() {
                continue;
            }
            let candidate = format!("{prefix}{segment}");
            if Path::new(&candidate).is_dir() {
                if let Some(found) = walk(segments, &format!("{candidate}/"), end) {
                    return Some(found);
                }
            }
        }
        None
    }

    walk(&segments, &root, 0)
}

/// Split an encoded directory name into its filesystem root and the rest.
///
/// A Windows name reads `<drive>--<rest>`, where the drive is one ASCII letter
/// and the two dashes stand in for `:\`. A POSIX path has no drive: its leading
/// `/` became the first `-`, so the name starts with a single dash and the root
/// is `/`. Anything else - a plain label, an empty string, two leading dashes -
/// is not a path that can be rebuilt.
///
/// Byte comparisons rather than slicing: an encoded name may hold non-ASCII
/// text, and `&encoded[1..3]` would panic when a multi-byte character straddles
/// that boundary.
fn split_encoded_name(encoded: &str) -> Option<(String, &str)> {
    let bytes = encoded.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b'-' && bytes[2] == b'-' {
        return Some((
            format!("{}:/", encoded[..1].to_ascii_uppercase()),
            &encoded[3..],
        ));
    }
    if bytes.first() == Some(&b'-') && bytes.get(1) != Some(&b'-') {
        return Some(("/".to_string(), &encoded[1..]));
    }
    None
}

/// The path to show for a session's project.
///
/// Prefers the `cwd` the session itself recorded: that is the authoritative
/// path and survives the lossy directory-name encoding, including non-ASCII
/// segments. Falls back to a filesystem-verified decode, then to the decoded
/// label so the UI still shows something recognizable for a deleted project.
pub fn project_display_path(encoded: &str, cwd: Option<&str>) -> String {
    if let Some(cwd) = cwd.map(str::trim).filter(|c| !c.is_empty()) {
        return cwd.to_string();
    }
    resolve_project_path(encoded).unwrap_or_else(|| decode_project_name(encoded))
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

    // The encoded name is lossy, so the only way to recover the real path is to
    // check the filesystem: `-` may be a separator or part of a directory name.
    #[test]
    fn resolve_project_path_uses_the_filesystem_to_split_dashes() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // root/AI/ModHeader-refactor  and  root/AI/ModHeader
        std::fs::create_dir_all(root.join("AI/ModHeader-refactor")).unwrap();
        std::fs::create_dir_all(root.join("AI/ModHeader")).unwrap();

        // Rebuild an encoded name from an absolute path, then resolve it.
        let encode = |rel: &str| {
            let abs = root.join(rel);
            abs.to_string_lossy().replace(['\\', '/', ':'], "-")
        };

        // "AI-ModHeader-refactor" must resolve to the deeper directory, not
        // stop at "AI-ModHeader" and leave "refactor" dangling.
        let encoded = encode("AI/ModHeader-refactor");
        let resolved = resolve_project_path(&encoded).expect("should resolve");
        let expected = root
            .join("AI/ModHeader-refactor")
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(resolved.replace('\\', "/"), expected);
    }

    #[test]
    fn resolve_project_path_returns_none_for_a_missing_directory() {
        // A deleted project cannot be recovered; the caller falls back to a label.
        assert_eq!(resolve_project_path("Z--definitely-not-here-xyz"), None);
        // Not an encoded path at all.
        assert_eq!(resolve_project_path("plain"), None);
        assert_eq!(resolve_project_path(""), None);
    }

    // The two naming shapes differ only in how the root is spelled, and getting
    // that wrong makes the whole lookup return `None` on one of the platforms.
    #[test]
    fn an_encoded_name_is_split_into_a_root_and_its_segments() {
        assert_eq!(
            split_encoded_name("D--DSH-proj"),
            Some(("D:/".to_string(), "DSH-proj"))
        );
        assert_eq!(
            split_encoded_name("-Users-me-proj"),
            Some(("/".to_string(), "Users-me-proj"))
        );

        // A bare drive letter is lowercase in the encoding, uppercase after.
        assert_eq!(
            split_encoded_name("c--proj"),
            Some(("C:/".to_string(), "proj"))
        );

        // `-` is the encoding of the root itself. It splits into a root with
        // nothing under it, and the walk over that lone empty segment finds no
        // directory - which is the right answer, since `/` names no project.
        assert_eq!(split_encoded_name("-"), Some(("/".to_string(), "")));
        assert_eq!(resolve_project_path("-"), None);

        // Not a path at all: an empty or plain label. A POSIX name has exactly
        // one leading dash, so `--x` is not a root plus a name.
        assert_eq!(split_encoded_name(""), None);
        assert_eq!(split_encoded_name("plain"), None);
        assert_eq!(split_encoded_name("--x"), None);
    }

    // The recorded `cwd` is authoritative: it survives the lossy encoding,
    // including non-ASCII segments that the directory name cannot represent.
    #[test]
    fn project_display_path_prefers_the_recorded_cwd() {
        let shown = project_display_path("C--Users-----claude", Some(r"C:\Users\陈强\.claude"));
        assert_eq!(shown, r"C:\Users\陈强\.claude");
    }

    #[test]
    fn project_display_path_falls_back_to_the_decoded_label() {
        // No cwd and nothing on disk: still show something recognizable rather
        // than an empty group.
        let shown = project_display_path("D--DSH", None);
        assert_eq!(shown, "D:/DSH");
        // A blank cwd is treated as absent, not as the path "".
        assert_eq!(project_display_path("D--DSH", Some("   ")), "D:/DSH");
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

    // A bare `[Tool: Agent]` says nothing about what was delegated; the
    // Messages tab should name the agent type and its task.
    #[test]
    fn extract_text_names_a_subagent_dispatch() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "name": "Agent",
                "input": {
                    "subagent_type": "Explore",
                    "description": "Audit the parser",
                    "run_in_background": true
                }
            }]
        });
        assert_eq!(
            extract_text(&message),
            "[Tool: Agent] Explore: Audit the parser (background)"
        );
    }

    #[test]
    fn extract_text_keeps_other_tools_bare() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "name": "Bash",
                "input": { "command": "ls" }
            }]
        });
        assert_eq!(extract_text(&message), "[Tool: Bash]");
    }

    #[test]
    fn extract_text_handles_a_dispatch_without_arguments() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": [{ "type": "tool_use", "name": "Task", "input": {} }]
        });
        assert_eq!(extract_text(&message), "[Tool: Task]");
    }

    #[test]
    fn extract_text_of_unrelated_values_is_empty() {
        assert_eq!(extract_text(&serde_json::json!(null)), "");
        assert_eq!(extract_text(&serde_json::json!(42)), "");
        assert_eq!(extract_text(&serde_json::json!({})), "");
    }
}
