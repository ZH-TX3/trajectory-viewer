// ── Codex sessions ───────────────────────────────────────────────────────
//
// Plain JSONL at ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl. Every payload
// line is `type: "response_item"`; `session_meta` carries the session id and
// cwd, and the session id can also be recovered from the rollout filename.

use std::path::{Path, PathBuf};

use crate::session_manager::provider::SessionProvider;
use crate::session_manager::utils::{dir_basename, extract_text, home_dir, truncate};
use crate::session_manager::{SessionMessage, SessionMeta};
use crate::trajectory::utils::{parse_timestamp_to_ms, read_head_tail_lines};

pub struct CodexProvider;

impl SessionProvider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn sessions_dir(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".codex").join("sessions");
        dir.exists().then_some(dir)
    }

    fn parse_session(&self, path: &Path) -> Option<SessionMeta> {
        let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

        let mut session_id: Option<String> = None;
        let mut project_dir: Option<String> = None;
        let mut created_at: Option<i64> = None;
        let mut first_user_message: Option<String> = None;

        for line in &head {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            if created_at.is_none() {
                created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
            }
            if value.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
                if let Some(payload) = value.get("payload") {
                    if session_id.is_none() {
                        session_id = payload.get("id").and_then(|v| v.as_str()).map(String::from);
                    }
                    if project_dir.is_none() {
                        project_dir = payload
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                    if let Some(ts) = payload.get("timestamp").and_then(parse_timestamp_to_ms) {
                        created_at.get_or_insert(ts);
                    }
                }
            }
            if first_user_message.is_none() {
                first_user_message = first_user_text(&value);
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

        for line in tail.iter().rev() {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            if last_active_at.is_none() {
                last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
            }
            if summary.is_none()
                && value.get("type").and_then(|v| v.as_str()) == Some("response_item")
            {
                if let Some(payload) = value.get("payload") {
                    if payload.get("type").and_then(|v| v.as_str()) == Some("message") {
                        let text = extract_text(
                            payload.get("content").unwrap_or(&serde_json::Value::Null),
                        );
                        if !text.trim().is_empty() {
                            summary = Some(text);
                        }
                    }
                }
            }
            if last_active_at.is_some() && summary.is_some() {
                break;
            }
        }

        let session_id = session_id.or_else(|| session_id_from_filename(path))?;

        let title = first_user_message
            .map(|t| truncate(&t, 80))
            .or_else(|| project_dir.as_deref().and_then(dir_basename));

        Some(SessionMeta {
            provider_id: "codex".to_string(),
            session_id: session_id.clone(),
            title,
            summary: summary.map(|text| truncate(&text, 160)),
            project_dir,
            project_group: project_group(path),
            created_at,
            last_active_at,
            source_path: Some(path.to_string_lossy().to_string()),
            resume_command: Some(format!("codex resume {session_id}")),
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

            if value.get("type").and_then(|v| v.as_str()) != Some("response_item") {
                continue;
            }
            let Some(payload) = value.get("payload") else {
                continue;
            };

            let (role, content) = match payload.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "message" => {
                    let role = payload
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let content =
                        extract_text(payload.get("content").unwrap_or(&serde_json::Value::Null));
                    (role, content)
                }
                "function_call" => {
                    let name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    ("assistant".to_string(), format!("[Tool: {name}]"))
                }
                "function_call_output" => {
                    let output = payload
                        .get("output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    ("tool".to_string(), output)
                }
                _ => continue,
            };

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

/// The first real user message in a `response_item` line, if any.
fn first_user_text(value: &serde_json::Value) -> Option<String> {
    if value.get("type").and_then(|v| v.as_str()) != Some("response_item") {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("type").and_then(|v| v.as_str()) != Some("message")
        || payload.get("role").and_then(|v| v.as_str()) != Some("user")
    {
        return None;
    }
    let text = extract_text(payload.get("content").unwrap_or(&serde_json::Value::Null));
    let trimmed = text.trim();
    // Codex prepends agent instructions / environment context to the first turn.
    if trimmed.is_empty()
        || trimmed.starts_with("# AGENTS.md")
        || trimmed.starts_with("<environment_context>")
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// Recover the session UUID embedded in a `rollout-<uuid>.jsonl` filename.
fn session_id_from_filename(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let re = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .ok()?;
    re.find(name).map(|m| m.as_str().to_string())
}

/// Codex groups sessions by the date directories: `YYYY/MM/DD` → `YYYY-MM`.
fn project_group(path: &Path) -> Option<String> {
    let day = path.parent()?;
    let month = day.parent()?;
    let year = month.parent()?;
    Some(format!(
        "{}-{}",
        year.file_name()?.to_str()?,
        month.file_name()?.to_str()?
    ))
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
    fn parse_reads_session_meta_and_groups_by_month() {
        let dir = tempdir().unwrap();
        let uuid = "11111111-2222-3333-4444-555555555555";
        let file = dir.path().join(format!("2026/03/06/rollout-{uuid}.jsonl"));
        write(
            &file,
            concat!(
                r#"{"type":"session_meta","payload":{"id":"sess-codex","cwd":"/work/app","timestamp":"2026-03-06T10:00:00Z"}}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"do the thing"}]}}"#,
            ),
        );

        let meta = CodexProvider.parse_session(&file).unwrap();
        assert_eq!(meta.provider_id, "codex");
        assert_eq!(meta.session_id, "sess-codex");
        assert_eq!(meta.title.as_deref(), Some("do the thing"));
        assert_eq!(meta.project_dir.as_deref(), Some("/work/app"));
        assert_eq!(meta.project_group.as_deref(), Some("2026-03"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("codex resume sess-codex")
        );
    }

    #[test]
    fn parse_falls_back_to_the_uuid_in_the_filename() {
        let dir = tempdir().unwrap();
        let uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let file = dir.path().join(format!("2026/03/06/rollout-{uuid}.jsonl"));
        // No session_meta line at all.
        write(
            &file,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":"hi"},"timestamp":"2026-03-06T10:00:00Z"}"#,
        );

        let meta = CodexProvider.parse_session(&file).unwrap();
        assert_eq!(meta.session_id, uuid);
    }

    #[test]
    fn parse_ignores_agent_instructions_as_the_title() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("2026/03/06/rollout-x.jsonl");
        write(
            &file,
            concat!(
                r#"{"type":"session_meta","payload":{"id":"s1","cwd":"/work/app"}}"#,
                "\n",
                r##"{"type":"response_item","payload":{"type":"message","role":"user","content":"# AGENTS.md\nrules here"}}"##,
                "\n",
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":"the real question"}}"#,
            ),
        );

        let meta = CodexProvider.parse_session(&file).unwrap();
        assert_eq!(meta.title.as_deref(), Some("the real question"));
    }

    #[test]
    fn load_messages_maps_calls_and_outputs() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("rollout-x.jsonl");
        write(
            &file,
            concat!(
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"list files"}]},"timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell"}}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"function_call_output","output":"a.txt"}}"#,
            ),
        );

        let msgs = CodexProvider.load_messages(file.to_str().unwrap()).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "[Tool: shell]");
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].content, "a.txt");
    }
}
