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
    // The memory directory is a sibling of the session file, so its location
    // does not depend on decoding the (lossy) project directory name.
    let Some(dir) = session_path.parent().map(|p| p.join("memory")) else {
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
}
