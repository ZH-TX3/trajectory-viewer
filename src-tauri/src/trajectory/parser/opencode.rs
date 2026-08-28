// ── OpenCode Session Parser ──────────────────────────────────────────────
//
// Parses OpenCode session data into structured TrajectoryEvent lists.
//
// OpenCode stores sessions under `~/.local/share/opencode/` (or
// `$XDG_DATA_HOME/opencode`), in one of two layouts:
//
//   JSON (legacy flat files):
//     storage/session/{project}/{ses_xxx}.json   session metadata
//     storage/message/{ses_xxx}/msg_xxx.json     one message per file
//     storage/part/{msg_xxx}/prt_xxx.json        one content part per file
//
//   SQLite (newer primary store, opencode.db):
//     session(id, title, directory, time_created, time_updated)
//     message(id, session_id, time_created, time_updated, data)
//     part(id, message_id, session_id, time_created, time_updated, data)
//     `data` holds the same JSON shape as the flat files.
//
// Message JSON carries role / time / modelID / providerID / tokens; the
// actual content lives in parts:
//   - `type:"text"`      — user/assistant text
//   - `type:"reasoning"` — assistant chain-of-thought
//   - `type:"tool"`      — tool invocation + result in one part
//                         (state.input / state.output / state.status)
//   - step-start / step-finish — step boundaries (mostly ignored here)

use std::collections::HashMap;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};
use crate::trajectory::utils::finalize_trajectory_timings;
use crate::trajectory::{ContentBlock, TrajectoryEvent};

/// Return the OpenCode base directory (`$XDG_DATA_HOME/opencode`).
pub fn get_opencode_base_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            let dir = PathBuf::from(xdg).join("opencode");
            if dir.is_dir() {
                return Some(dir);
            }
        }
    }
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|home| PathBuf::from(home).join(".local/share/opencode"))
        .filter(|dir| dir.is_dir())
}

/// One content part of an OpenCode message.
struct PartRecord {
    part_type: String,
    text: Option<String>,
    tool_name: Option<String>,
    call_id: Option<String>,
    /// Parsed `state` object for tool parts (input / output / status).
    tool_state: Option<Value>,
    time_start: Option<i64>,
    time_end: Option<i64>,
}

/// One OpenCode message with all of its parts.
struct MessageRecord {
    id: String,
    role: String,
    created: i64,
    model: Option<String>,
    provider: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    parts: Vec<PartRecord>,
}

// ── Source reference helpers ─────────────────────────────────────────────

const DB_SOURCE_PREFIX: &str = "sqlite:";

/// Parse a SQLite source reference `sqlite:<db_path>:<session_id>`.
///
/// Splits via `rfind(":ses_")` so the Windows db path's own colons survive.
fn parse_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
    let rest = source.strip_prefix(DB_SOURCE_PREFIX)?;
    let sep = rest.rfind(":ses_")?;
    let db_path = PathBuf::from(&rest[..sep]);
    let session_id = rest[sep + 1..].to_string();
    Some((db_path, session_id))
}

/// `true` when `source_path` references the SQLite database rather than a
/// flat session file.
pub fn is_sqlite_source(source: &str) -> bool {
    source.starts_with(DB_SOURCE_PREFIX)
}

// ── Trajectory parsing ───────────────────────────────────────────────────

/// Parse an OpenCode session into trajectory events.
///
/// `path` is either a `sqlite:<db>:<session_id>` reference or a session JSON
/// file under `storage/session/`.
pub fn parse_trajectory(path: &Path) -> Result<(String, Vec<TrajectoryEvent>), String> {
    let source = path.to_string_lossy();
    if let Some((db, session_id)) = parse_sqlite_source(&source) {
        let messages = load_messages_from_db(&db, &session_id)?;
        build_events(&session_id, messages)
    } else if path.is_dir() {
        // Message directory for this session (legacy cc-switch layout).
        load_messages_from_json_dir(path).and_then(|messages| {
            let sid = session_id_from_dir(path)
                .unwrap_or_else(|| "opencode-session".to_string());
            build_events(&sid, messages)
        })
    } else {
        let session_id = read_session_id(path)?;
        let storage = find_storage_root(path)
            .ok_or_else(|| format!("Cannot locate OpenCode storage for {}", path.display()))?;
        let msg_dir = storage.join("message").join(&session_id);
        let messages = load_messages_from_json_dir(&msg_dir)?;
        build_events(&session_id, messages)
    }
}

/// Load messages (message + parts) from a message directory.
fn load_messages_from_json_dir(msg_dir: &Path) -> Result<Vec<MessageRecord>, String> {
    if !msg_dir.is_dir() {
        return Err(format!(
            "OpenCode message directory not found: {}",
            msg_dir.display()
        ));
    }

    // `{storage}/message/{session_id}` → storage is two levels up.
    let storage = msg_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| "Cannot determine OpenCode storage root".to_string())?;

    let mut files = Vec::new();
    collect_json_files(msg_dir, &mut files);

    let mut messages = Vec::new();
    for path in &files {
        let value = read_json_file(path)?;
        if let Some(mut msg) = parse_message_json(&value, None, None) {
            // Attach this message's parts from storage/part/{message_id}/.
            let part_dir = storage.join("part").join(&msg.id);
            let mut part_files = Vec::new();
            collect_json_files(&part_dir, &mut part_files);
            for part_path in &part_files {
                if let Ok(part_value) = read_json_file(part_path) {
                    if let Some(part) = parse_part_json(&part_value, None) {
                        msg.parts.push(part);
                    }
                }
            }
            messages.push(msg);
        }
    }

    messages.sort_by_key(|m| m.created);
    Ok(messages)
}

