// ── Fork a conversation from a chosen node ───────────────────────────────
//
// "Resume" continues a session where it actually ended; "restore" continues it
// from a message the reader picked, leaving the original transcript untouched.
// The two are different files on disk, and only the first is a plain launch.
//
// A provider resumes from its transcript, so a fork is a second transcript that
// ends at the chosen node. Claude Code names a session after its file and reads
// it back as an append-only log, so the fork is: the ancestor chain of the node,
// in file order, with every `sessionId` rewritten to a fresh id. Verified against
// the CLI — `claude --resume <new-id>` continues from the last copied message.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// Providers whose transcript can be forked this way.
///
/// DSH and Codex compress or nest their sessions differently, so each needs its
/// own copier rather than a guess bolted onto this one.
pub const SUPPORTED: [&str; 1] = ["claude"];

/// What the UI needs to continue the fork it just asked for.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkOutcome {
    /// The new session's id — the value the CLI resumes.
    pub session_id: String,
    /// The transcript the fork was written to.
    pub source_path: String,
    /// Directory the original conversation ran in, used as the new tab's cwd.
    pub cwd: Option<String>,
    /// Records carried over, so the UI can report a short chain instead of
    /// looking like it silently dropped history.
    pub messages: usize,
}

/// Whether `provider_id` can be forked at an arbitrary node yet.
pub fn is_supported(provider_id: &str) -> bool {
    SUPPORTED.contains(&provider_id)
}

/// Write a new transcript that ends at `message_uuid`, and report its id.
pub fn fork(
    provider_id: &str,
    source_path: &str,
    message_uuid: &str,
) -> Result<ForkOutcome, String> {
    if !is_supported(provider_id) {
        return Err(format!(
            "Restoring a conversation is not supported for {provider_id} yet"
        ));
    }
    if message_uuid.trim().is_empty() {
        return Err("Missing message id".to_string());
    }
    fork_transcript(Path::new(source_path), message_uuid)
}

