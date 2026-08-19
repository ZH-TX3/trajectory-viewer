// ── DSH (DeepSeek Harness) Trajectory Parser ─────────────────────────────
//
// Parses DSH session files (zstd-compressed JSONL) into structured
// TrajectoryEvent lists.
//
// DSH format:
// - Stored as ~/.dsh/sessions/{project}/{session-id}/session.jsonl.zstd
// - zstd-compressed JSONL with event types:
//   - session: metadata (id, createdAt, cwd)
//   - user/message: user input
//   - assistant/message: assistant response (content blocks may include tool-calls)
//   - assistant/chunk: streaming chunks (block-start, text-delta, tool-call-delta, etc.)
//   - tool/call: tool invocation
//   - tool/result: tool execution result
//   - turn/start, turn/end, step/start, step/end: turn/step boundaries

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::trajectory::utils::{finalize_trajectory_timings, parse_timestamp_to_ms};
use crate::trajectory::{ContentBlock, TrajectoryEvent};

/// Parse a DSH session file (zstd-compressed JSONL) into trajectory events.
pub fn parse_trajectory(path: &Path) -> Result<(String, Vec<TrajectoryEvent>), String> {
    let mut file = File::open(path).map_err(|e| format!("Cannot open session file: {e}"))?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)
        .map_err(|e| format!("Failed to read session file: {e}"))?;

    let decompressed = zstd::decode_all(std::io::Cursor::new(&compressed))
        .map_err(|e| format!("Failed to decompress session file: {e}"))?;

    let content = String::from_utf8(decompressed)
        .map_err(|e| format!("Invalid UTF-8 in session file: {e}"))?;

    let mut events: Vec<TrajectoryEvent> = Vec::new();
    let mut seq = 0usize;
    let mut session_id = String::new();
    let mut tool_call_tracker: HashMap<String, (String, String)> = HashMap::new();
    let mut current_turn = 0usize;
    let mut current_step = 0usize;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let json: Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("Invalid JSON in DSH session: {e}"))?;

        let line_type = json["type"].as_str().unwrap_or("").to_string();
        let ts = parse_dsh_timestamp(&json["time"]).unwrap_or(0);

        // Extract session ID from the session metadata line
        if session_id.is_empty() && line_type == "session" {
            if let Some(id) = json["id"].as_str() {
                session_id = id.to_string();
            }
        }

        match line_type.as_str() {
            "user/message" => {
                seq += 1;

                // A user message starts a new turn
                current_turn += 1;
                current_step = 0;

                let data = &json["data"];
                let content = extract_content_from_blocks(&data["content"]);
                let content_blocks = extract_content_blocks_from_value(&data["content"]);

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "user-message".to_string(),
                    role: Some("user".to_string()),
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
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    model: None,
                    provider: Some("dsh".to_string()),
                });
            }

            "assistant/message" => {
                seq += 1;

                // Update step from data
                if let Some(step) = json["data"]["step"].as_u64() {
                    current_step = step as usize;
                }

                let data = &json["data"];
                let message = &data["message"];

                // Extract text content
                let content = message["content"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                if item["type"].as_str() == Some("text") {
                                    item["text"].as_str().map(|s| s.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();

                let content_blocks = extract_content_blocks_from_value(&message["content"]);

                // Extract token usage from the last chunk
                let (
                    input_tokens,
                    output_tokens,
                    reasoning_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                ) = (None, None, None, None, None);

                let model = json["data"]["model"]
                    .as_str()
                    .or_else(|| message["model"].as_str())
                    .map(|s| s.to_string());

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "assistant-message".to_string(),
                    role: Some("assistant".to_string()),
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
                    reasoning_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                    model,
                    provider: Some("dsh".to_string()),
                });
            }

            "tool/call" => {
                seq += 1;
                if current_turn > 0 {
                    current_step = 1;
                }

                let data = &json["data"];

                let call_id = data["callId"]
                    .as_str()
                    .or_else(|| data["toolCall"]["id"].as_str())
                    .map(|s| s.to_string());

                let tool_name = data["name"]
                    .as_str()
                    .or_else(|| data["toolCall"]["name"].as_str())
                    .map(|s| s.to_string());

                let tool_args = data["arguments"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        data["toolCall"]["arguments"]
                            .as_str()
                            .map(|s| s.to_string())
                    })
                    .or_else(|| {
                        data["toolCall"]["input"]
                            .as_object()
                            .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                    });

                if let (Some(id), Some(name)) = (&call_id, &tool_name) {
                    tool_call_tracker.insert(
                        id.clone(),
                        (name.clone(), tool_args.clone().unwrap_or_default()),
                    );
                }

                // Try to get step from data
                if let Some(step) = data["step"].as_u64() {
                    current_step = step as usize;
                }

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: call_id,
                    tool_name,
                    tool_args,
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
                    provider: Some("dsh".to_string()),
                });
            }

            "tool/result" => {
                seq += 1;

                let data = &json["data"];
                let message = &data["message"];

                if let Some(step) = data["step"].as_u64() {
                    current_step = step as usize;
                }

                let call_id = message["source"]["callId"].as_str().map(|s| s.to_string());

                let tool_result = if let Some(content) = message["content"].as_array() {
                    Some(
                        content
                            .iter()
                            .filter_map(|item| {
                                if item["type"].as_str() == Some("text") {
                                    item["text"].as_str().map(|s| s.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                } else {
                    None
                };

                let is_error = data["isError"]
                    .as_bool()
                    .or_else(|| message["isError"].as_bool());

                // Look up the tool name from tracker
                let (tracked_name, tracked_args) = call_id
                    .as_ref()
                    .and_then(|id| tool_call_tracker.get(id))
                    .cloned()
                    .unwrap_or((String::new(), String::new()));
                let tool_name = if tracked_name.is_empty() {
                    None
                } else {
                    Some(tracked_name)
                };
                let tool_args = if tracked_args.is_empty() {
                    None
                } else {
                    Some(tracked_args)
                };

                events.push(TrajectoryEvent {
                    seq,
                    ts,
                    event_type: "tool-result".to_string(),
                    role: Some("tool".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: call_id,
                    tool_name,
                    tool_args,
                    tool_result,
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
                    provider: Some("dsh".to_string()),
                });
            }

            // Skip streaming chunks; token usage is extracted from assistant/message events
            "assistant/chunk" => {}

            // Skip turn/step boundaries and other metadata
            "turn/start"
            | "turn/end"
            | "step/start"
            | "step/end"
            | "session/title"
            | "session/title-llm-request"
            | "session/end-seed"
            | "request/header"
            | "request/context"
            | "agent/inbox/spliced"
            | "permission/preset"
            | "sandbox/mode"
            | "approval/policy"
            | "text-chunks"
            | "tool-call-chunks" => {}

            "session" => {
                // Session metadata already handled above
            }

            _ => {
                // Unknown line type — skip
            }
        }
    }

    // Sort events by seq to ensure chronological order
    events.sort_by_key(|e| e.seq);

    // Fill in duration / TTFT estimates
    finalize_trajectory_timings(&mut events);

    Ok((session_id, events))
}

/// Parse a DSH timestamp value (milliseconds epoch).
fn parse_dsh_timestamp(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    if let Some(n) = value.as_f64() {
        return Some(n as i64);
    }
    parse_timestamp_to_ms(value)
}

/// Extract concatenated text from DSH content blocks array.
fn extract_content_from_blocks(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                if item["type"].as_str() == Some("text") {
                    item["text"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Extract structured content blocks from DSH content blocks array.
fn extract_content_blocks_from_value(value: &Value) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    match value {
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
                let tool_call_id = item["id"].as_str().map(|s| s.to_string());
                let tool_name = item["name"].as_str().map(|s| s.to_string());
                let tool_args = item["input"]
                    .as_object()
                    .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                    .or_else(|| item["arguments"].as_str().map(|s| s.to_string()));
                let image_src = item["source"]["url"]
                    .as_str()
                    .or_else(|| item["source"]["path"].as_str())
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
        Value::Object(obj) => {
            let block_type = obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("text")
                .to_string();
            let text = obj
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let tool_call_id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let tool_name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let tool_args = obj
                .get("input")
                .and_then(|v| v.as_object())
                .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                .or_else(|| {
                    obj.get("arguments")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            let image_src = None;

            blocks.push(ContentBlock {
                block_type,
                text,
                tool_call_id,
                tool_name,
                tool_args,
                image_src,
            });
        }
        _ => {}
    }

    blocks
}

/// Load messages from a DSH session file for the messages view.
pub fn load_messages(path: &Path) -> Result<Vec<crate::session_manager::SessionMessage>, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)
        .map_err(|e| format!("Failed to read session file: {e}"))?;

    let decompressed = zstd::decode_all(std::io::Cursor::new(&compressed))
        .map_err(|e| format!("Failed to decompress: {e}"))?;

    let content = String::from_utf8(decompressed).map_err(|e| format!("Invalid UTF-8: {e}"))?;

    let mut messages = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let json: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON: {e}"))?;

        let line_type = json["type"].as_str().unwrap_or("");

        match line_type {
            "user/message" => {
                let data = &json["data"];
                let content = extract_content_from_blocks(&data["content"]);
                if !content.trim().is_empty() {
                    let ts = parse_dsh_timestamp(&json["time"]);
                    messages.push(crate::session_manager::SessionMessage {
                        role: "user".to_string(),
                        content,
                        ts,
                    });
                }
            }
            "assistant/message" => {
                let message = &json["data"]["message"];
                let content = message["content"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| match item["type"].as_str() {
                                Some("text") => item["text"].as_str().map(|s| s.to_string()),
                                Some("tool-call") => {
                                    let name = item["name"].as_str().unwrap_or("unknown");
                                    Some(format!("[Tool: {name}]"))
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    let ts = parse_dsh_timestamp(&json["time"]);
                    messages.push(crate::session_manager::SessionMessage {
                        role: "assistant".to_string(),
                        content,
                        ts,
                    });
                }
            }
            "tool/result" => {
                let message = &json["data"]["message"];
                let content = message["content"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                if item["type"].as_str() == Some("text") {
                                    item["text"].as_str().map(|s| s.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    let ts = parse_dsh_timestamp(&json["time"]);
                    messages.push(crate::session_manager::SessionMessage {
                        role: "tool".to_string(),
                        content,
                        ts,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Create a minimal DSH session file for testing.
    fn create_dsh_session(path: &Path, lines: &[&str]) {
        use std::io::Write;
        let content = lines.join("\n");
        let compressed = zstd::encode_all(std::io::Cursor::new(content.as_bytes()), 0).unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(&compressed).unwrap();
    }

    #[test]
    fn parse_dsh_session_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl.zstd");

        create_dsh_session(
            &path,
            &[
                r#"{"type":"session","id":"sess-1","createdAt":1787023345223,"cwd":"/tmp/project","delegationDepth":0,"agentPreset":"router-standard"}"#,
                r#"{"type":"user/message","seq":1,"time":1787023346000,"data":{"content":[{"type":"text","text":"hello"}]}}"#,
                r#"{"type":"assistant/message","seq":2,"time":1787023347000,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"text","text":"Hi there!"}],"source":{"kind":"assistant","model":"deepseek-chat"}}}}"#,
            ],
        );

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-1");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "assistant-message");
        assert_eq!(events[0].turn, Some(1));
        assert_eq!(events[1].turn, Some(1));
    }

    #[test]
    fn parse_dsh_tool_calls() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl.zstd");

        create_dsh_session(
            &path,
            &[
                r#"{"type":"session","id":"sess-2","createdAt":1787023345223,"cwd":"/tmp"}"#,
                r#"{"type":"user/message","seq":1,"time":1787023346000,"data":{"content":[{"type":"text","text":"list files"}]}}"#,
                r#"{"type":"assistant/message","seq":2,"time":1787023347000,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"text","text":"Checking..."},{"type":"tool-call","id":"call_1","name":"Bash","input":{"command":"ls"}}],"source":{"kind":"assistant","model":"deepseek-chat"}}}}"#,
                r#"{"type":"tool/call","seq":3,"time":1787023347001,"data":{"turn":1,"step":1,"callId":"call_1","name":"Bash","arguments":"{\"command\":\"ls\"}"}}"#,
                r#"{"type":"tool/result","seq":4,"time":1787023348000,"data":{"turn":1,"step":1,"message":{"source":{"kind":"tool","callId":"call_1"},"content":[{"type":"text","text":"file1.txt\nfile2.txt"}],"role":"tool"}}}"#,
            ],
        );

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-2");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "assistant-message");
        assert_eq!(events[2].event_type, "tool-call");
        assert_eq!(events[3].event_type, "tool-result");
        assert_eq!(events[2].tool_name.as_deref(), Some("Bash"));
        assert!(events[3]
            .tool_result
            .as_deref()
            .unwrap()
            .contains("file1.txt"));
    }

    #[test]
    fn parse_dsh_empty_session() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl.zstd");
        create_dsh_session(
            &path,
            &[r#"{"type":"session","id":"sess-empty","createdAt":1787023345223,"cwd":"/tmp"}"#],
        );

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-empty");
        assert!(events.is_empty());
    }
}