/// Load messages from the SQLite database for one session.
fn load_messages_from_db(db_path: &Path, session_id: &str) -> Result<Vec<MessageRecord>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open OpenCode database: {e}"))?;

    let mut part_stmt = conn
        .prepare(
            "SELECT message_id, time_created, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC",
        )
        .map_err(|e| format!("Failed to prepare part query: {e}"))?;

    let mut parts_map: HashMap<String, Vec<PartRecord>> = HashMap::new();
    let part_rows = part_stmt
        .query_map([session_id], |row| {
            let message_id: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            let data: String = row.get(2)?;
            Ok((message_id, ts, data))
        })
        .map_err(|e| format!("Failed to query parts: {e}"))?;
    for row in part_rows.flatten() {
        let (message_id, ts, data) = row;
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if let Some(part) = parse_part_json(&value, Some(ts)) {
                parts_map.entry(message_id).or_default().push(part);
            }
        }
    }

    let mut msg_stmt = conn
        .prepare(
            "SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC",
        )
        .map_err(|e| format!("Failed to prepare message query: {e}"))?;

    let mut messages = Vec::new();
    let msg_rows = msg_stmt
        .query_map([session_id], |row| {
            let id: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            let data: String = row.get(2)?;
            Ok((id, ts, data))
        })
        .map_err(|e| format!("Failed to query messages: {e}"))?;
    for row in msg_rows.flatten() {
        let (id, ts, data) = row;
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if let Some(mut msg) = parse_message_json(&value, Some(ts), Some(&id)) {
                let mut parts = parts_map.remove(&id).unwrap_or_default();
                // Sort parts: parts carrying an explicit timestamp first
                // (reasoning/tool carry one), then by table timestamp.
                parts.sort_by_key(|p| (p.time_start.unwrap_or(0), 0usize));
                msg.parts = parts;
                messages.push(msg);
            }
        }
    }

    Ok(messages)
}

/// Parse one message JSON object into a MessageRecord (parts filled later).
///
/// The SQLite layout stores `{role, ...}` (no `id`) in `data`, so callers can
/// pass the row's own id via `id_override`.
fn parse_message_json(
    value: &Value,
    db_ts: Option<i64>,
    id_override: Option<&str>,
) -> Option<MessageRecord> {
    let id = id_override
        .map(|s| s.to_string())
        .or_else(|| value.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))?;
    let role = value
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let created = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .or(db_ts)
        .unwrap_or(0);

    // Model + provider: top-level modelID/providerID, or a nested model object.
    let model = value
        .get("modelID")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("model").and_then(|m| m.get("modelID")).and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let provider = value
        .get("providerID")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("model").and_then(|m| m.get("providerID")).and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    let tokens = value.get("tokens");
    let input_tokens = tokens.and_then(|t| t.get("input")).and_then(|v| v.as_i64());
    let output_tokens = tokens.and_then(|t| t.get("output")).and_then(|v| v.as_i64());
    let reasoning_tokens = tokens
        .and_then(|t| t.get("reasoning"))
        .and_then(|v| v.as_i64());
    let cache_read_tokens = tokens
        .and_then(|t| t.get("cache"))
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_i64());
    let cache_write_tokens = tokens
        .and_then(|t| t.get("cache"))
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_i64());

    Some(MessageRecord {
        id,
        role,
        created,
        model,
        provider,
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        parts: Vec::new(),
    })
}

/// Parse one part JSON object into a PartRecord.
fn parse_part_json(value: &Value, db_ts: Option<i64>) -> Option<PartRecord> {
    let part_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let (time_start, time_end) = match part_type.as_str() {
        "text" | "reasoning" => (
            value
                .get("time")
                .and_then(|t| t.get("start"))
                .and_then(|v| v.as_i64()),
            value
                .get("time")
                .and_then(|t| t.get("end"))
                .and_then(|v| v.as_i64()),
        ),
        "tool" => (
            value
                .get("state")
                .and_then(|s| s.get("time"))
                .and_then(|t| t.get("start"))
                .and_then(|v| v.as_i64()),
            value
                .get("state")
                .and_then(|s| s.get("time"))
                .and_then(|t| t.get("end"))
                .and_then(|v| v.as_i64()),
        ),
        _ => (None, None),
    };

    Some(PartRecord {
        part_type,
        text: value.get("text").and_then(|v| v.as_str()).map(|s| s.to_string()),
        tool_name: value.get("tool").and_then(|v| v.as_str()).map(|s| s.to_string()),
        call_id: value.get("callID").and_then(|v| v.as_str()).map(|s| s.to_string()),
        tool_state: value.get("state").cloned().filter(|s| s.is_object()),
        time_start: time_start.or(db_ts),
        time_end,
    })
}

/// Reconstruct turn/step counters and emit TrajectoryEvents from messages.
fn build_events(
    session_id: &str,
    messages: Vec<MessageRecord>,
) -> Result<(String, Vec<TrajectoryEvent>), String> {
    let mut events: Vec<TrajectoryEvent> = Vec::new();
    let mut seq = 0usize;
    let mut tool_call_tracker: HashMap<String, (String, String)> = HashMap::new();
    let mut current_turn = 0usize;
    let mut current_step = 0usize;

    for mut msg in messages {
        // Stable part order: parts carrying an explicit start time go first.
        msg.parts.sort_by_key(|p| p.time_start.unwrap_or(0));

        match msg.role.as_str() {
            "user" => push_user_events(
                &mut events,
                &mut seq,
                &msg,
                &msg.parts,
                &mut current_turn,
                &mut current_step,
            ),
            "assistant" => push_assistant_events(
                &mut events,
                &mut seq,
                &msg,
                &msg.parts,
                &mut tool_call_tracker,
                &mut current_turn,
                &mut current_step,
            ),
            _ => {}
        }
    }

    events.sort_by_key(|e| e.seq);
    finalize_trajectory_timings(&mut events);

    Ok((session_id.to_string(), events))
}