fn fork_transcript(path: &Path, message_uuid: &str) -> Result<ForkOutcome, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("Cannot read the session: {e}"))?;
    let records: Vec<Value> = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();

    let by_uuid: HashMap<&str, usize> = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((record.get("uuid")?.as_str()?, index)))
        .collect();

    let Some(&node) = by_uuid.get(message_uuid) else {
        return Err(format!("Message {message_uuid} is not in this session"));
    };

    // Walking `parentUuid` rather than taking the lines above the node: a rewind
    // leaves a sibling branch behind in the file, and copying that prefix would
    // hand the new session a history this message never had.
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = Some(message_uuid);
    while let Some(uuid) = cursor {
        let Some(&index) = by_uuid.get(uuid) else {
            break;
        };
        // A malformed transcript could point at itself; stop instead of hanging.
        if !seen.insert(index) {
            break;
        }
        chain.push(index);
        cursor = records[index].get("parentUuid").and_then(Value::as_str);
    }
    // The chain is read leaf-first; the log has to stay in the order it was
    // written, or a resumed CLI reorders the conversation.
    chain.sort_unstable();

    let cwd = records[node]
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);

    let new_id = uuid::Uuid::new_v4().to_string();
    let dir = path
        .parent()
        .ok_or_else(|| "The session has no directory".to_string())?;
    let fork_path = dir.join(format!("{new_id}.jsonl"));

    let mut out = String::new();
    for &index in &chain {
        let mut record = records[index].clone();
        if let Some(object) = record.as_object_mut() {
            object.insert("sessionId".to_string(), Value::String(new_id.clone()));
        }
        let line = serde_json::to_string(&record)
            .map_err(|e| format!("Cannot encode the forked session: {e}"))?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(&fork_path, out).map_err(|e| format!("Cannot write the forked session: {e}"))?;

    Ok(ForkOutcome {
        session_id: new_id,
        source_path: fork_path.to_string_lossy().into_owned(),
        cwd,
        messages: chain.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A transcript shaped like Claude's: two user turns, an assistant reply
    /// between them, and a rewound sibling that must not join the fork.
    fn transcript(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("original.jsonl");
        let lines = [
            r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"old","cwd":"/proj","message":{"role":"user","content":"first"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"old","cwd":"/proj","message":{"role":"assistant","content":[{"type":"text","text":"one"}]}}"#,
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","sessionId":"old","cwd":"/proj","message":{"role":"user","content":"second"}}"#,
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","sessionId":"old","cwd":"/proj","message":{"role":"assistant","content":[{"type":"text","text":"two"}]}}"#,
            // A rewind: same parent as `a2`, written later — a prefix copy would
            // drag it into a fork taken at `u2`.
            r#"{"type":"assistant","uuid":"a2b","parentUuid":"u2","sessionId":"old","cwd":"/proj","message":{"role":"assistant","content":[{"type":"text","text":"two again"}]}}"#,
        ];
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        path
    }

    #[test]
    fn a_fork_carries_the_chosen_nodes_ancestry_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path());

        let fork = fork_transcript(&path, "u2").unwrap();

        assert_eq!(fork.messages, 3, "u1 → a1 → u2");
        assert_eq!(fork.cwd.as_deref(), Some("/proj"));
        let written = std::fs::read_to_string(&fork.source_path).unwrap();
        let records: Vec<Value> = written
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect();
        let uuids: Vec<&str> = records
            .iter()
            .map(|r| r.get("uuid").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(uuids, ["u1", "a1", "u2"]);
        assert!(
            !uuids.contains(&"a2b"),
            "a rewound sibling must not join the fork"
        );
    }

    #[test]
    fn every_copied_record_is_renamed_to_the_new_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path());

        let fork = fork_transcript(&path, "u2").unwrap();

        // The transcript is named after the session, and the CLI finds a session
        // by that name — both have to agree.
        assert!(fork
            .source_path
            .ends_with(&format!("{}.jsonl", fork.session_id)));
        assert_ne!(fork.session_id, "old");
        let written = std::fs::read_to_string(&fork.source_path).unwrap();
        for line in written.lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            assert_eq!(record["sessionId"], fork.session_id);
        }
    }

    #[test]
    fn the_original_transcript_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path());
        let before = std::fs::read_to_string(&path).unwrap();

        fork_transcript(&path, "u2").unwrap();

        assert_eq!(
            before,
            std::fs::read_to_string(&path).unwrap(),
            "restoring a conversation must not rewrite the one it came from"
        );
    }

    #[test]
    fn forking_at_the_first_message_keeps_only_that_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path());

        let fork = fork_transcript(&path, "u1").unwrap();

        assert_eq!(fork.messages, 1);
    }

    #[test]
    fn an_unknown_node_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path());
        let err = fork_transcript(&path, "nope").unwrap_err();
        assert!(err.contains("not in this session"), "got: {err}");
    }

    #[test]
    fn providers_without_a_transcript_copier_are_refused() {
        let err = fork("dsh", "/nowhere", "u1").unwrap_err();
        assert!(err.contains("not supported"), "got: {err}");
        assert!(is_supported("claude"));
        assert!(!is_supported("dsh"));
    }

    /// Newest real Claude transcript on this machine, if there is one.
    fn a_real_transcript() -> Option<std::path::PathBuf> {
        let projects = crate::backup::home_dir()?.join(".claude").join("projects");
        let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
        for dir in std::fs::read_dir(projects).ok()?.flatten() {
            let Ok(entries) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(seen, _)| modified > *seen) {
                    best = Some((modified, path));
                }
            }
        }
        best.map(|(_, path)| path)
    }

    // Smoke test against a real transcript: the fork must be a file the CLI can
    // read, and the session it came from must be byte-identical afterwards.
    #[test]
    #[ignore]
    fn fork_a_real_claude_session() {
        // Opt-in via `--ignored`, so a missing corpus is a failure, not a pass.
        let source = a_real_transcript().expect("no claude transcript on this machine");
        let before = std::fs::read_to_string(&source).unwrap();
        let messages =
            crate::session_manager::load_messages("claude", &source.to_string_lossy()).unwrap();

        // A user turn with a record id, past the opening noise.
        let node = messages
            .iter()
            .filter(|m| m.role == "user")
            .filter_map(|m| m.uuid.clone())
            .nth(1)
            .expect("a second user message");
        eprintln!("source: {}", source.display());

        let fork = fork("claude", &source.to_string_lossy(), &node).unwrap();
        eprintln!("fork: {} messages -> {}", fork.messages, fork.source_path);

        assert!(fork.messages >= 2, "fork carried {} records", fork.messages);
        let forked = std::fs::read_to_string(&fork.source_path).unwrap();
        let lines: Vec<Value> = forked
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect();
        assert!(
            lines.iter().all(|r| r["sessionId"] == fork.session_id),
            "every record must carry the new session id"
        );
        assert_eq!(
            lines.last().unwrap()["uuid"],
            node,
            "the fork must end on the chosen node"
        );
        assert_eq!(
            before,
            std::fs::read_to_string(&source).unwrap(),
            "the source transcript must be untouched"
        );

        std::fs::remove_file(&fork.source_path).unwrap();
    }
}
