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

use crate::trajectory::utils::{finalize_trajectory_timings, parse_timestamp_to_ms, ParseWarnings};
use crate::trajectory::{ContentBlock, TrajectoryEvent};

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
    let mut warnings = ParseWarnings::default();

    for line in reader.lines() {
        // A truncated final line (crashed session) is expected; skip it and
        // keep the rest of the session readable.
        let Ok(line) = line else {
            warnings.skip_line("unreadable line");
            continue;
        };
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let json: Value = match serde_json::from_str(&trimmed) {
            Ok(value) => value,
            Err(err) => {
                warnings.skip_line(&err.to_string());
                continue;
            }
        };

        let line_type = json["type"].as_str().unwrap_or("unknown").to_string();

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
                    &mut events,
                    &mut seq,
                    ts,
                    &json,
                    &tool_call_tracker,
                    &mut current_turn,
                    &mut current_step,
                );
            }

            "assistant" | "assistant_message" | "assistant-message" | "text" => {
                push_assistant_events(
                    &mut events,
                    &mut seq,
                    ts,
                    &json,
                    &mut tool_call_tracker,
                    &mut current_turn,
                    &mut current_step,
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
                    .or_else(|| json["message"]["tool_args"].as_str().map(|s| s.to_string()))
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
                    seq,
                    ts,
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id,
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
                    provider: Some("claude".to_string()),
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
                    role: Some("user".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id,
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
                    provider: Some("claude".to_string()),
                });
            }

            "system" => {
                push_system_event(&mut events, &mut seq, ts, &json, current_turn);
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

    if let Some(summary) = warnings.summary() {
        eprintln!("[trajectory] {}: {}", path.display(), summary);
    }

    Ok((session_id, events))
}