/// Emit user-message (and any legacy tool-result) events for a user message.
fn push_user_events(
    events: &mut Vec<TrajectoryEvent>,
    seq: &mut usize,
    msg: &MessageRecord,
    parts: &[PartRecord],
    current_turn: &mut usize,
    current_step: &mut usize,
) {
    let text_parts: Vec<String> = parts
        .iter()
        .filter(|p| p.part_type == "text")
        .filter_map(|p| p.text.clone().filter(|t| !t.trim().is_empty()))
        .collect();
    let tool_parts: Vec<&PartRecord> = parts
        .iter()
        .filter(|p| p.part_type == "tool")
        .collect();

    if text_parts.is_empty() && tool_parts.is_empty() {
        return; // service / summary-only message
    }

    // A real user text message starts a new turn. Tool-result-only messages
    // (legacy layouts) continue the current assistant turn.
    if !text_parts.is_empty() {
        *current_turn += 1;
        *current_step = 0;
    }

    // Legacy layout: tool results may live on user messages.
    for part in tool_parts {
        *seq += 1;
        let state = part.tool_state.as_ref();
        let call_id = part.call_id.clone();
        let tool_name = part.tool_name.clone();
        let args = state
            .and_then(|s| s.get("input"))
            .map(|i| serde_json::to_string(i).unwrap_or_default());

        events.push(TrajectoryEvent {
            seq: *seq,
            ts: part.time_start.or(Some(msg.created)).unwrap_or(0),
            event_type: "tool-result".to_string(),
            role: Some("user".to_string()),
            content: None,
            content_blocks: None,
            tool_call_id: call_id,
            tool_name,
            tool_args: args,
            tool_result: state.and_then(|s| s.get("output")).and_then(|v| v.as_str()).map(|s| s.to_string()),
            is_error: tool_status_is_error(state),
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
            provider: Some("opencode".to_string()),
        });
    }

    if !text_parts.is_empty() {
        *seq += 1;
        let content = text_parts.join("\n");
        events.push(TrajectoryEvent {
            seq: *seq,
            ts: msg.created,
            event_type: "user-message".to_string(),
            role: Some("user".to_string()),
            content: Some(content.clone()),
            content_blocks: Some(
                text_parts
                    .into_iter()
                    .map(|t| ContentBlock {
                        block_type: "text".to_string(),
                        text: Some(t),
                        tool_call_id: None,
                        tool_name: None,
                        tool_args: None,
                        image_src: None,
                    })
                    .collect(),
            ),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            is_error: None,
            turn: Some(*current_turn),
            step: Some(0),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: msg.input_tokens,
            output_tokens: msg.output_tokens,
            reasoning_tokens: msg.reasoning_tokens,
            cache_read_tokens: msg.cache_read_tokens,
            cache_write_tokens: msg.cache_write_tokens,
            model: msg.model.clone(),
            provider: Some("opencode".to_string()),
        });
    }
}

