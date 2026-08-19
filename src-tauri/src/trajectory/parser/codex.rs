// ── Codex Trajectory Parser ──────────────────────────────────────────────
//
// Parses Codex session files (JSONL) into structured TrajectoryEvent lists.
// Codex uses `type: "response_item"` with `payload.type`:
//   - `message` → user or assistant message
//   - `function_call` → tool invocation
//   - `function_call_output` → tool execution result

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::trajectory::utils::{finalize_trajectory_timings, parse_timestamp_to_ms};
use crate::trajectory::{ContentBlock, TrajectoryEvent};

/// Parse a Codex JSONL session file into trajectory events.
pub fn parse_trajectory(path: &Path) -> Result<(String, Vec<TrajectoryEvent>), String> {
    let file = File::open(path).map_err(|e| format!("Cannot open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut events: Vec<TrajectoryEvent> = Vec::new();
    let mut seq = 0usize;
    let mut session_id = String::new();
    let mut tool_call_tracker: HashMap<String, (String, String)> = HashMap::new();
    let mut current_turn = 0usize;
    let mut current_step = 0usize;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Failed to read line: {e}"))?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let json: Value =
            serde_json::from_str(&trimmed).map_err(|e| format!("Invalid JSON: {e}"))?;

        let ts = parse_timestamp_to_ms(&json["timestamp"]).unwrap_or(0);

        // Extract session ID from the first line that has one
        if session_id.is_empty() {
            if let Some(uuid) = json["session_id"].as_str() {
                session_id = uuid.to_string();
            } else if let Some(uuid) = json["payload"]["id"].as_str() {
                session_id = uuid.to_string();
            }
        }

        let line_type = json["type"].as_str().unwrap_or("").to_string();

        // Handle session_meta type for metadata
        if line_type == "session_meta" {
            if session_id.is_empty() {
                if let Some(uuid) = json["session_id"].as_str() {
                    session_id = uuid.to_string();
                } else if let Some(uuid) = json["payload"]["id"].as_str() {
                    session_id = uuid.to_string();
                }
            }
            continue;
        }

        // Only process response_item entries for trajectory events
        if line_type != "response_item" {
            continue;
        }

        let payload = match json.get("payload") {
            Some(p) => p,
            None => continue,
        };

        let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");

        match payload_type {
            "message" => {
                seq += 1;
                let role = payload
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();

                if role == "user" {
                    current_turn += 1;
                    current_step = 0;
                }

                let content = extract_codex_text(payload);
                let content_blocks = extract_codex_content_blocks(payload);

                let event_type = match role.as_str() {
                    "user" => "user-message",
                    _ => "assistant-message",
                };

                let input_tokens = payload["usage"]["input"].as_i64();
                let output_tokens = payload["usage"]["output"].as_i64();
                let model = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: event_type.to_string(),
                    role: Some(role),
                    content: Some(content),
                    content_blocks: Some(content_blocks),
                    tool_call_id: None,
                    tool_name: None,
                    tool_args: None,
                    tool_result: None,
                    is_error: None,
                    turn: Some(current_turn),
                    step: Some(current_step),
                    duration_ms: None,
                    ttft_ms: None,
                    input_tokens,
                    output_tokens,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    model,
                    provider: Some("codex".to_string()),
                });
            }

            "function_call" => {
                seq += 1;
                if current_turn > 0 {
                    current_step = 1;
                }

                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let call_id = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("fc_{seq}"));
                let arguments = payload.get("arguments").map(|a| a.to_string());

                tool_call_tracker.insert(
                    call_id.clone(),
                    (name.clone(), arguments.clone().unwrap_or_default()),
                );

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: Some(call_id),
                    tool_name: Some(name),
                    tool_args: arguments,
                    tool_result: None,
                    is_error: None,
                    turn: Some(current_turn),
                    step: Some(current_step),
                    duration_ms: None,
                    ttft_ms: None,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    model: None,
                    provider: Some("codex".to_string()),
                });
            }

            "function_call_output" => {
                seq += 1;
                let call_id = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let output = match payload.get("output") {
                    Some(Value::String(s)) => s.clone(),
                    Some(value) => value.to_string(),
                    None => String::new(),
                };
                let is_error = payload.get("is_error").and_then(Value::as_bool);

                let (tool_name, tool_args) = tool_call_tracker
                    .get(&call_id)
                    .cloned()
                    .map(|(name, args)| (Some(name), Some(args)))
                    .unwrap_or((None, None));

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "tool-result".to_string(),
                    role: Some("tool".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: Some(call_id),
                    tool_name,
                    tool_args,
                    tool_result: Some(output),
                    is_error,
                    turn: Some(current_turn),
                    step: Some(current_step),
                    duration_ms: None,
                    ttft_ms: None,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    model: None,
                    provider: Some("codex".to_string()),
                });
            }

            _ => {
                // Unknown payload type — skip
            }
        }
    }

    // Sort events by seq to ensure chronological order
    events.sort_by_key(|e| e.seq);

    // Fill in duration / TTFT estimates after sequencing
    finalize_trajectory_timings(&mut events);

    Ok((session_id, events))
}

