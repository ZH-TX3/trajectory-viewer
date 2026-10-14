// ── Project memory documents ─────────────────────────────────────────────
//
// Claude stores per-project memory as Markdown next to the session files:
//
//   ~/.claude/projects/{encoded-project}/memory/MEMORY.md   <- index
//   ~/.claude/projects/{encoded-project}/memory/{name}.md   <- one fact each
//
// A fact file carries YAML frontmatter with its own `name`, `description`,
// `type`, and `originSessionId` — the last one is what lets the UI say which
// session a memory came from.
//
// There is no YAML crate in the dependency set and the frontmatter is a flat
// `key: value` block plus one nested `metadata:` group, so it is scanned line
// by line rather than pulling in a parser.

use std::path::Path;

use serde::Serialize;

/// One Markdown memory document, ready to render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDoc {
    /// File name, e.g. `feedback_quantified_eval.md`.
    pub file_name: String,
    /// Frontmatter `name`, when present.
    pub name: Option<String>,
    /// Frontmatter `description` — the one-line summary shown in the list.
    pub description: Option<String>,
    /// Frontmatter `metadata.type`: `feedback` | `project` | `reference`.
    pub kind: Option<String>,
    /// Frontmatter `metadata.originSessionId`: the session that wrote this.
    pub origin_session_id: Option<String>,
    /// File modification time, in milliseconds.
    pub updated_at: Option<i64>,
    /// `MEMORY.md` is the index the agent reads first, not a single fact.
    pub is_index: bool,
    /// The Markdown body, with any frontmatter stripped.
    pub content: String,
}

/// Load every memory document for the project that owns `session_path`.
///
/// Returns an empty list when the project has no `memory/` directory, which is
/// the common case — most projects never write one.
pub fn load_memories(session_path: &Path) -> Vec<MemoryDoc> {
    let Some(dir) = memory_dir(session_path) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut docs: Vec<MemoryDoc> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
        })
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?.to_string();
            let raw = std::fs::read_to_string(&path).ok()?;
            let updated_at = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            let (front, body) = split_frontmatter(&raw);
            Some(MemoryDoc {
                is_index: file_name.eq_ignore_ascii_case("MEMORY.md"),
                name: front_field(front, "name"),
                description: front_field(front, "description"),
                kind: front_field(front, "type"),
                origin_session_id: front_field(front, "originSessionId"),
                updated_at,
                file_name,
                content: body,
            })
        })
        .collect();

    // The index first (it is the entry point), then newest first, then by name
    // so the order is stable across reloads.
    docs.sort_by(|a, b| {
        b.is_index
            .cmp(&a.is_index)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.file_name.cmp(&b.file_name))
    });
    docs
}

/// Where the project's memory documents live, given any session file path.
///
/// The memory directory is a sibling of the session files, so this does not
/// depend on decoding the (lossy) project directory name.
fn memory_dir(session_path: &Path) -> Option<std::path::PathBuf> {
    session_path.parent().map(|p| p.join("memory"))
}

/// Overwrite one memory document's Markdown body.
///
/// The frontmatter is preserved: these files carry the `name`/`description`/
/// `type` the sidebar sorts and labels by, and rewriting the body through the
/// app must not silently drop them.
///
/// The `session_path` decides which project is being edited and `file_name`
/// picks the document inside it, so neither is trusted to name a file on its
/// own: a `..`-laden or absolute `file_name` resolves against the memory
/// directory rather than the filesystem root. The target must also already
/// exist and be a regular file — this edits documents the agent wrote, and
/// creating or replacing them with something that is not a document is the
/// agent's job.
pub fn save_memory(session_path: &Path, file_name: &str, content: &str) -> Result<(), String> {
    let Some(dir) = memory_dir(session_path) else {
        return Err("No memory directory for this project".to_string());
    };
    let target = dir.join(file_name);

    // Resolve both sides with the filesystem rather than reasoning about `..`
    // in strings: canonicalize collapses `../`, follows symlinks, and fails
    // when the target does not exist — which is the answer we want here anyway,
    // since this only edits documents the agent already wrote.
    let (resolved_dir, resolved_target) = match (dir.canonicalize(), target.canonicalize()) {
        (Ok(d), Ok(t)) => (d, t),
        _ => return Err(format!("No such memory document: {file_name}")),
    };
    if !resolved_target.is_file() {
        return Err(format!("No such memory document: {file_name}"));
    }
    // The resolved target must sit inside the resolved directory. This is what
    // stops `../secret.md`: once canonicalized it no longer starts with the
    // memory directory.
    if !resolved_target.starts_with(&resolved_dir) {
        return Err(format!("Not a memory document: {file_name}"));
    }

    let raw = std::fs::read_to_string(&resolved_target)
        .map_err(|e| format!("Cannot read {}: {e}", resolved_target.display()))?;
    let (front, _) = split_frontmatter(&raw);

    // Rebuild the file rather than writing the body alone, so the frontmatter
    // survives. A file without frontmatter is written as the new body.
    let mut out = String::new();
    if let Some(front) = front {
        // Drop any leading whitespace the split left on the first key line.
        let front = front.trim_start_matches(['\r', '\n']);
        out.push_str(&format!("---\n{front}\n---\n"));
        // A blank line before the body, the way these files are written.
        out.push('\n');
    }
    out.push_str(content);

    std::fs::write(&resolved_target, out).map_err(|e| {
        format!(
            "Cannot write {}: {e}",
            resolved_target.display()
        )
    })
}

