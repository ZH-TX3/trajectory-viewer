// ── Claude Code Trajectory Parser ────────────────────────────────────────
//
// Parses Claude Code JSONL session files into structured TrajectoryEvent lists.
// Claude Code stores `tool_use` blocks inside `assistant` messages and
// `tool_result` blocks inside `user` messages. We split those blocks into
// separate `tool-call` / `tool-result` trajectory events.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::trajectory::{ContentBlock, TrajectoryEvent};
use crate::trajectory::utils::{finalize_trajectory_timings, parse_timestamp_to_ms};

/// Parse a Claude Code JSONL session file into trajectory events.
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

        let json: Value = serde_json::from_str(&trimmed)
            .map_err(|e| format!("Invalid JSON: {e}"))?;

        let line_type = json["type"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let ts = parse_timestamp_to_ms(&json["timestamp"]).unwrap_or(0);

        // Extract session UUID from the first line that has it
        if session_id.is_empty() {
            if let Some(uuid) = json["sessionId"].as_str() {
                session_id = uuid.to_string();
            }
        }

        match line_type.as_str() {
            "user" | "user_message" | "user-message" => {
                push_user_events(
                    &mut events, &mut seq, ts, &json,
                    &tool_call_tracker, &mut current_turn, &mut current_step,
                );
            }

            "assistant" | "assistant_message" | "assistant-message" | "text" => {
                push_assistant_events(
                    &mut events, &mut seq, ts, &json,
                    &mut tool_call_tracker, &mut current_turn, &mut current_step,
                );
            }

            "tool_call" | "tool_use" | "tool-use" => {
                seq += 1;
                if current_turn > 0 {
                    current_step = 1;
                }

                let tool_call_id = json["tool_call_id"]
                    .as_str()
                    .or_else(|| json["message"]["tool_call_id"].as_str())
                    .or_else(|| json["id"].as_str())
                    .map(|s| s.to_string());

                let tool_name = json["tool_name"]
                    .as_str()
                    .or_else(|| json["message"]["tool_name"].as_str())
                    .or_else(|| json["tool_use"]["name"].as_str())
                    .map(|s| s.to_string());

                let tool_args: Option<String> = json["tool_args"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        json["message"]["tool_args"].as_str().map(|s| s.to_string())
                    })
                    .or_else(|| {
                        json["tool_use"]["input"]
                            .as_object()
                            .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                    });

                if let (Some(id), Some(name)) = (&tool_call_id, &tool_name) {
                    tool_call_tracker.insert(
                        id.clone(),
                        (name.clone(), tool_args.clone().unwrap_or_default()),
                    );
                }

                events.push(TrajectoryEvent {
                    seq, ts,
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None, content_blocks: None,
                    tool_call_id, tool_name, tool_args, tool_result: None, is_error: None,
                    turn: Some(current_turn), step: Some(current_step),
                    duration_ms: None, ttft_ms: None,
                    input_tokens: None, output_tokens: None, reasoning_tokens: None,
                    cache_read_tokens: None, cache_write_tokens: None,
                    model: None, provider: Some("claude".to_string()),
                });
            }

            "tool_result" | "tool-result" | "tool_output" | "tool_output_from_task" => {
                seq += 1;
                let tool_call_id = json["tool_call_id"]
                    .as_str()
                    .or_else(|| json["message"]["tool_call_id"].as_str())
                    .map(|s| s.to_string());

                let tool_result = json["tool_result"]
                    .as_str()
                    .or_else(|| json["message"]["tool_result"].as_str())
                    .or_else(|| json["message"]["content"].as_str())
                    .or_else(|| json["result"].as_str())
                    .map(|s| s.to_string());

                let is_error = json["is_error"]
                    .as_bool()
                    .or_else(|| json["message"]["is_error"].as_bool())
                    .or_else(|| json["error"].as_bool());

                let (tracked_name, tracked_args) = tool_call_id
                    .as_ref()
                    .and_then(|id| tool_call_tracker.get(id))
                    .cloned()
                    .unwrap_or((String::new(), String::new()));
                let tool_name = if tracked_name.is_empty() { None } else { Some(tracked_name) };
                let tool_args = if tracked_args.is_empty() { None } else { Some(tracked_args) };

                events.push(TrajectoryEvent {
                    seq, ts,
                    event_type: "tool-result".to_string(),
                    role: Some("user".to_string()),
                    content: None, content_blocks: None,
                    tool_call_id, tool_name, tool_args, tool_result, is_error,
                    turn: Some(current_turn), step: Some(current_step),
                    duration_ms: None, ttft_ms: None,
                    input_tokens: None, output_tokens: None, reasoning_tokens: None,
                    cache_read_tokens: None, cache_write_tokens: None,
                    model: None, provider: Some("claude".to_string()),
                });
            }

            "custom-title" | "title" => {
                // Title lines don't produce trajectory events
            }

            _ => {
                // Unknown line type — skip silently
            }
        }
    }

    // Sort events by seq to ensure chronological order
    events.sort_by_key(|e| e.seq);

    // Fill in duration / TTFT estimates after sequencing
    finalize_trajectory_timings(&mut events);

    Ok((session_id, events))
}