/// Turn a `system` line into a trajectory event, when it is one that belongs on
/// the timeline.
///
/// Only `compact_boundary` qualifies today: it is the moment the context was
/// thrown away, and it carries the metadata describing what was dropped. The
/// other subtypes of a real log - `turn_duration`, `api_error`, `away_summary`,
/// `local_command`, `informational`, `stop_hook_summary` - have no event shape
/// yet, so they stay dropped rather than becoming noise.
fn push_system_event(
    events: &mut Vec<TrajectoryEvent>,
    seq: &mut usize,
    ts: i64,
    json: &Value,
    current_turn: usize,
) {
    if json["subtype"].as_str() != Some("compact_boundary") {
        return;
    }

    *seq += 1;
    events.push(TrajectoryEvent {
        seq: *seq,
        ts,
        event_type: "compaction".to_string(),
        role: Some("system".to_string()),
        content: json["content"].as_str().map(|s| s.to_string()),
        content_blocks: None,
        tool_call_id: None,
        tool_name: None,
        tool_args: None,
        tool_result: None,
        is_error: None,
        turn: Some(current_turn),
        step: None,
        duration_ms: None,
        ttft_ms: None,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        model: None,
        provider: Some("claude".to_string()),
    });
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
    // The context left over from a compaction is replayed as a `user` line
    // carrying `isCompactSummary`. It is not user input: emitting it opened a
    // phantom turn and rendered the whole restored context as something the
    // user never said. The `compact_boundary` system line marks it instead.
    if json["isCompactSummary"].as_bool() == Some(true) {
        return;
    }
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
    // Injected context arrives as a `user` line but is not user input: system
    // reminders and subagent task notifications. Counting them as turns split
    // one assistant turn into several — every background subagent's result
    // started a phantom turn of its own.
    let is_injected = user_texts.iter().any(|text| is_injected_context(text));
    if has_user_text && !is_injected {
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
            seq: *seq,
            ts,
            event_type: "tool-result".to_string(),
            role: Some("user".to_string()),
            content: None,
            content_blocks: None,
            tool_call_id: call_id,
            tool_name,
            tool_args,
            tool_result: Some(result),
            is_error,
            turn: Some(*current_turn),
            step: Some(*current_step),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            model: None,
            provider: Some("claude".to_string()),
        });
    }

    if has_user_text {
        *seq += 1;
        let content = user_texts.join("\n");
        events.push(TrajectoryEvent {
            seq: *seq,
            ts,
            event_type: "user-message".to_string(),
            role: Some("user".to_string()),
            content: Some(content.clone()),
            content_blocks: Some(extract_content_blocks(message)),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            is_error: None,
            turn: Some(*current_turn),
            step: Some(0),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: json["usage"]["input"].as_i64(),
            output_tokens: None,
            reasoning_tokens: None,
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
    // Each assistant record is the next step of its turn: a turn is one user
    // message followed by as many request/response round-trips as it took.
    // Every record used to be pinned to step 1, so a turn of ~19 records
    // collapsed into a single "Step 1" group and the numbering said nothing.
    let step = *current_step + 1;
    let usage = Usage::read(message, json);
    let mut text_parts: Vec<String> = Vec::new();
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut saw_tool_use = false;
    // Index of the first tool-call this record pushes, if it pushes any.
    let mut first_tool_call: Option<usize> = None;

    if let Some(items) = message["content"].as_array() {
        for item in items {
            let item_type = item["type"].as_str().unwrap_or("");
            if item_type == "tool_use" {
                saw_tool_use = true;

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

                if first_tool_call.is_none() {
                    first_tool_call = Some(events.len());
                }
                events.push(TrajectoryEvent {
                    seq: *seq,
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
                    turn: Some(*current_turn),
                    step: Some(step),
                    duration_ms: None,
                    ttft_ms: None,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    model: None,
                    provider: Some("claude".to_string()),
                });
            } else if item_type == "thinking" || item_type == "reasoning" {
                // Reasoning text lives under `thinking` (or `reasoning`), not
                // `text`. Kept apart from the message body so it can be shown
                // as its own tab.
                let text = item["thinking"]
                    .as_str()
                    .or_else(|| item["reasoning"].as_str())
                    .or_else(|| item["text"].as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    thinking_parts.push(text.to_string());
                }
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

    // A message that only reasons is still a real assistant record; emitting it
    // only when `text_parts` was non-empty dropped those messages entirely.
    if !text_parts.is_empty() || !thinking_parts.is_empty() {
        *seq += 1;
        let content = text_parts.join("\n");
        let model = json["model"]
            .as_str()
            .or_else(|| message["model"].as_str())
            .map(|s| s.to_string());

        events.push(TrajectoryEvent {
            seq: *seq,
            ts,
            event_type: "assistant-message".to_string(),
            role: Some("assistant".to_string()),
            content: Some(content.clone()),
            content_blocks: Some(extract_content_blocks(message)),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            is_error: None,
            turn: Some(*current_turn),
            step: Some(step),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: usage.input,
            output_tokens: usage.output,
            reasoning_tokens: usage.reasoning,
            cache_read_tokens: usage.cache_read,
            cache_write_tokens: usage.cache_write,
            model,
            provider: Some("claude".to_string()),
        });
    } else if let Some(index) = first_tool_call {
        // A record that only calls tools is still a paid request. Emitting
        // nothing for it dropped half the records of a real session and
        // almost all of its tokens, which left the usage view nearly empty.
        // The request's usage rides on the first call it produced.
        let event = &mut events[index];
        event.input_tokens = usage.input;
        event.output_tokens = usage.output;
        event.reasoning_tokens = usage.reasoning;
        event.cache_read_tokens = usage.cache_read;
        event.cache_write_tokens = usage.cache_write;
    }

    // Only a record that produced something advances the turn's step, so a
    // stray empty record cannot leave a hole in the numbering.
    if saw_tool_use || !text_parts.is_empty() || !thinking_parts.is_empty() {
        *current_step = step;
    }
}

/// Token accounting for one assistant message.
///
/// Claude Code puts `usage` on the message (not on the JSONL line) and spells
/// every counter `*_tokens` - `input_tokens`, `output_tokens`,
/// `cache_read_input_tokens`, `cache_creation_input_tokens`. Reading the short
/// names, or reading from the line, silently yielded `None` for every real
/// session; the short names stay as a fallback so older files still parse.
#[derive(Default, Clone, Copy)]
struct Usage {
    input: Option<i64>,
    output: Option<i64>,
    reasoning: Option<i64>,
    cache_read: Option<i64>,
    cache_write: Option<i64>,
}

impl Usage {
    fn read(message: &Value, line: &Value) -> Self {
        Self {
            input: usage_number(message, line, &["input_tokens", "input"]),
            output: usage_number(message, line, &["output_tokens", "output"]),
            reasoning: usage_number(message, line, &["reasoning_tokens", "reasoning"]),
            cache_read: usage_number(message, line, &["cache_read_input_tokens"]),
            cache_write: usage_number(message, line, &["cache_creation_input_tokens"]),
        }
    }
}

/// The first of `keys` present as an integer under `message.usage` or
/// `line.usage`.
fn usage_number(message: &Value, line: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        message["usage"][key]
            .as_i64()
            .or_else(|| line["usage"][key].as_i64())
    })
}

// ── Text extraction helpers ───────────────────────────────────────────────

/// Whether a `user` line is injected context rather than user input. Claude
/// writes workspace/system reminders and subagent task notifications this way;
/// neither starts a turn.
fn is_injected_context(text: &str) -> bool {
    text.contains("<system-reminder>") || text.contains("<task-notification>")
}

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
            .filter_map(|item| item["text"].as_str().map(|s| s.to_string()))
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
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            image_src: None,
        });
        return blocks;
    }

    // Callers pass the whole `message` object, whose blocks live under
    // `content`; delegate so a message-shaped value still yields its blocks.
    if msg.is_object() {
        return msg
            .get("content")
            .map(extract_content_blocks)
            .unwrap_or_default();
    }

    if let Some(arr) = msg.as_array() {
        for item in arr {
            let block_type = item["type"].as_str().unwrap_or("text").to_string();
            // Thinking blocks carry their text under `thinking`, not `text`.
            let text = item["text"]
                .as_str()
                .or_else(|| item["thinking"].as_str())
                .or_else(|| item["reasoning"].as_str())
                .or_else(|| item["content"].as_str())
                .map(|s| s.to_string());
            let tool_call_id = item["id"]
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
                .or_else(|| item["tool_args"].as_str().map(|s| s.to_string()));
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
        return blocks;
    }

    if let Some(obj) = msg.as_object() {
        let block_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let text = obj
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("content").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_call_id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("tool_call_id").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("tool_name").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let tool_args = obj
            .get("input")
            .and_then(|v| v.as_object())
            .map(|obj| serde_json::to_string(obj).unwrap_or_default())
            .or_else(|| {
                obj.get("tool_args")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });
        let image_src = obj
            .get("source")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                obj.get("source")
                    .and_then(|v| v.get("path"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| obj.get("image_url").and_then(|v| v.as_str()))
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

    // Real transcripts put thinking text under `thinking`, not `text`; reading
    // only `text` silently dropped every reasoning block.
    #[test]
    fn parse_thinking_block_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-t\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"weighing options\",\"signature\":\"sig\"},{\"type\":\"text\",\"text\":\"answer\"}]},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let assistant = events
            .iter()
            .find(|e| e.event_type == "assistant-message")
            .expect("assistant event");
        let blocks = assistant.content_blocks.as_ref().unwrap();
        let thinking = blocks
            .iter()
            .find(|b| b.block_type == "thinking")
            .expect("thinking block");
        assert_eq!(thinking.text.as_deref(), Some("weighing options"));
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
        assert!(events[3]
            .tool_result
            .as_deref()
            .unwrap()
            .contains("file1.txt"));
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

    // A real session puts `usage` on the message and spells every counter
    // `*_tokens`. The fixture above uses short names on the line, which the
    // parser accepted while every real session yielded `None` for all four.
    #[test]
    fn parse_reads_real_usage_field_names() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-u\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\",\"usage\":{\"input_tokens\":4,\"output_tokens\":210,\"cache_read_input_tokens\":20000,\"cache_creation_input_tokens\":12345}},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events[0].input_tokens, Some(4));
        assert_eq!(events[0].output_tokens, Some(210));
        assert_eq!(events[0].cache_read_tokens, Some(20000));
        assert_eq!(events[0].cache_write_tokens, Some(12345));
    }

    // A tool-only assistant record produces no message event, so its usage
    // vanished with it — in a real session that is half the records and almost
    // all the tokens. It now rides on the first call the record made.
    #[test]
    fn parse_keeps_the_usage_of_a_tool_only_record() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-to\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}],\"usage\":{\"input_tokens\":4,\"output_tokens\":90,\"cache_read_input_tokens\":20000,\"cache_creation_input_tokens\":12345}},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let call = events
            .iter()
            .find(|e| e.event_type == "tool-call")
            .expect("tool-call event");
        assert_eq!(call.input_tokens, Some(4));
        assert_eq!(call.output_tokens, Some(90));
        assert_eq!(call.cache_read_tokens, Some(20000));
        assert_eq!(call.cache_write_tokens, Some(12345));
        // Nothing is invented: the record still yields no message row.
        assert_eq!(events.len(), 2);
    }

    // A turn is one user message plus every request/response round-trip it
    // took. Every assistant record used to be pinned to step 1, so a long turn
    // rendered as a single "Step 1" group and the numbering meant nothing.
    #[test]
    fn parse_numbers_the_steps_of_a_turn() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-step\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}]},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"ok\"}]},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Bash\",\"input\":{\"command\":\"pwd\"}}]},\"timestamp\":\"2026-03-06T10:04:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"timestamp\":\"2026-03-06T10:05:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let step_of = |wanted: &str| {
            events
                .iter()
                .find(|e| e.tool_call_id.as_deref() == Some(wanted))
                .unwrap_or_else(|| panic!("no event for {wanted}"))
                .step
        };

        assert_eq!(events[0].step, Some(0), "the user message opens the turn");
        assert_eq!(step_of("t1"), Some(1));
        assert_eq!(step_of("t2"), Some(2));
        let last = events.last().unwrap();
        assert_eq!(last.content.as_deref(), Some("done"));
        assert_eq!(last.step, Some(3));
        assert!(events.iter().all(|e| e.turn == Some(1)), "still one turn");
    }

    // The tool result answers the step whose call produced it, not a fixed 1.
    #[test]
    fn parse_keeps_a_tool_result_in_its_calls_step() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-step2\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"a\",\"name\":\"Read\",\"input\":{\"file_path\":\"/x\"}}]},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"a\",\"content\":\"1\"}]},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"b\",\"name\":\"Read\",\"input\":{\"file_path\":\"/y\"}}]},\"timestamp\":\"2026-03-06T10:04:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"b\",\"content\":\"2\"}]},\"timestamp\":\"2026-03-06T10:05:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let result_step = |wanted: &str| {
            events
                .iter()
                .find(|e| {
                    e.event_type == "tool-result" && e.tool_call_id.as_deref() == Some(wanted)
                })
                .unwrap_or_else(|| panic!("no result for {wanted}"))
                .step
        };
        assert_eq!(result_step("a"), Some(1));
        assert_eq!(result_step("b"), Some(2));
    }

    // A compaction is the moment the context was thrown away. The frontend has
    // rendered a `compaction` record all along; the parser never emitted one.
    #[test]
    fn parse_marks_a_compaction_and_ignores_other_system_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-c\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"system\",\"subtype\":\"turn_duration\",\"durationMs\":443308,\"messageCount\":141,\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"content\":\"Context compacted\",\"compactMetadata\":{\"trigger\":\"manual\",\"preTokens\":763},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"after\"},\"timestamp\":\"2026-03-06T10:04:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let compactions: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "compaction")
            .collect();
        assert_eq!(
            compactions.len(),
            1,
            "one compaction, no other system lines"
        );
        assert_eq!(compactions[0].content.as_deref(), Some("Context compacted"));
        assert_eq!(compactions[0].turn, Some(1));
        // Nothing else was invented from the turn_duration line.
        assert_eq!(events.len(), 3);
    }

    // The context restored by a compaction is replayed as a `user` line. Taking
    // it for user input opened a phantom turn and showed the restored context
    // as something the user had said.
    #[test]
    fn parse_does_not_mistake_replayed_context_for_user_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-cc\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"user\",\"isCompactSummary\":true,\"message\":{\"role\":\"user\",\"content\":\"This session is being continued from a previous conversation\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"still the same turn\"},\"timestamp\":\"2026-03-06T10:03:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        assert_eq!(events.len(), 2, "the replayed context yields no record");
        assert!(
            !events.iter().any(|e| e
                .content
                .as_deref()
                .unwrap_or("")
                .contains("being continued")),
            "the restored context must not surface as a user message"
        );
        assert_eq!(events[1].turn, Some(1), "no phantom turn was opened");
        assert_eq!(events[1].step, Some(1));
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
            assert!(
                events[i].ts >= events[i - 1].ts,
                "events should be chronological"
            );
        }
        // Two turns
        assert_eq!(events[0].turn, Some(1));
        assert_eq!(events[2].turn, Some(2));
    }

    #[test]
    fn parse_empty_session_returns_no_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"sessionId\":\"sess-empty\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-empty");
        assert!(events.is_empty());
    }

    // Session logs are appended incrementally, so a crashed run leaves a
    // truncated last line. That must not sink the whole session.
    // A task notification arrives as a `user` line. Counting it as user input
    // gave every background subagent's result a phantom turn of its own,
    // splitting one assistant turn into several.
    #[test]
    fn parse_task_notification_does_not_start_a_turn() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-n\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Agent\",\"input\":{\"description\":\"x\"}}]},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<task-notification>\\n<task-id>a</task-id>\\n<status>completed</status>\\n<result>done</result>\\n</task-notification>\"},\"timestamp\":\"2026-03-06T10:05:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"wrapped up\"},\"timestamp\":\"2026-03-06T10:06:00Z\"}\n",
            ),
        )
        .unwrap();

        let (_, events) = parse_trajectory(&path).unwrap();
        let notification = events
            .iter()
            .find(|e| {
                e.content
                    .as_deref()
                    .unwrap_or("")
                    .contains("<task-notification>")
            })
            .expect("notification event");
        // The notification stays inside turn 1 — it opened no turn of its own.
        assert_eq!(notification.turn, Some(1));
        assert_eq!(notification.event_type, "user-message");

        let after = events
            .iter()
            .find(|e| e.content.as_deref() == Some("wrapped up"))
            .expect("trailing assistant event");
        assert_eq!(after.turn, Some(1));

        // And the real user message still opens turn 1.
        assert_eq!(events[0].turn, Some(1));
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"sess-bad\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assist\n", // truncated
                "not json at all\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hi!\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .unwrap();

        let (sid, events) = parse_trajectory(&path).unwrap();
        assert_eq!(sid, "sess-bad");
        // The two valid messages survive; the two bad lines are dropped.
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[1].event_type, "assistant-message");
        assert_eq!(events[1].content.as_deref(), Some("Hi!"));
    }

    #[test]
    fn parse_survives_a_file_of_only_garbage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "garbage\nmore garbage\n").unwrap();

        let (_sid, events) = parse_trajectory(&path).unwrap();
        assert!(events.is_empty());
    }
}