/// Emit tool-call / tool-result / assistant-message events for an assistant
/// message. OpenCode stores a tool call AND its result in one part.
fn push_assistant_events(
    events: &mut Vec<TrajectoryEvent>,
    seq: &mut usize,
    msg: &MessageRecord,
    parts: &[PartRecord],
    tool_call_tracker: &mut HashMap<String, (String, String)>,
    current_turn: &mut usize,
    current_step: &mut usize,
) {
    if *current_turn > 0 {
        *current_step += 1;
    }

    let mut text_parts: Vec<String> = Vec::new();
    let mut reasoning_blocks: Vec<ContentBlock> = Vec::new();

    // Emit one tool-call + tool-result pair per tool part.
    for part in parts {
        match part.part_type.as_str() {
            "text" => {
                if let Some(text) = part.text.clone().filter(|t| !t.trim().is_empty()) {
                    text_parts.push(text);
                }
            }
            "reasoning" => {
                if let Some(text) = part.text.clone().filter(|t| !t.trim().is_empty()) {
                    reasoning_blocks.push(ContentBlock {
                        block_type: "reasoning".to_string(),
                        text: Some(text),
                        tool_call_id: None,
                        tool_name: None,
                        tool_args: None,
                        image_src: None,
                    });
                }
            }
            "tool" => {
                let state = part.tool_state.as_ref();
                let call_id = part.call_id.clone();
                let tool_name = part.tool_name.clone();
                let args = state
                    .and_then(|s| s.get("input"))
                    .map(|i| serde_json::to_string(i).unwrap_or_default());

                if let (Some(id), Some(name)) = (&call_id, &tool_name) {
                    tool_call_tracker.insert(
                        id.clone(),
                        (name.clone(), args.clone().unwrap_or_default()),
                    );
                }

                // Tool-call event (request, at state start).
                *seq += 1;
                events.push(TrajectoryEvent {
                    seq: *seq,
                    ts: part.time_start.or(Some(msg.created)).unwrap_or_default(),
                    event_type: "tool-call".to_string(),
                    role: Some("assistant".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    tool_args: args.clone(),
                    tool_result: None,
                    is_error: None,
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
                    provider: Some("opencode".to_string()),
                });

                // Tool-result event (response, at state end).
                *seq += 1;
                let result = state
                    .and_then(|s| s.get("output"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                events.push(TrajectoryEvent {
                    seq: *seq,
                    ts: part.time_end.or(part.time_start).or(Some(msg.created)).unwrap_or_default(),
                    event_type: "tool-result".to_string(),
                    role: Some("tool".to_string()),
                    content: None,
                    content_blocks: None,
                    tool_call_id: call_id,
                    tool_name,
                    tool_args: args,
                    tool_result: result,
                    is_error: tool_status_is_error(state),
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
                    provider: Some("opencode".to_string()),
                });
            }
            _ => {
                // step-start / step-finish / others: ignored
            }
        }
    }

    if !text_parts.is_empty() || !reasoning_blocks.is_empty() {
        *seq += 1;
        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };
        let mut blocks = text_parts
            .into_iter()
            .map(|t| ContentBlock {
                block_type: "text".to_string(),
                text: Some(t),
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                image_src: None,
            })
            .collect::<Vec<_>>();
        blocks.extend(reasoning_blocks);

        events.push(TrajectoryEvent {
            seq: *seq,
            ts: msg.created,
            event_type: "assistant-message".to_string(),
            role: Some("assistant".to_string()),
            content,
            content_blocks: Some(blocks),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            tool_result: None,
            is_error: None,
            turn: Some(*current_turn),
            step: Some(*current_step),
            duration_ms: None,
            ttft_ms: None,
            input_tokens: msg.input_tokens,
            output_tokens: msg.output_tokens,
            reasoning_tokens: msg.reasoning_tokens,
            cache_read_tokens: msg.cache_read_tokens,
            cache_write_tokens: msg.cache_write_tokens,
            model: msg.model.clone(),
            provider: msg.provider.clone(),
        });
    }
}

/// Map an OpenCode tool state.status to the error flag.
fn tool_status_is_error(state: Option<&Value>) -> Option<bool> {
    state
        .and_then(|s| s.get("status"))
        .and_then(|v| v.as_str())
        .map(|status| status != "completed")
}

// ── Session scanning ─────────────────────────────────────────────────────

/// Scan OpenCode sessions from both the legacy JSON files and the SQLite
/// database, merging results (SQLite takes precedence on ID conflicts).
pub fn scan_sessions() -> Vec<SessionMeta> {
    let base = match get_opencode_base_dir() {
        Some(dir) => dir,
        None => return Vec::new(),
    };

    let json_sessions = scan_sessions_json(&base);
    let sqlite_sessions = scan_sessions_sqlite(&base);

    if sqlite_sessions.is_empty() {
        return json_sessions;
    }
    if json_sessions.is_empty() {
        return sqlite_sessions;
    }

    let sqlite_ids: std::collections::HashSet<String> = sqlite_sessions
        .iter()
        .map(|s| s.session_id.clone())
        .collect();

    let mut merged = sqlite_sessions;
    for session in json_sessions {
        if !sqlite_ids.contains(&session.session_id) {
            merged.push(session);
        }
    }
    merged
}

fn scan_sessions_json(base: &Path) -> Vec<SessionMeta> {
    let storage = base.join("storage");
    let session_dir = storage.join("session");
    if !session_dir.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_json_files(&session_dir, &mut files);

    let mut sessions = Vec::new();
    for path in files {
        if let Some(meta) = parse_session_meta(&storage, &path) {
            sessions.push(meta);
        }
    }
    sessions
}

fn scan_sessions_sqlite(base: &Path) -> Vec<SessionMeta> {
    let db_path = base.join("opencode.db");
    if !db_path.exists() {
        return Vec::new();
    }

    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let stmt = match conn.prepare(
        "SELECT id, title, directory, time_created, time_updated FROM session ORDER BY time_updated DESC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut stmt = stmt;

    let db_display = db_path.display().to_string();

    let iter = match stmt.query_map([], |row| {
        let session_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let directory: String = row.get(2)?;
        let created: i64 = row.get(3)?;
        let updated: i64 = row.get(4)?;
        Ok((session_id, title, directory, created, updated))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    iter.flatten()
        .map(|(session_id, title, directory, created, updated)| {
            let display_title = if title.is_empty() {
                path_basename(&directory)
            } else {
                Some(title)
            };
            let project_group = project_group_from_dir(&directory);
            let project_dir = if directory.is_empty() { None } else { Some(directory) };
            SessionMeta {
                provider_id: "opencode".to_string(),
                session_id: session_id.clone(),
                title: display_title.clone(),
                summary: display_title,
                project_dir,
                project_group,
                created_at: Some(created),
                last_active_at: Some(updated),
                source_path: Some(format!("sqlite:{db_display}:{session_id}")),
                resume_command: Some(format!("opencode session resume {session_id}")),
            }
        })
        .collect()
}

/// Parse a `storage/session/.../ses_xxx.json` file into a SessionMeta.
///
/// `source_path` points at the session JSON file itself (mtime-pollable),
/// unlike the upstream cc-switch which uses the message directory.
fn parse_session_meta(storage: &Path, path: &Path) -> Option<SessionMeta> {
    let value = read_json_file(path).ok()?;

    let session_id = value.get("id").and_then(|v| v.as_str())?.to_string();
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let directory = value
        .get("directory")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let created_at = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64());
    let updated_at = value
        .get("time")
        .and_then(|t| t.get("updated"))
        .and_then(|v| v.as_i64());

    let display_title = title.or_else(|| {
        directory.as_deref().and_then(path_basename).map(|s| s.to_string())
    });
    let summary = if display_title.is_some() {
        // OpenCode has no per-message /summary in the session store; the
        // title is a good enough list summary.
        None
    } else {
        get_first_user_summary(storage, &session_id)
    };

    Some(SessionMeta {
        provider_id: "opencode".to_string(),
        session_id: session_id.clone(),
        title: display_title,
        summary,
        project_dir: directory.clone(),
        project_group: project_group_from_dir(&directory.as_deref().unwrap_or_default()),
        created_at,
        last_active_at: updated_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("opencode session resume {session_id}")),
    })
}

/// Derive a project group label from the session's working directory.
fn project_group_from_dir(directory: &str) -> Option<String> {
    let normalized = directory.trim_end_matches(['/', '\\']);
    if normalized.is_empty() {
        return None;
    }
    normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Summarize the first user message's first text part (fallback summary).
fn get_first_user_summary(storage: &Path, session_id: &str) -> Option<String> {
    let msg_dir = storage.join("message").join(session_id);
    if !msg_dir.is_dir() {
        return None;
    }

    let mut files = Vec::new();
    collect_json_files(&msg_dir, &mut files);

    let mut user_msgs: Vec<(i64, String)> = Vec::new();
    for path in &files {
        let value = read_json_file(path).ok()?;
        if value.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let id = value.get("id").and_then(|v| v.as_str())?.to_string();
        let ts = value
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        user_msgs.push((ts, id));
    }

    user_msgs.sort_by_key(|(ts, _)| *ts);
    let (_, first_id) = user_msgs.first()?;

    let part_dir = storage.join("part").join(first_id);
    let mut part_files = Vec::new();
    collect_json_files(&part_dir, &mut part_files);

    for path in part_files {
        let value = read_json_file(&path).ok()?;
        if value.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(truncate(trimmed, 160));
                }
            }
        }
    }
    None
}

// ── Messages view ────────────────────────────────────────────────────────

/// Load flat `SessionMessage`s for the Messages tab. Dispatches on the
/// source reference (SQLite vs. session JSON file vs. message dir).
pub fn load_messages(source: &str) -> Result<Vec<SessionMessage>, String> {
    if let Some((db, session_id)) = parse_sqlite_source(source) {
        return load_messages_from_db_as_session(&db, &session_id);
    }

    let path = Path::new(source);
    let msg_dir = if path.is_dir() {
        // legacy source: message directory
        path.to_path_buf()
    } else {
        let session_id = read_session_id(path)?;
        let storage = find_storage_root(path).ok_or_else(|| {
            format!("Cannot locate OpenCode storage for {}", path.display())
        })?;
        storage.join("message").join(&session_id)
    };

    let messages = load_messages_from_json_dir(&msg_dir)?;
    let entries = messages
        .into_iter()
        .map(|m| {
            let content = collect_message_text(&m);
            SessionMessage {
                role: m.role,
                content,
                ts: if m.created > 0 { Some(m.created) } else { None },
            }
        })
        .filter(|m| !m.content.trim().is_empty())
        .collect();
    Ok(entries)
}

fn load_messages_from_db_as_session(
    db: &Path,
    session_id: &str,
) -> Result<Vec<SessionMessage>, String> {
    let messages = load_messages_from_db(db, session_id)?;
    Ok(messages
        .into_iter()
        .map(|m| {
            let content = collect_message_text(&m);
            SessionMessage {
                role: m.role,
                content,
                ts: Some(m.created),
            }
        })
        .filter(|m| !m.content.trim().is_empty())
        .collect())
}

/// Text for the messages view: text parts plus `[Tool: name]` markers.
fn collect_message_text(msg: &MessageRecord) -> String {
    let mut pieces = Vec::new();
    for part in &msg.parts {
        match part.part_type.as_str() {
            "text" => {
                if let Some(text) = part.text.as_deref().filter(|t| !t.trim().is_empty()) {
                    pieces.push(text.to_string());
                }
            }
            "tool" => {
                let tool = part.tool_name.as_deref().unwrap_or("unknown");
                pieces.push(format!("[Tool: {tool}]"));
            }
            _ => {}
        }
    }
    pieces.join("\n")
}

// ── Low-level helpers ────────────────────────────────────────────────────

// ── Deletion support ─────────────────────────────────────────────────────

/// `true` when `source` refers to OpenCode session data we manage (either a
/// `sqlite:` reference or a session JSON file inside `storage/session/`).
pub fn is_opencode_source(source: &str) -> bool {
    if is_sqlite_source(source) {
        return true;
    }
    let path = Path::new(source);
    path.is_file()
        && path.extension().and_then(|e| e.to_str()) == Some("json")
        && opencode_storage_of(path).is_some()
}

/// `true` when `dir` is a directory inside OpenCode storage (for bulk
/// deletes of a project's session files).
pub fn is_session_store_dir(dir: &Path) -> bool {
    dir.is_dir() && opencode_storage_of(dir).is_some()
}

/// Locate the OpenCode `storage` root that contains `path` (walking up).
fn opencode_storage_of(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() { Some(path.to_path_buf()) } else { path.parent().map(Path::to_path_buf) };
    while let Some(candidate) = dir {
        if candidate.join("message").is_dir()
            && candidate.join("part").is_dir()
            && candidate.join("session").is_dir()
        {
            return Some(candidate);
        }
        dir = candidate.parent().map(Path::to_path_buf);
    }
    None
}

/// Delete one OpenCode session (JSON file layout or SQLite row set).
pub fn delete_session(source: &str) -> Result<(), String> {
    if let Some((db, session_id)) = parse_sqlite_source(source) {
        return delete_session_sqlite(&db, &session_id);
    }
    delete_session_json(Path::new(source))
}

/// Deletion for the legacy JSON layout: session file + message dir + parts.
fn delete_session_json(path: &Path) -> Result<(), String> {
    let session_id = read_session_id(path)?;
    let storage = opencode_storage_of(path)
        .ok_or_else(|| format!("Cannot locate OpenCode storage for {}", path.display()))?;

    let base = get_opencode_base_dir()
        .ok_or_else(|| "OpenCode directory not found".to_string())?;
    if !path.starts_with(&base.join("storage")) {
        return Err("Refusing to delete outside OpenCode storage".to_string());
    }

    // Remove the part directories for every message in this session.
    let msg_dir = storage.join("message").join(&session_id);
    let mut msg_files = Vec::new();
    collect_json_files(&msg_dir, &mut msg_files);
    for file in &msg_files {
        if let Ok(value) = read_json_file(file) {
            if let Some(message_id) = value.get("id").and_then(|v| v.as_str()) {
                let part_dir = storage.join("part").join(message_id);
                if part_dir.is_dir() {
                    std::fs::remove_dir_all(&part_dir)
                        .map_err(|e| format!("Failed to delete part dir: {e}"))?;
                }
            }
        }
    }

    // session_diff summary + the message directory itself.
    let diff = storage.join("session_diff").join(format!("{session_id}.json"));
    if diff.exists() {
        std::fs::remove_file(&diff).map_err(|e| format!("Failed to delete session diff: {e}"))?;
    }
    if msg_dir.is_dir() {
        std::fs::remove_dir_all(&msg_dir)
            .map_err(|e| format!("Failed to delete message dir: {e}"))?;
    }

    std::fs::remove_file(path).map_err(|e| format!("Failed to delete session file: {e}"))?;
    Ok(())
}

/// Deletion from the OpenCode SQLite database, guarded to the canonical
/// `opencode.db` so a foreign db can never be touched.
fn delete_session_sqlite(db_path: &Path, session_id: &str) -> Result<(), String> {
    let expected = get_opencode_base_dir()
        .ok_or_else(|| "OpenCode directory not found".to_string())?
        .join("opencode.db");

    let actual = db_path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve db path: {e}"))?;
    let wanted = expected
        .canonicalize()
        .map_err(|e| format!("Failed to resolve expected db path: {e}"))?;
    if actual != wanted {
        return Err("SQLite path does not match expected OpenCode database".to_string());
    }

    let conn = rusqlite::Connection::open(&actual)
        .map_err(|e| format!("Failed to open OpenCode database: {e}"))?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {e}"))?;

    tx.execute("DELETE FROM part WHERE session_id = ?1", [session_id])
        .map_err(|e| format!("Failed to delete parts: {e}"))?;
    tx.execute("DELETE FROM message WHERE session_id = ?1", [session_id])
        .map_err(|e| format!("Failed to delete messages: {e}"))?;
    let deleted = tx
        .execute("DELETE FROM session WHERE id = ?1", [session_id])
        .map_err(|e| format!("Failed to delete session: {e}"))?;

    tx.commit()
        .map_err(|e| format!("Failed to commit deletion: {e}"))?;
    if deleted == 0 {
        return Err("Session not found in database".to_string());
    }
    Ok(())
}

/// Delete every OpenCode session whose files live under `dir` (a
/// `storage/session/{project}` directory), returning how many were removed.
pub fn delete_sessions_in_dir(dir: &Path) -> Result<usize, String> {
    opencode_storage_of(dir)
        .ok_or_else(|| format!("Cannot locate OpenCode storage for {}", dir.display()))?;
    let base = get_opencode_base_dir()
        .ok_or_else(|| "OpenCode directory not found".to_string())?;
    if !dir.starts_with(&base.join("storage")) {
        return Err("Refusing to delete outside OpenCode storage".to_string());
    }

    let mut files = Vec::new();
    collect_json_files(dir, &mut files);

    let mut deleted = 0;
    for file in &files {
        if delete_session_json(file).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);
    let mut text = String::new();
    for line in reader.lines() {
        text.push_str(&line.map_err(|e| format!("Read error: {e}"))?);
    }
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

fn read_session_id(path: &Path) -> Result<String, String> {
    let value = read_json_file(path)?;
    value
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("No session id in {}", path.display()))
}

/// Walk up from a session JSON file to the storage root, i.e. the directory
/// that contains `session/`, `message/` and `part/`.
fn find_storage_root(session_path: &Path) -> Option<PathBuf> {
    let mut dir = session_path.parent();
    while let Some(candidate) = dir {
        if candidate.join("message").is_dir() && candidate.join("part").is_dir() {
            return Some(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

fn session_id_from_dir(msg_dir: &Path) -> Option<String> {
    msg_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .filter(|s| s.starts_with("ses_"))
}

fn path_basename(path: &str) -> Option<String> {
    let normalized = path.trim_end_matches(['/', '\\']);
    if normalized.is_empty() {
        return None;
    }
    normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.replace('\\', "/"))
}

fn truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut result: String = trimmed.chars().take(max_chars).collect();
    result.push_str("...");
    result
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn opencode_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Build the legacy JSON layout under `storage/`.
    fn build_json_layout(storage: &Path) {
        write_file(
            &storage.join("session/project-1/ses_1.json"),
            r#"{"id":"ses_1","title":"Session Title","directory":"/tmp/proj","time":{"created":1000,"updated":3000}}"#,
        );
        write_file(
            &storage.join("message/ses_1/msg_1.json"),
            r#"{"id":"msg_1","sessionID":"ses_1","role":"user","time":{"created":1000}}"#,
        );
        write_file(
            &storage.join("message/ses_1/msg_2.json"),
            r#"{"id":"msg_2","sessionID":"ses_1","role":"assistant","time":{"created":2000,"completed":3500},"modelID":"kimi","providerID":"nvidia","tokens":{"total":15,"input":10,"output":5,"cache":{"read":2,"write":0}}}"#,
        );
        write_file(
            &storage.join("part/msg_1/prt_1.json"),
            r#"{"id":"prt_1","type":"text","text":"Hello"}"#,
        );
        write_file(
            &storage.join("part/msg_2/prt_2.json"),
            r#"{"id":"prt_2","type":"reasoning","text":"think...","time":{"start":2000,"end":2100}}"#,
        );
        write_file(
            &storage.join("part/msg_2/prt_3.json"),
            r#"{"id":"prt_3","type":"tool","tool":"bash","callID":"call_1","state":{"status":"completed","input":{"command":"ls"},"output":"file.txt","time":{"start":2050,"end":2100}}}"#,
        );
        write_file(
            &storage.join("part/msg_2/prt_4.json"),
            r#"{"id":"prt_4","type":"text","text":"Here you go","time":{"start":2200,"end":2300}}"#,
        );
    }

    #[test]
    fn parse_json_layout_message_and_tool() {
        let dir = tempdir().unwrap();
        let storage = dir.path().join("storage");
        build_json_layout(&storage);
        let session_file = storage.join("session/project-1/ses_1.json");

        let (sid, events) = parse_trajectory(&session_file).unwrap();
        assert_eq!(sid, "ses_1");
        assert_eq!(events.len(), 4);

        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[0].role.as_deref(), Some("user"));
        assert_eq!(events[0].content.as_deref(), Some("Hello"));
        assert_eq!(events[0].turn, Some(1));
        assert_eq!(events[0].step, Some(0));

        assert_eq!(events[1].event_type, "tool-call");
        assert_eq!(events[1].tool_name.as_deref(), Some("bash"));
        assert!(events[1].tool_args.as_deref().unwrap().contains("ls"));
        assert_eq!(events[1].turn, Some(1));
        assert_eq!(events[1].step, Some(1));

        assert_eq!(events[2].event_type, "tool-result");
        assert_eq!(events[2].tool_result.as_deref(), Some("file.txt"));
        assert_eq!(events[2].is_error, Some(false));

        assert_eq!(events[3].event_type, "assistant-message");
        assert_eq!(events[3].content.as_deref(), Some("Here you go"));
        assert_eq!(events[3].model.as_deref(), Some("kimi"));
        assert_eq!(events[3].input_tokens, Some(10));
        assert_eq!(events[3].output_tokens, Some(5));
        assert_eq!(events[3].cache_read_tokens, Some(2));
        // Reasoning text surfaces as a content block, not main content.
        let blocks = events[3].content_blocks.as_ref().unwrap();
        assert!(blocks
            .iter()
            .any(|b| b.block_type == "reasoning" && b.text.as_deref() == Some("think...")));
    }

    #[test]
    fn load_messages_json_layout() {
        let dir = tempdir().unwrap();
        let storage = dir.path().join("storage");
        build_json_layout(&storage);
        let session_file = storage.join("session/project-1/ses_1.json");

        let msgs = load_messages(session_file.to_str().unwrap()).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("[Tool: bash]"));
        assert!(msgs[1].content.contains("Here you go"));
        assert_eq!(msgs[0].ts, Some(1000));
    }

    fn setup_sqlite(db: &Path) {
        let conn = Connection::open(db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, title, directory, time_created, time_updated) VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "Db Session", "/tmp/proj", 1000, 3000),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_1','ses_1',1000,1000,'{\"role\":\"user\"}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_2','ses_1',2000,2000,'{\"role\":\"assistant\",\"modelID\":\"kimi\",\"tokens\":{\"input\":10,\"output\":5}}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_1','msg_1','ses_1',1000,1000,'{\"type\":\"text\",\"text\":\"Hello db\"}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_2','msg_2','ses_1',2000,2000,'{\"type\":\"tool\",\"tool\":\"bash\",\"callID\":\"call_db\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"pwd\"},\"output\":\"/tmp\"}}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_3','msg_2','ses_1',2100,2100,'{\"type\":\"text\",\"text\":\"Done db\"}')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn parse_sqlite_layout_events() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        setup_sqlite(&db);

        let source = format!("sqlite:{}:ses_1", db.display());
        let (sid, events) = parse_trajectory(Path::new(&source)).unwrap();

        assert_eq!(sid, "ses_1");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_type, "user-message");
        assert_eq!(events[0].content.as_deref(), Some("Hello db"));
        assert_eq!(events[1].event_type, "tool-call");
        assert_eq!(events[1].tool_name.as_deref(), Some("bash"));
        assert_eq!(events[2].event_type, "tool-result");
        assert_eq!(events[2].tool_result.as_deref(), Some("/tmp"));
        assert_eq!(events[3].event_type, "assistant-message");
        assert_eq!(events[3].content.as_deref(), Some("Done db"));
        assert_eq!(events[3].input_tokens, Some(10));
    }

    #[test]
    fn scan_sessions_merges_json_and_sqlite_with_sqlite_priority() {
        let _guard = opencode_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        #[allow(deprecated)]
        std::env::set_var("XDG_DATA_HOME", dir.path());

        let base = dir.path().join("opencode");
        let storage = base.join("storage");
        std::fs::create_dir_all(&base).unwrap();

        // SQLite session ses_1 (also present as a JSON file → SQLite wins).
        setup_sqlite(&base.join("opencode.db"));
        write_file(
            &storage.join("session/project-1/ses_1.json"),
            r#"{"id":"ses_1","title":"Json Title","directory":"/tmp/proj","time":{"created":1000,"updated":5000}}"#,
        );
        // JSON-only session ses_2.
        write_file(
            &storage.join("session/project-2/ses_2.json"),
            r#"{"id":"ses_2","title":"Only Json","directory":"/tmp/other","time":{"created":2000,"updated":2000}}"#,
        );

        let sessions = scan_sessions();
        #[allow(deprecated)]
        std::env::remove_var("XDG_DATA_HOME");

        let by_id: HashMap<String, SessionMeta> = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect();
        // SQLite copy wins the duplicate.
        assert_eq!(by_id["ses_1"].title.as_deref(), Some("Db Session"));
        assert!(by_id["ses_1"]
            .source_path
            .as_deref()
            .unwrap()
            .starts_with("sqlite:"));
        // JSON-only session still present with its resume command.
        assert_eq!(by_id["ses_2"].title.as_deref(), Some("Only Json"));
        assert_eq!(
            by_id["ses_2"].resume_command.as_deref(),
            Some("opencode session resume ses_2")
        );
    }

    #[test]
    fn delete_json_session_removes_files_and_parts() {
        let _guard = opencode_env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        #[allow(deprecated)]
        std::env::set_var("XDG_DATA_HOME", dir.path());

        let storage = dir.path().join("opencode/storage");
        build_json_layout(&storage);
        let session_file = storage.join("session/project-1/ses_1.json");

        assert!(storage.join("part/msg_1/prt_1.json").exists());
        delete_session(session_file.to_str().unwrap()).unwrap();

        #[allow(deprecated)]
        std::env::remove_var("XDG_DATA_HOME");

        assert!(!session_file.exists());
        assert!(!storage.join("message/ses_1").exists());
        assert!(!storage.join("part/msg_1").exists());
        assert!(!storage.join("part/msg_2").exists());
    }

    // ── Real-data smoke tests (run with `cargo test -- --ignored`) ────────

    #[test]
    #[ignore]
    fn parse_real_opencode_session_json() {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let session_file = format!(
            r"{home}\.local\share\opencode\storage\session\114da7d99c6aa5f10148ac9ede1d9c4fb3d0879d\ses_3c89ad158ffehsPfY3RNV6GvUM.json"
        );
        if !Path::new(&session_file).exists() {
            return;
        }
        let (sid, events) = parse_trajectory(Path::new(&session_file)).unwrap();
        println!("sid={sid} events={}", events.len());
        assert!(!events.is_empty());
        assert_eq!(sid, "ses_3c89ad158ffehsPfY3RNV6GvUM");
        let tool_calls = events.iter().filter(|e| e.event_type == "tool-call").count();
        let asst = events.iter().filter(|e| e.event_type == "assistant-message").count();
        println!("tool-calls={tool_calls} assistant={asst}");
        assert!(tool_calls > 0);
    }

    #[test]
    #[ignore]
    fn parse_real_opencode_session_sqlite() {
        let base = get_opencode_base_dir();
        if base.is_none() {
            return;
        }
        let sessions = scan_sessions();
        println!("scanned {} opencode sessions", sessions.len());
        assert!(!sessions.is_empty());

        // Load the most recent session's trajectory from SQLite.
        let newest = sessions
            .iter()
            .max_by_key(|s| s.last_active_at.unwrap_or(0))
            .unwrap();
        let source = newest.source_path.as_deref().unwrap();
        let (sid, events) = parse_trajectory(Path::new(source)).unwrap();
        println!("top session: {sid} ({}) events={}", newest.title.as_deref().unwrap_or(""), events.len());
        assert_eq!(sid, newest.session_id);
        let msgs = load_messages(source).unwrap();
        println!("messages={}", msgs.len());
        assert!(!msgs.is_empty());

        // Dump the LARGEST session (most events) — the one most likely to
        // stress the front-end rendering path.
        let mut best: Option<(&SessionMeta, Vec<crate::trajectory::TrajectoryEvent>)> = None;
        for s in sessions.iter().take(25) {
            let sp = s.source_path.as_deref().unwrap();
            if let Ok((_, ev)) = parse_trajectory(Path::new(sp)) {
                if best.as_ref().map(|(_, be)| be.len()).unwrap_or(0) < ev.len() {
                    best = Some((s, ev));
                }
            }
        }
        let (largest, events) = best.expect("at least one session parses");
        println!(
            "largest session: {} events={}",
            largest.title.as_deref().unwrap_or("?"),
            events.len()
        );

        let mut out = String::from("seq\ttype\trole\tts\tturn\tstep\ttool\tcontentLen\targsLen\tresultLen\tblocks\n");
        for e in &events[..events.len().min(60)] {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}\t{}\t{}\n",
                e.seq,
                e.event_type,
                e.role.as_deref().unwrap_or("-"),
                e.ts,
                e.turn,
                e.step,
                e.tool_name.as_deref().unwrap_or("-"),
                e.content.as_ref().map(|s| s.len()).unwrap_or(0),
                e.tool_args.as_ref().map(|s| s.len()).unwrap_or(0),
                e.tool_result.as_ref().map(|s| s.len()).unwrap_or(0),
                e.content_blocks.as_ref().map(|b| b.len()).unwrap_or(0),
            ));
        }
        let dump = std::env::temp_dir().join("oc_events_dump.txt");
        let _ = std::fs::write(&dump, &out);
        println!("dump written to {}", dump.display());

        // Full events JSON for front-end render regression (vitest).
        let full = serde_json::to_string(&events).unwrap_or_default();
        let _ = std::fs::write(std::env::temp_dir().join("oc_events_real.json"), &full);

        let huge = events
            .iter()
            .filter(|e| e.content.as_ref().map(|s| s.len()).unwrap_or(0) > 100_000
                || e.tool_args.as_ref().map(|s| s.len()).unwrap_or(0) > 100_000
                || e.tool_result.as_ref().map(|s| s.len()).unwrap_or(0) > 100_000)
            .collect::<Vec<_>>();
        println!("events with >100KB payload: {}", huge.len());
    }

    #[test]
    #[ignore]
    fn dump_specific_session() {
        // The session the user reports as white-screening.
        let target = "ses_44975df13ffeqkFFqLxe4ZmZvW";
        let base = get_opencode_base_dir();
        if base.is_none() {
            return;
        }
        let db = base.unwrap().join("opencode.db");
        let source = format!("sqlite:{}:{}", db.display(), target);
        let (sid, events) = parse_trajectory(Path::new(&source)).unwrap();
        println!("target session: {sid} events={}", events.len());

        let full = serde_json::to_string(&events).unwrap_or_default();
        let out_path = std::env::temp_dir().join("oc_specific_events.json");
        let _ = std::fs::write(&out_path, &full);
        println!("wrote {} bytes to {}", full.len(), out_path.display());
    }
}