/// Split a Claude `user` line into `tool-result` events (one per tool_result
/// block) and a single `user-message` event for any text content.
fn push_user_events(
    events: &mut Vec<TrajectoryEvent>,
    seq: &mut usize,
    ts: i64,
    json: &Value,
    tool_call_tracker: &HashMap<String, (String, String)>,
    current_turn: &mut usize,
    current_step: &mut usize,
) {
    let message = &json["message"];
    let mut user_texts: Vec<String> = Vec::new();
    let mut tool_results: Vec<(Option<String>, String, Option<bool>)> = Vec::new();

    if let Some(items) = message["content"].as_array() {
        for item in items {
            let item_type = item["type"].as_str().unwrap_or("");
            if item_type == "tool_result" {
                let call_id = item["tool_use_id"]
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .map(|s| s.to_string());
                let result = item["content"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| extract_text(&item["content"]));
                let is_error = item["is_error"]
                    .as_bool()
                    .or_else(|| json["is_error"].as_bool());
                tool_results.push((call_id, result, is_error));
            } else {
                let text = item["text"]
                    .as_str()
                    .or_else(|| item["content"].as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    user_texts.push(text.to_string());
                }
            }
        }
    } else {
        let text = extract_text_content(message);
        if !text.trim().is_empty() {
            user_texts.push(text);
        }
    }

    // Only a real user text message starts a new turn. Tool-result-only
    // user lines continue the current assistant turn/step.
    let has_user_text = !user_texts.is_empty();
    if has_user_text {
        *current_turn += 1;
        *current_step = 0;
    }

    // Emit tool-result events first so seq stays chronological
    for (call_id, result, is_error) in tool_results {
        *seq += 1;
        let (tracked_name, tracked_args) = call_id
            .as_ref()
            .and_then(|id| tool_call_tracker.get(id))
            .cloned()
            .unwrap_or((String::new(), String::new()));
        let tool_name = if tracked_name.is_empty() { None } else { Some(tracked_name) };
        let tool_args = if tracked_args.is_empty() { None } else { Some(tracked_args) };

        events.push(TrajectoryEvent {
            seq: *seq, ts,
            event_type: "tool-result".to_string(),
            role: Some("user".to_string()),
            content: None, content_blocks: None,
            tool_call_id: call_id, tool_name, tool_args,
            tool_result: Some(result), is_error,
            turn: Some(*current_turn), step: Some(1),
            duration_ms: None, ttft_ms: None,
            input_tokens: None, output_tokens: None, reasoning_tokens: None,
            cache_read_tokens: None, cache_write_tokens: None,
            model: None, provider: Some("claude".to_string()),
        });
    }

    if has_user_text {
        *seq += 1;
        let content = user_texts.join("\n");
        events.push(TrajectoryEvent {
            seq: *seq, ts,
            event_type: "user-message".to_string(),
            role: Some("user".to_string()),
            content: Some(content.clone()),
            content_blocks: Some(extract_content_blocks(message)),
            tool_call_id: None, tool_name: None, tool_args: None, tool_result: None, is_error: None,
            turn: Some(*current_turn), step: Some(0),
            duration_ms: None, ttft_ms: None,
            input_tokens: json["usage"]["input"].as_i64(),
            output_tokens: None, reasoning_tokens: None,
            cache_read_tokens: json["usage"]["cache_read_input_tokens"].as_i64(),
            cache_write_tokens: json["usage"]["cache_creation_input_tokens"].as_i64(),
            model: json["model"].as_str().map(|s| s.to_string()),
            provider: Some("claude".to_string()),
        });
    }
}