fn extract_codex_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(text) = item["text"].as_str() {
                    parts.push(text.to_string());
                }
            }
            parts.join("\n")
        }
        Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                text.to_string()
            } else if let Some(content) = obj.get("content").and_then(Value::as_str) {
                content.to_string()
            } else {
                value.to_string()
            }
        }
        _ => value.to_string(),
    }
}

fn extract_codex_content_blocks(payload: &Value) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    let content = match payload.get("content") {
        Some(c) => c,
        None => return blocks,
    };

    match content {
        Value::String(text) => {
            blocks.push(ContentBlock {
                block_type: "text".to_string(),
                text: Some(text.clone()),
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                image_src: None,
            });
        }
        Value::Array(arr) => {
            for item in arr {
                let block_type = item["type"].as_str().unwrap_or("text").to_string();
                let text = item["text"].as_str().map(|s| s.to_string());
                let tool_call_id = item["id"]
                    .as_str()
                    .or_else(|| item["tool_call_id"].as_str())
                    .map(|s| s.to_string());
                let tool_name = item["name"].as_str().map(|s| s.to_string());
                let tool_args = item["input"]
                    .as_object()
                    .map(|obj| serde_json::to_string(obj).unwrap_or_default());
                let image_src = item["source"]["url"]
                    .as_str()
                    .or_else(|| item["source"]["path"].as_str())
                    .or_else(|| item["image_url"].as_str())
                    .map(|s| s.to_string());

                blocks.push(ContentBlock {
                    block_type,
                    text,
                    tool_call_id,
                    tool_name,
                    tool_args,
                    image_src,
                });
            }
        }
        _ => {}
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_basic_user_assistant() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-1\",\"cwd\":\"/tmp\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:02:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Hi!\"}}\n",
            ),
        ).unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-1");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "assistant-message");
    }

    #[test]
    fn parse_function_call_and_output() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-2\",\"cwd\":\"/tmp\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"list files\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:02:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"shell\",\"arguments\":\"{\\\"cmd\\\":[\\\"ls\\\"]}\",\"call_id\":\"call_1\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:03:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call_1\",\"output\":\"file1.txt\\nfile2.txt\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:04:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done.\"}]}}\n",
            ),
        ).unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "tool-call");
        assert_eq!(events[2].event_type, "tool-result");
        assert_eq!(events[3].event_type, "assistant-message");
        assert_eq!(events[1].tool_name.as_deref(), Some("shell"));
        assert!(events[2]
            .tool_result
            .as_deref()
            .unwrap()
            .contains("file1.txt"));
    }

    #[test]
    fn parse_extracts_tokens_and_model() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-3\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\",\"usage\":{\"input\":10},\"model\":\"gpt-4o\"}}\n",
                "{\"timestamp\":\"2026-03-06T10:02:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Hi!\",\"usage\":{\"input\":10,\"output\":20},\"model\":\"gpt-4o\"}}\n",
            ),
        ).unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events[0].input_tokens, Some(10));
        assert_eq!(events[0].model.as_deref(), Some("gpt-4o"));
        assert_eq!(events[1].output_tokens, Some(20));
        assert_eq!(events[1].model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn parse_empty_session_returns_no_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "{\"timestamp\":\"2026-03-06T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-empty\"}}\n").unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-empty");
        assert!(events.is_empty());
    }
}
