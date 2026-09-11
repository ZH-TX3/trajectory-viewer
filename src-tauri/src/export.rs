// ── Session Export ───────────────────────────────────────────────────────
//
// Renders one session to Markdown (human-readable transcript) or JSONL (the
// normalized TrajectoryEvent stream, one JSON object per line). Both are
// produced from the same parsed data the viewer already uses, so an export
// matches what is on screen.

use std::fmt::Write as _;

use crate::session_manager::{self, SessionMessage};
use crate::trajectory::{self, TrajectoryEvent};

pub const MARKDOWN_EXT: &str = "md";
pub const JSONL_EXT: &str = "jsonl";

/// Build a Markdown transcript for a session.
pub fn to_markdown(
    title: &str,
    provider_id: &str,
    session_id: &str,
    messages: &[SessionMessage],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {title}\n");
    let _ = writeln!(out, "- Provider: `{provider_id}`");
    let _ = writeln!(out, "- Session: `{session_id}`");
    let _ = writeln!(out, "- Messages: {}", messages.len());
    if let (Some(first), Some(last)) = (
        messages.first().and_then(|m| m.ts),
        messages.last().and_then(|m| m.ts),
    ) {
        let _ = writeln!(out, "- Span: {} → {}", format_ts(first), format_ts(last));
    }
    out.push('\n');

    for msg in messages {
        let heading = match msg.role.as_str() {
            "user" => "## User",
            "assistant" => "## Assistant",
            "tool" => "## Tool",
            other => {
                out.push_str(&format!("## {other}\n\n"));
                out.push_str(msg.content.trim_end());
                out.push_str("\n\n");
                continue;
            }
        };
        let _ = writeln!(out, "{heading}");
        if let Some(ts) = msg.ts {
            let _ = writeln!(out, "\n_{}_\n", format_ts(ts));
        } else {
            out.push('\n');
        }
        out.push_str(msg.content.trim_end());
        out.push_str("\n\n");
    }

    out
}

/// Build JSONL from trajectory events (one compact JSON object per line).
pub fn to_jsonl(events: &[TrajectoryEvent]) -> String {
    let mut out = String::new();
    for event in events {
        match serde_json::to_string(event) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            // A single unserializable event shouldn't abort the export.
            Err(err) => {
                eprintln!("[export] skipping unserializable event: {err}");
            }
        }
    }
    out
}

/// Export a session and write it to `target_path`.
///
/// `format` is `"md"` or `"jsonl"` (anything else is rejected). Returns the
/// number of messages/events written.
pub fn export_session(
    provider_id: &str,
    source_path: &str,
    title: &str,
    format: &str,
    target_path: &str,
) -> Result<usize, String> {
    let (content, count) = match format {
        MARKDOWN_EXT => {
            let messages = session_manager::load_messages(provider_id, source_path)?;
            let session_id = trajectory::parse_trajectory(provider_id, source_path)
                .map(|d| d.session_id)
                .unwrap_or_else(|_| "unknown".to_string());
            let count = messages.len();
            (
                to_markdown(title, provider_id, &session_id, &messages),
                count,
            )
        }
        JSONL_EXT => {
            let data = trajectory::parse_trajectory(provider_id, source_path)?;
            let count = data.events.len();
            (to_jsonl(&data.events), count)
        }
        other => return Err(format!("Unsupported export format: {other}")),
    };

    std::fs::write(target_path, content)
        .map_err(|e| format!("Failed to write {target_path}: {e}"))?;
    Ok(count)
}

/// Local-time HH:MM:SS for a millisecond timestamp (export header/metadata).
fn format_ts(ms: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => ms.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::TrajectoryEvent;

    fn message(role: &str, content: &str, ts: Option<i64>) -> SessionMessage {
        SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            ts,
        }
    }

    fn event(seq: usize, event_type: &str, content: Option<&str>) -> TrajectoryEvent {
        TrajectoryEvent {
            seq,
            ts: 1_700_000_000_000,
            event_type: event_type.to_string(),
            role: Some("user".to_string()),
            content: content.map(|s| s.to_string()),
            content_blocks: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            is_error: None,
            turn: Some(1),
            step: Some(0),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            model: None,
            provider: Some("claude".to_string()),
        }
    }

    #[test]
    fn markdown_has_header_and_roles() {
        let md = to_markdown(
            "My Session",
            "claude",
            "sess-1",
            &[
                message("user", "hello", Some(1_700_000_000_000)),
                message("assistant", "hi there", Some(1_700_000_001_000)),
                message("tool", "file.txt", None),
            ],
        );

        assert!(md.starts_with("# My Session\n"));
        assert!(md.contains("- Provider: `claude`"));
        assert!(md.contains("- Session: `sess-1`"));
        assert!(md.contains("- Messages: 3"));
        assert!(md.contains("## User"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("## Tool"));
        assert!(md.contains("hi there"));
    }

    #[test]
    fn markdown_handles_an_empty_session() {
        let md = to_markdown("Empty", "codex", "sess-0", &[]);
        assert!(md.contains("- Messages: 0"));
        assert!(!md.contains("## User"));
    }

    #[test]
    fn jsonl_emits_one_object_per_line() {
        let jsonl = to_jsonl(&[
            event(1, "user-message", Some("hello")),
            event(2, "assistant-message", None),
        ]);
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["seq"], 1);
        assert_eq!(first["eventType"], "user-message");
        assert_eq!(first["content"], "hello");
        // Null content must still serialize (opencode tool-only rounds).
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert!(second["content"].is_null());
    }

    #[test]
    fn jsonl_of_no_events_is_empty() {
        assert!(to_jsonl(&[]).is_empty());
    }

    #[test]
    fn export_rejects_unknown_format() {
        let err = export_session("claude", "/nope", "t", "pdf", "/tmp/x.pdf").unwrap_err();
        assert!(err.contains("Unsupported export format"));
    }

    /// Real-data smoke test: export one session per provider and report the
    /// sizes (run with `--ignored`).
    #[test]
    #[ignore]
    fn export_real_sessions() {
        let sessions = session_manager::scan_sessions();
        let mut out = String::new();
        let mut seen = std::collections::HashSet::new();
        for session in &sessions {
            let Some(source) = session.source_path.as_deref() else {
                continue;
            };
            // One representative per provider, whatever their list order.
            if !seen.insert(session.provider_id.clone()) {
                continue;
            }
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| session.session_id.clone());
            let dir = std::env::temp_dir();
            let md_path = dir.join(format!("export_{}.md", session.provider_id));
            let jsonl_path = dir.join(format!("export_{}.jsonl", session.provider_id));

            let md_count = export_session(
                &session.provider_id,
                source,
                &title,
                MARKDOWN_EXT,
                md_path.to_str().unwrap(),
            );
            let jsonl_count = export_session(
                &session.provider_id,
                source,
                &title,
                JSONL_EXT,
                jsonl_path.to_str().unwrap(),
            );

            let md_len = std::fs::metadata(&md_path).map(|m| m.len()).unwrap_or(0);
            let jsonl_len = std::fs::metadata(&jsonl_path).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!(
                "{:>9} md={:?} jsonl={:?} (md {md_len}B, jsonl {jsonl_len}B)\n",
                session.provider_id,
                md_count.as_ref().map(|c| *c).map_err(|e| e.clone()),
                jsonl_count.as_ref().map(|c| *c).map_err(|e| e.clone()),
            ));
        }
        let _ = std::fs::write(std::env::temp_dir().join("export_smoke.txt"), out);
    }
}