/// Split a Claude `assistant` line into `tool-call` events (one per tool_use
/// block) and a single `assistant-message` event for any text content.
fn push_assistant_events(
    events: &mut Vec<TrajectoryEvent>,
    seq: &mut usize,
    ts: i64,
    json: &Value,
    tool_call_tracker: &mut HashMap<String, (String, String)>,
    current_turn: &mut usize,
    current_step: &mut usize,
) {
    let message = &json["message"];
    let assistant_step = *current_step;
    let mut text_parts: Vec<String> = Vec::new();
    let mut saw_tool_use = false;

    if let Some(items) = message["content"].as_array() {
        for item in items {
            let item_type = item["type"].as_str().unwrap_or("");
            if item_type == "tool_use" {
                saw_tool_use = true;
                if *current_turn > 0 {
                    *current_step = 1;
                }

                *seq += 1;
                let call_id = item["id"]
                    .as_str()
                    .or_else(|| item["tool_call_id"].as_str())
                    .map(|s| s.to_string());
                let tool_name = item["name"]
                    .as_str()
                    .or_else(|| item["tool_name"].as_str())
                    .map(|s| s.to_string());
                let tool_args = item["input"]
                    .as_object()
                    .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                    .or_else(|| item["input"].as_str().map(|s| s.to_string()))
                    .or_else(|| item["tool_args"].as_str().map(|s| s.to_string()));

                if let (Some(id), Some(name)) = (&call_id, &tool_name) {
                    tool_call_tracker.insert(
                        id.clone(),
                        (name.clone(), tool_args.clone().unwrap_or_default()),
                    );
                }

                events.push(TrajectoryEvent {
                    seq: *seq, ts,
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None, content_blocks: None,
                    tool_call_id: call_id, tool_name, tool_args,
                    tool_result: None, is_error: None,
                    turn: Some(*current_turn), step: Some(*current_step),
                    duration_ms: None, ttft_ms: None,
                    input_tokens: None, output_tokens: None, reasoning_tokens: None,
                    cache_read_tokens: None, cache_write_tokens: None,
                    model: None, provider: Some("claude".to_string()),
                });
            } else {
                let text = item["text"]
                    .as_str()
                    .or_else(|| item["content"].as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    text_parts.push(text.to_string());
                }
            }
        }
    } else {
        let text = extract_text_content(message);
        if !text.trim().is_empty() {
            text_parts.push(text);
        }
    }

    if !text_parts.is_empty() {
        *seq += 1;
        let content = text_parts.join("\n");
        let model = json["model"]
            .as_str()
            .or_else(|| message["model"].as_str())
            .map(|s| s.to_string());
        let input_tokens = message["usage"]["input"]
            .as_i64()
            .or_else(|| json["usage"]["input"].as_i64());
        let output_tokens = message["usage"]["output"]
            .as_i64()
            .or_else(|| json["usage"]["output"].as_i64());
        let reasoning_tokens = message["usage"]["reasoning"]
            .as_i64()
            .or_else(|| json["usage"]["reasoning"].as_i64());

        events.push(TrajectoryEvent {
            seq: *seq, ts,
            event_type: "assistant-message".to_string(),
            role: Some("assistant".to_string()),
            content: Some(content.clone()),
            content_blocks: Some(extract_content_blocks(message)),
            tool_call_id: None, tool_name: None, tool_args: None, tool_result: None, is_error: None,
            turn: Some(*current_turn),
            step: Some(if saw_tool_use { 1 } else { assistant_step }),
            duration_ms: None, ttft_ms: None,
            input_tokens, output_tokens, reasoning_tokens,
            cache_read_tokens: json["usage"]["cache_read_input_tokens"].as_i64(),
            cache_write_tokens: json["usage"]["cache_creation_input_tokens"].as_i64(),
            model,
            provider: Some("claude".to_string()),
        });
    }
}

// ── Text extraction helpers ───────────────────────────────────────────────

fn extract_text_content(msg: &Value) -> String {
    if let Some(text) = msg.as_str() {
        return text.to_string();
    }
    if let Some(obj) = msg.as_object() {
        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
            return text.to_string();
        }
        if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
            return content.to_string();
        }
    }
    if let Some(arr) = msg.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    String::new()
}

