// ── Claude Code sessions ─────────────────────────────────────────────────
//
// Stored as plain JSONL at ~/.claude/projects/{project}/{sessionId}.jsonl.
// Session metadata comes from `sessionId`/`cwd`/`timestamp` lines; titles
// prefer a user-set `/rename` (custom-title), then the model's ai-title,
// then the first user message.

use std::path::{Path, PathBuf};

use crate::session_manager::provider::SessionProvider;
use crate::session_manager::utils::{
    decode_project_name, dir_basename, extract_text, home_dir, truncate,
};
use crate::session_manager::{SessionMessage, SessionMeta};
use crate::trajectory::utils::{parse_timestamp_to_ms, read_head_tail_lines};

pub struct ClaudeProvider;

impl SessionProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn sessions_dir(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".claude").join("projects");
        dir.exists().then_some(dir)
    }

    fn parse_session(&self, path: &Path) -> Option<SessionMeta> {
        // Sub-agent transcripts and the journal aren't standalone sessions.
        let fname = path.file_name()?.to_str()?;
        if fname.starts_with("agent-") || fname == "journal.jsonl" {
            return None;
        }

        // A generous tail so ai-title / custom-title entries written mid-session
        // are still found (the title is re-emitted periodically).
        let (head, tail) = read_head_tail_lines(path, 10, 150).ok()?;

        let mut session_id: Option<String> = None;
        let mut project_dir: Option<String> = None;
        let mut created_at: Option<i64> = None;
        let mut first_user_message: Option<String> = None;

        for line in &head {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            if session_id.is_none() {
                session_id = value
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
            if project_dir.is_none() {
                project_dir = value
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
            if created_at.is_none() {
                created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
            }
            if first_user_message.is_none() && is_user_line(&value) {
                if let Some(message) = value.get("message") {
                    let text = extract_text(message);
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.contains("<local-command-caveat>")
                        && !trimmed.starts_with("<command-name>")
                    {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
            if session_id.is_some()
                && project_dir.is_some()
                && created_at.is_some()
                && first_user_message.is_some()
            {
                break;
            }
        }

        let mut last_active_at: Option<i64> = None;
        let mut summary: Option<String> = None;
        let mut custom_title: Option<String> = None;
        let mut ai_title: Option<String> = None;

        for line in tail.iter().rev() {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            if last_active_at.is_none() {
                last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
            }
            let line_type = value.get("type").and_then(|v| v.as_str());
            if custom_title.is_none() && line_type == Some("custom-title") {
                custom_title = value
                    .get("customTitle")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            if ai_title.is_none() && line_type == Some("ai-title") {
                ai_title = value
                    .get("aiTitle")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            if summary.is_none() {
                if value.get("isMeta").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                if let Some(message) = value.get("message") {
                    let text = extract_text(message);
                    if !text.trim().is_empty() {
                        summary = Some(text);
                    }
                }
            }
            if last_active_at.is_some() && summary.is_some() {
                break;
            }
        }

        let session_id =
            session_id.or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))?;

        let title = custom_title
            .or(ai_title)
            .or_else(|| first_user_message.map(|t| truncate(&t, 80)))
            .or_else(|| project_dir.as_deref().and_then(dir_basename));

        Some(SessionMeta {
            provider_id: "claude".to_string(),
            session_id: session_id.clone(),
            title,
            summary: summary.map(|text| truncate(&text, 160)),
            project_dir,
            project_group: project_group(path),
            created_at,
            last_active_at,
            source_path: Some(path.to_string_lossy().to_string()),
            resume_command: Some(format!("claude --resume {session_id}")),
        })
    }

    fn load_messages(&self, source: &str) -> Result<Vec<SessionMessage>, String> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let file = File::open(source).map_err(|e| format!("Failed to open session file: {e}"))?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|e| format!("Read error: {e}"))?;
            let value: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if value.get("isMeta").and_then(|v| v.as_bool()) == Some(true) {
                continue;
            }

            let Some(message) = value.get("message") else {
                continue;
            };

            let mut role = message
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Tool results arrive as user-role messages; surface them as "tool".
            if role == "user" {
                if let Some(items) = message.get("content").and_then(|v| v.as_array()) {
                    if !items.is_empty()
                        && items.iter().all(|item| {
                            item.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                        })
                    {
                        role = "tool".to_string();
                    }
                }
            }

            let content = extract_text(message);
            if content.trim().is_empty() {
                continue;
            }

            messages.push(SessionMessage {
                role,
                content,
                ts: value.get("timestamp").and_then(parse_timestamp_to_ms),
            });
        }

        Ok(messages)
    }

    fn trash_session(&self, source: &str) -> Result<String, String> {
        crate::session_manager::delete_file_backed_session(self.id(), Path::new(source))
    }

    fn trash_sessions_in_dir(&self, dir: &Path) -> Result<usize, String> {
        crate::session_manager::trash_files_in_dir(self.id(), dir)
    }
}

/// Whether a line carries a user-role message (either shape Claude uses).
fn is_user_line(value: &serde_json::Value) -> bool {
    value.get("type").and_then(|v| v.as_str()) == Some("user")
        || value
            .get("message")
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str())
            == Some("user")
}