/// Split leading `---` frontmatter from the Markdown body.
///
/// A file without frontmatter is all body.
fn split_frontmatter(raw: &str) -> (Option<&str>, String) {
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut lines = trimmed.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return (None, raw.to_string());
    }
    let rest = trimmed.split_once('\n').map(|(_, r)| r).unwrap_or_default();
    let Some(end) = rest.find("\n---") else {
        // An unterminated block is not frontmatter; show the file as-is.
        return (None, raw.to_string());
    };
    let front = &rest[..end];
    // `end` points at the newline BEFORE the closing `---`, so skip past that
    // whole line rather than just the newline.
    let after_closing = &rest[end + 1..];
    let body = after_closing
        .split_once('\n')
        .map(|(_, b)| b.trim_start_matches(['\r', '\n']))
        .unwrap_or_default();
    (Some(front), body.to_string())
}

/// Read one `key: value` from a frontmatter block.
///
/// Only the flat keys and the single nested `metadata:` group exist in these
/// files, so the scan tracks the current group instead of parsing YAML.
fn front_field(front: Option<&str>, key: &str) -> Option<String> {
    let front = front?;
    let mut in_metadata = false;
    for line in front.lines() {
        let indent = line.len() - line.trim_start().len();
        let content = line.trim();
        if content.is_empty() {
            continue;
        }
        if indent == 0 {
            in_metadata = content.trim_end_matches(':').trim() == "metadata";
        }
        // A line without a colon is not a key/value pair (a bare list item, a
        // stray `---`). Skipping it is required: `?` here would abort the whole
        // scan and silently drop every field after it.
        let Some((k, v)) = content.split_once(':') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let value = v.trim().trim_matches('"').trim_matches('\'').trim();
        if value.is_empty() {
            continue;
        }
        // `type` and `originSessionId` live under `metadata:`; `name` and
        // `description` are top level. Accept either so a file that inlines
        // them still resolves, but keep a `name:` in the body from matching.
        let at_top = indent == 0 && !in_metadata;
        if at_top || in_metadata {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(dir: &Path, name: &str, contents: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn frontmatter_is_stripped_from_the_body() {
        let (front, body) = split_frontmatter("---\nname: x\n---\n\nHello\n");
        assert_eq!(front, Some("name: x"));
        assert_eq!(body.trim(), "Hello");
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let (front, body) = split_frontmatter("# Title\n\ntext\n");
        assert_eq!(front, None);
        assert_eq!(body, "# Title\n\ntext\n");
    }

    // An opening `---` with no closing one is not frontmatter; the content must
    // still be readable rather than swallowed.
    #[test]
    fn an_unterminated_block_is_not_frontmatter() {
        let (front, body) = split_frontmatter("---\nname: x\nno closing\n");
        assert_eq!(front, None);
        assert!(body.contains("no closing"));
    }

    #[test]
    fn nested_metadata_keys_are_read() {
        let front = "name: my-fact\ndescription: one line\nmetadata: \n  node_type: memory\n  type: feedback\n  originSessionId: abc-123\n";
        assert_eq!(front_field(Some(front), "name").as_deref(), Some("my-fact"));
        assert_eq!(
            front_field(Some(front), "description").as_deref(),
            Some("one line")
        );
        assert_eq!(
            front_field(Some(front), "type").as_deref(),
            Some("feedback")
        );
        assert_eq!(
            front_field(Some(front), "originSessionId").as_deref(),
            Some("abc-123")
        );
    }

    // Regression: `split_once(':')?` aborted the whole scan on a line with
    // no colon, silently dropping every field after it.
    #[test]
    fn a_colonless_line_does_not_abort_the_scan() {
        let front = "name: my-fact
---
description: real desc
";
        assert_eq!(
            front_field(Some(front), "description").as_deref(),
            Some("real desc")
        );
    }

    #[test]
    fn a_missing_key_is_none() {
        assert_eq!(front_field(Some("name: x\n"), "type"), None);
        assert_eq!(front_field(None, "name"), None);
    }

    #[test]
    fn loads_and_orders_the_index_first() {
        let dir = tempdir().unwrap();
        let memory = dir.path().join("proj/memory");
        write(&memory, "MEMORY.md", "# index\n");
        write(
            &memory,
            "feedback_old.md",
            "---\nname: old\ndescription: d\nmetadata:\n  type: feedback\n  originSessionId: s1\n---\n\nbody\n",
        );
        // A non-Markdown file must be ignored.
        write(&memory, "notes.txt", "ignore me");

        let docs = load_memories(&dir.path().join("proj/sess.jsonl"));
        assert_eq!(docs.len(), 2);
        assert!(docs[0].is_index, "the index sorts first");
        assert_eq!(docs[0].file_name, "MEMORY.md");
        let fact = &docs[1];
        assert_eq!(fact.name.as_deref(), Some("old"));
        assert_eq!(fact.kind.as_deref(), Some("feedback"));
        assert_eq!(fact.origin_session_id.as_deref(), Some("s1"));
        assert_eq!(fact.content.trim(), "body");
    }

    #[test]
    fn a_project_without_memory_yields_nothing() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("proj")).unwrap();
        assert!(load_memories(&dir.path().join("proj/sess.jsonl")).is_empty());
    }

    #[test]
    fn saving_a_document_preserves_its_frontmatter() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj/sess.jsonl");
        std::fs::create_dir_all(dir.path().join("proj")).unwrap();
        let memory = dir.path().join("proj/memory");
        write(
            &memory,
            "fact.md",
            "---\nname: my-fact\ndescription: one line\n---\n\nold body\n",
        );

        save_memory(&session, "fact.md", "new body\n").unwrap();

        let after = std::fs::read_to_string(memory.join("fact.md")).unwrap();
        assert!(after.contains("name: my-fact"), "frontmatter survives");
        assert!(after.contains("new body"));
        assert!(!after.contains("old body"));

        // And it still reads back through the loader.
        let docs = load_memories(&session);
        assert_eq!(docs[0].content.trim(), "new body");
        assert_eq!(docs[0].name.as_deref(), Some("my-fact"));
    }

    #[test]
    fn a_document_without_frontmatter_is_rewritten_as_its_body() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj/sess.jsonl");
        let memory = dir.path().join("proj/memory");
        write(&memory, "plain.md", "# Old

text
");

        save_memory(&session, "plain.md", "# New\n\ntext\n").unwrap();

        let after = std::fs::read_to_string(memory.join("plain.md")).unwrap();
        assert!(after.starts_with("# New"), "no frontmatter is invented: {after:?}");
        assert!(!after.contains("# Old"));
    }

    #[test]
    fn a_missing_document_is_refused() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj/sess.jsonl");
        assert!(save_memory(&session, "nope.md", "x").is_err());
    }

    #[test]
    fn a_path_outside_the_memory_directory_is_refused() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj/sess.jsonl");
        write(&dir.path().join("proj"), "outside.md", "keep me");
        // The memory directory has to exist, or the request fails earlier as
        // "no such document" and the containment check is never reached.
        write(&dir.path().join("proj/memory"), "real.md", "fine");

        let err = save_memory(&session, "../outside.md", "clobbered").unwrap_err();
        assert!(err.contains("Not a memory document"), "got: {err}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("proj/outside.md")).unwrap(),
            "keep me"
        );
    }

    fn a_project_without_a_memory_directory_is_refused() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj/sess.jsonl");
        std::fs::create_dir_all(dir.path().join("proj")).unwrap();
        assert!(save_memory(&session, "fact.md", "body").is_err());
    }
}