fn extract_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item["text"].as_str().map(|s| s.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn extract_content_blocks(msg: &Value) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    if let Some(text) = msg.as_str() {
        blocks.push(ContentBlock {
            block_type: "text".to_string(),
            text: Some(text.to_string()),
            tool_call_id: None, tool_name: None, tool_args: None, image_src: None,
        });
        return blocks;
    }

    if let Some(arr) = msg.as_array() {
        for item in arr {
            let block_type = item["type"].as_str().unwrap_or("text").to_string();
            let text = item["text"].as_str()
                .or_else(|| item["content"].as_str())
                .map(|s| s.to_string());
            let tool_call_id = item["id"].as_str()
                .or_else(|| item["tool_call_id"].as_str())
                .map(|s| s.to_string());
            let tool_name = item["name"].as_str()
                .or_else(|| item["tool_name"].as_str())
                .map(|s| s.to_string());
            let tool_args = item["input"].as_object()
                .map(|obj| serde_json::to_string(obj).unwrap_or_default())
                .or_else(|| item["tool_args"].as_str().map(|s| s.to_string()));
            let image_src = item["source"]["url"].as_str()
                .or_else(|| item["source"]["path"].as_str())
                .or_else(|| item["image_url"].as_str())
                .map(|s| s.to_string());

            blocks.push(ContentBlock {
                block_type, text, tool_call_id, tool_name, tool_args, image_src,
            });
        }
        return blocks;
    }

    if let Some(obj) = msg.as_object() {
        let block_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("text").to_string();
        let text = obj.get("text").and_then(|v| v.as_str())
            .or_else(|| obj.get("content").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_call_id = obj.get("id").and_then(|v| v.as_str())
            .or_else(|| obj.get("tool_call_id").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_name = obj.get("name").and_then(|v| v.as_str())
            .or_else(|| obj.get("tool_name").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_args = obj.get("input").and_then(|v| v.as_object())
            .map(|obj| serde_json::to_string(obj).unwrap_or_default())
            .or_else(|| obj.get("tool_args").and_then(|v| v.as_str()).map(|s| s.to_string()));
        let image_src = obj.get("source").and_then(|v| v.get("url")).and_then(|v| v.as_str())
            .or_else(|| obj.get("source").and_then(|v| v.get("path")).and_then(|v| v.as_str()))
            .or_else(|| obj.get("image_url").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        blocks.push(ContentBlock {
            block_type, text, tool_call_id, tool_name, tool_args, image_src,
        });
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
                "{\"sessionId\":\"sess-1\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hi there!\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        ).unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-1");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "assistant-message");
        assert_eq!(events[0].turn, Some(1));
        assert_eq!(events[1].turn, Some(1));
    }

    #[test]
    fn parse_tool_use_and_result() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-2\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"list files\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Let me check.\"},{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}]},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"file1.txt\\nfile2.txt\"}]},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
            ),
        ).unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-2");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "tool-call");
        assert_eq!(events[2].event_type, "assistant-message");
        assert_eq!(events[3].event_type, "tool-result");
        assert_eq!(events[1].tool_name.as_deref(), Some("Bash"));
        assert!(events[3].tool_result.as_deref().unwrap().contains("file1.txt"));
    }

    #[test]
    fn parse_extracts_tokens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-3\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"usage\":{\"input\":10,\"cache_read_input_tokens\":5,\"cache_creation_input_tokens\":3},\"model\":\"claude-sonnet-4\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hi!\",\"usage\":{\"input\":10,\"output\":5,\"reasoning\":2}},\"model\":\"claude-sonnet-4\",\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        ).unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events[0].input_tokens, Some(10));
        assert_eq!(events[0].cache_read_tokens, Some(5));
        assert_eq!(events[0].cache_write_tokens, Some(3));
        assert_eq!(events[0].model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(events[1].output_tokens, Some(5));
        assert_eq!(events[1].reasoning_tokens, Some(2));
        assert_eq!(events[1].model.as_deref(), Some("claude-sonnet-4"));
    }

    #[test]
    fn parse_timestamps_are_chronological() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-4\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"a\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"b\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"c\"},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"d\"},\"timestamp\":\"2026-03-06T10:04:00Z\"}\n",
            ),
        ).unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events.len(), 4);
        for i in 1..events.len() {
            assert!(events[i].ts >= events[i-1].ts, "events should be chronological");
        }
        // Two turns
        assert_eq!(events[0].turn, Some(1));
        assert_eq!(events[2].turn, Some(2));
    }

    #[test]
    fn parse_empty_session_returns_no_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "{\"sessionId\":\"sess-empty\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n").unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-empty");
        assert!(events.is_empty());
    }
}