/// Claude encodes the project as the parent directory name (`D--DSH`).
fn project_group(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_project_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn parse_reads_metadata_title_and_timestamps() {
        let dir = tempdir().unwrap();
        // Real layout: projects/{encoded-project}/{session}.jsonl
        let file = dir.path().join("D--DSH/sess-1.jsonl");
        write(
            &file,
            concat!(
                r#"{"sessionId":"sess-1","cwd":"D:/DSH","timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":"fix the bug"},"timestamp":"2026-03-06T10:01:00Z"}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]},"timestamp":"2026-03-06T10:02:00Z"}"#,
            ),
        );

        let meta = ClaudeProvider.parse_session(&file).unwrap();
        assert_eq!(meta.provider_id, "claude");
        assert_eq!(meta.session_id, "sess-1");
        assert_eq!(meta.title.as_deref(), Some("fix the bug"));
        assert_eq!(meta.project_dir.as_deref(), Some("D:/DSH"));
        assert_eq!(meta.project_group.as_deref(), Some("D:/DSH"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("claude --resume sess-1")
        );
        assert!(meta.created_at.is_some());
        assert!(meta.last_active_at.is_some());
    }

    #[test]
    fn parse_prefers_custom_title_over_first_message() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("proj/sess-2.jsonl");
        write(
            &file,
            concat!(
                r#"{"sessionId":"sess-2","timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":"first msg"}}"#,
                "\n",
                r#"{"type":"custom-title","customTitle":"Renamed Session"}"#,
            ),
        );

        let meta = ClaudeProvider.parse_session(&file).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Renamed Session"));
    }

    #[test]
    fn parse_skips_agent_and_journal_files() {
        let dir = tempdir().unwrap();
        for name in ["agent-abc.jsonl", "journal.jsonl"] {
            let file = dir.path().join(name);
            write(&file, r#"{"sessionId":"x"}"#);
            assert!(ClaudeProvider.parse_session(&file).is_none(), "{name}");
        }
    }

    #[test]
    fn load_messages_reclassifies_tool_results() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sess.jsonl");
        write(
            &file,
            concat!(
                r#"{"type":"user","message":{"role":"user","content":"list files"},"timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"checking"},{"type":"tool_use","name":"Bash"}]}}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"a.txt"}]}}"#,
                "\n",
                r#"{"isMeta":true,"message":{"role":"user","content":"hidden"}}"#,
            ),
        );

        let msgs = ClaudeProvider
            .load_messages(file.to_str().unwrap())
            .unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("[Tool: Bash]"));
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].content, "a.txt");
    }
}
