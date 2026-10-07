// ── Subagent Runs ────────────────────────────────────────────────────────
//
// Claude gives every dispatched subagent its own transcript beside the
// session file:
//
//   projects/{project}/{session}.jsonl        main session
//   projects/{project}/{session}/subagents/
//       agent-{id}.jsonl                      the run's own trajectory
//       agent-{id}.meta.json                  type, tree links, flags
//
// `meta.toolUseId` points at the `Agent` tool_use in the main session, which
// is how a dispatch row finds its run. The run's *outcome* is not in the meta
// though — it comes from the main session, either inline on the dispatch's
// `toolUseResult` (synchronous runs) or from a later `<task-notification>`
// user line (background runs). `collect_signals` gathers both in one pass.
//
// Transcripts are loaded on demand rather than folded into `TrajectoryData`:
// one session can hold a dozen runs and thousands of events between them.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::trajectory::utils::parse_timestamp_to_ms;
use crate::trajectory::{parser, TrajectoryData};

/// Token counts a finished run reported back to the main session.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
}

/// What a run did with its tools, per Claude's own tally.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStats {
    pub read_count: i64,
    pub search_count: i64,
    pub bash_count: i64,
    pub edit_file_count: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub other_tool_count: i64,
}

/// One dispatched subagent: its identity, its place in the spawn tree, and how
/// it ended. `has_transcript` gates the drill-down entry point.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRun {
    /// Bare agent id (`a5d9764d0ae64ef79`) — matches `parentAgentId` and the
    /// `<task-id>` in a background run's notification.
    pub agent_id: String,
    /// The `Agent` tool_use in the main session that spawned this run.
    pub tool_call_id: Option<String>,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    /// Explicit name, set by workflow-driven spawns.
    pub name: Option<String>,
    pub spawn_depth: usize,
    pub parent_agent_id: Option<String>,
    pub is_fork: bool,
    pub stopped_by_user: bool,
    pub model: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    /// `completed` | `failed` | `stopped` | `killed` | `async_launched` | `forked`.
    pub status: Option<String>,
    pub duration_ms: Option<i64>,
    pub total_tokens: Option<i64>,
    pub tool_use_count: Option<i64>,
    pub usage: Option<SubagentUsage>,
    pub stats: Option<SubagentStats>,
    /// The run's answer: inline for synchronous runs, from the task
    /// notification for background ones.
    pub result: Option<String>,
    pub has_transcript: bool,
}

/// Directory holding one session's subagent transcripts, if it has any.
pub fn subagents_dir(session_path: &Path) -> Option<PathBuf> {
    let stem = session_path.file_stem()?.to_str()?;
    let dir = session_path.parent()?.join(stem).join("subagents");
    dir.is_dir().then_some(dir)
}

/// List every subagent dispatched by `session_path`, enriched with the outcome
/// recorded in the main session.
pub fn scan(session_path: &Path) -> Vec<SubagentRun> {
    let Some(dir) = subagents_dir(session_path) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut metas: Vec<(String, Value)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(agent_id) = name
            .strip_suffix(".meta.json")
            .and_then(|stem| stem.strip_prefix("agent-"))
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        metas.push((agent_id.to_string(), meta));
    }
    if metas.is_empty() {
        return Vec::new();
    }

    let signals = collect_signals(session_path);
    let parents: HashMap<&str, &str> = metas
        .iter()
        .filter_map(|(id, meta)| Some((id.as_str(), meta.get("parentAgentId")?.as_str()?)))
        .collect();

    let mut runs: Vec<SubagentRun> = metas
        .iter()
        .map(|(id, meta)| build_run(id, meta, &dir, &signals, &parents))
        .collect();
    runs.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    runs
}

/// Resolve one subagent's transcript path, validating the id.
///
/// `session_path` is the MAIN session file — nested runs share the parent
/// session's `subagents/` directory, so that is where every transcript lives.
pub fn transcript_path(session_path: &Path, agent_id: &str) -> Result<PathBuf, String> {
    // The id reaches us from the frontend and names a file; keep it to the
    // character set Claude actually emits so it can't escape the directory.
    if agent_id.is_empty()
        || !agent_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("Invalid agent id: {agent_id}"));
    }

    let dir = subagents_dir(session_path)
        .ok_or_else(|| "This session has no subagent transcripts".to_string())?;
    let transcript = dir.join(format!("agent-{agent_id}.jsonl"));
    if !transcript.is_file() {
        return Err(format!("No transcript stored for agent {agent_id}"));
    }
    Ok(transcript)
}

/// Load one subagent's own trajectory.
pub fn parse_subagent_trajectory(
    session_path: &Path,
    agent_id: &str,
) -> Result<TrajectoryData, String> {
    let transcript = transcript_path(session_path, agent_id)?;
    let (_, events) = parser::claude::parse_trajectory(&transcript)?;
    // The full run list travels along so nested dispatches inside this
    // transcript resolve their own rows (lookup is by tool call id).
    Ok(crate::trajectory::build_trajectory_data(
        "claude",
        agent_id.to_string(),
        events,
        scan(session_path),
    ))
}

// ── Enrichment from the main session ──────────────────────────────────────

/// What the main session records about a dispatch, gathered in one pass.
#[derive(Debug, Default)]
struct Dispatch {
    ts: Option<i64>,
    /// The `toolUseResult` object sitting on the result line.
    result: Option<Value>,
    /// The inline `tool_result` text — Claude's "launched" banner for a
    /// background run, the actual answer for a synchronous one.
    result_text: Option<String>,
}

#[derive(Debug)]
struct Notification {
    ts: Option<i64>,
    status: Option<String>,
    result: Option<String>,
}

#[derive(Debug, Default)]
struct Signals {
    dispatches: HashMap<String, Dispatch>,
    notifications: HashMap<String, Notification>,
}

fn collect_signals(session_path: &Path) -> Signals {
    let mut signals = Signals::default();
    let Ok(file) = File::open(session_path) else {
        return signals;
    };

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(json) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let ts = parse_timestamp_to_ms(&json["timestamp"]);
        let content = &json["message"]["content"];

        // A background run reports back on a later user line.
        if let Some(text) = content_text(content) {
            if text.trim_start().starts_with("<task-notification>") {
                if let Some(id) = xml_field(text, "tool-use-id") {
                    signals.notifications.insert(
                        id.to_string(),
                        Notification {
                            ts,
                            status: xml_field(text, "status").map(str::to_string),
                            result: xml_field(text, "result").map(str::to_string),
                        },
                    );
                }
                continue;
            }
        }

        let Some(blocks) = content.as_array() else {
            continue;
        };

        for block in blocks {
            if block["type"].as_str() != Some("tool_use") {
                continue;
            }
            let name = block["name"].as_str().unwrap_or("");
            if name != "Agent" && name != "Task" {
                continue;
            }
            if let Some(id) = block["id"].as_str() {
                signals.dispatches.entry(id.to_string()).or_default().ts = ts;
            }
        }

        // `toolUseResult` sits on the line, not the block, so it can only be
        // attributed when the line carries exactly one result. `agentId` marks
        // it as a dispatch outcome and keeps every other tool out of the map.
        let results: Vec<&Value> = blocks
            .iter()
            .filter(|b| b["type"].as_str() == Some("tool_result"))
            .collect();
        if results.len() != 1 {
            continue;
        }
        let Some(tool_use_result) = json.get("toolUseResult") else {
            continue;
        };
        if tool_use_result.get("agentId").is_none() {
            continue;
        }
        let Some(id) = results[0]["tool_use_id"].as_str() else {
            continue;
        };
        let entry = signals.dispatches.entry(id.to_string()).or_default();
        entry.result = Some(tool_use_result.clone());
        entry.result_text = extract_result_text(&results[0]["content"]);
    }

    signals
}

fn build_run(
    agent_id: &str,
    meta: &Value,
    dir: &Path,
    signals: &Signals,
    parents: &HashMap<&str, &str>,
) -> SubagentRun {
    let tool_call_id = text(meta, "toolUseId");
    let dispatch = tool_call_id
        .as_ref()
        .and_then(|id| signals.dispatches.get(id));
    let notification = tool_call_id
        .as_ref()
        .and_then(|id| signals.notifications.get(id));
    let outcome = dispatch.and_then(|d| d.result.as_ref());
    let is_fork = meta.get("isFork").and_then(Value::as_bool).unwrap_or(false);

    let status = notification
        .and_then(|n| n.status.clone())
        .or_else(|| outcome.and_then(|o| text(o, "status")))
        .or_else(|| is_fork.then(|| "forked".to_string()));

    // A background dispatch's inline result is Claude's launch banner, not the
    // run's answer, so it is only used when the run finished synchronously.
    let result = notification
        .and_then(|n| n.result.clone())
        .or_else(|| outcome.and_then(extract_result_text))
        .or_else(|| {
            (status.as_deref() != Some("async_launched"))
                .then(|| dispatch.and_then(|d| d.result_text.clone()))
                .flatten()
        });

    let duration_ms = outcome
        .and_then(|o| o.get("totalDurationMs").and_then(Value::as_i64))
        .or_else(
            || match (dispatch.and_then(|d| d.ts), notification.and_then(|n| n.ts)) {
                (Some(start), Some(end)) if end >= start => Some(end - start),
                _ => None,
            },
        );

    SubagentRun {
        agent_id: agent_id.to_string(),
        tool_call_id,
        agent_type: text(meta, "agentType"),
        description: text(meta, "description"),
        name: text(meta, "name"),
        spawn_depth: resolve_depth(agent_id, meta, parents),
        parent_agent_id: text(meta, "parentAgentId"),
        is_fork,
        stopped_by_user: meta
            .get("stoppedByUser")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        model: outcome
            .and_then(|o| text(o, "resolvedModel"))
            .or_else(|| text(meta, "model")),
        worktree_path: text(meta, "worktreePath"),
        worktree_branch: text(meta, "worktreeBranch"),
        status,
        duration_ms,
        total_tokens: outcome.and_then(|o| o.get("totalTokens").and_then(Value::as_i64)),
        tool_use_count: outcome.and_then(|o| o.get("totalToolUseCount").and_then(Value::as_i64)),
        usage: outcome.and_then(|o| o.get("usage")).and_then(parse_usage),
        stats: outcome
            .and_then(|o| o.get("toolStats"))
            .and_then(parse_stats),
        result,
        has_transcript: dir.join(format!("agent-{agent_id}.jsonl")).is_file(),
    }
}

/// Nesting level, 1 for a run dispatched by the main session. The parent chain
/// is authoritative; `spawnDepth` is only a fallback for entries that carry no
/// `parentAgentId`.
fn resolve_depth(agent_id: &str, meta: &Value, parents: &HashMap<&str, &str>) -> usize {
    let mut depth = 1usize;
    let mut cursor = agent_id;
    // A malformed chain must not spin forever.
    for _ in 0..32 {
        let Some(parent) = parents.get(cursor) else {
            break;
        };
        depth += 1;
        cursor = parent;
    }
    if depth > 1 {
        return depth;
    }
    meta.get("spawnDepth")
        .and_then(Value::as_u64)
        .filter(|d| *d > 0)
        .map(|d| d as usize)
        .unwrap_or(1)
}

fn parse_usage(value: &Value) -> Option<SubagentUsage> {
    if !value.is_object() {
        return None;
    }
    Some(SubagentUsage {
        input_tokens: value.get("input_tokens").and_then(Value::as_i64),
        output_tokens: value.get("output_tokens").and_then(Value::as_i64),
        cache_read_tokens: value.get("cache_read_input_tokens").and_then(Value::as_i64),
        cache_write_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64),
    })
}

fn parse_stats(value: &Value) -> Option<SubagentStats> {
    if !value.is_object() {
        return None;
    }
    let field = |key: &str| value.get(key).and_then(Value::as_i64).unwrap_or(0);
    Some(SubagentStats {
        read_count: field("readCount"),
        search_count: field("searchCount"),
        bash_count: field("bashCount"),
        edit_file_count: field("editFileCount"),
        lines_added: field("linesAdded"),
        lines_removed: field("linesRemoved"),
        other_tool_count: field("otherToolCount"),
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// The plain text of a message `content`, which is either a string or an array
/// of blocks.
fn content_text(content: &Value) -> Option<&str> {
    match content {
        Value::String(s) => Some(s.as_str()),
        Value::Array(blocks) => blocks
            .iter()
            .find_map(|b| b.get("text").and_then(Value::as_str)),
        _ => None,
    }
}

/// A tool result's text, which is a string or a list of `{type, text}` blocks.
fn extract_result_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => (!s.trim().is_empty()).then(|| s.clone()),
        Value::Array(items) => {
            let parts: Vec<&str> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Object(_) => text(value, "text"),
        _ => None,
    }
}

/// Read `<tag>value</tag>` out of a task notification.
fn xml_field<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Lay out one session with a single background subagent dispatch.
    fn fixture(dir: &Path) -> PathBuf {
        let session = dir.join("proj").join("sess-1.jsonl");
        let subagents = dir.join("proj").join("sess-1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();

        std::fs::write(
            &session,
            concat!(
                r#"{"sessionId":"sess-1","timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"Agent","input":{"description":"Audit the parser","subagent_type":"Explore","run_in_background":true}}]},"timestamp":"2026-03-06T10:01:00Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"Async agent launched successfully."}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"aaa","description":"Audit the parser","resolvedModel":"deepseek-v4-pro","prompt":"p"},"timestamp":"2026-03-06T10:01:01Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>aaa</task-id>\n<tool-use-id>call_1</tool-use-id>\n<status>completed</status>\n<result>The parser is flat.</result>\n</task-notification>"},"timestamp":"2026-03-06T10:03:00Z"}"#,
                "\n",
            ),
        )
        .unwrap();

        std::fs::write(
            subagents.join("agent-aaa.meta.json"),
            r#"{"agentType":"Explore","description":"Audit the parser","toolUseId":"call_1","spawnDepth":1}"#,
        )
        .unwrap();
        std::fs::write(
            subagents.join("agent-aaa.jsonl"),
            concat!(
                r#"{"sessionId":"sess-1","isSidechain":true,"agentId":"aaa","type":"user","message":{"role":"user","content":"Audit the parser"},"timestamp":"2026-03-06T10:01:02Z"}"#,
                "\n",
                r#"{"sessionId":"sess-1","isSidechain":true,"agentId":"aaa","type":"assistant","message":{"role":"assistant","content":"The parser is flat.","usage":{"input":10,"output":5}},"model":"deepseek-v4-pro","timestamp":"2026-03-06T10:02:00Z"}"#,
                "\n",
            ),
        )
        .unwrap();

        session
    }

    #[test]
    fn scan_links_dispatch_to_its_transcript() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());

        let runs = scan(&session);
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.agent_id, "aaa");
        assert_eq!(run.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(run.agent_type.as_deref(), Some("Explore"));
        assert_eq!(run.spawn_depth, 1);
        assert!(run.has_transcript);
        assert_eq!(run.model.as_deref(), Some("deepseek-v4-pro"));
    }

    // A background run's inline result is Claude's launch banner; the real
    // answer only arrives on the later task notification.
    #[test]
    fn background_run_result_comes_from_the_notification() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());

        let runs = scan(&session);
        assert_eq!(runs[0].status.as_deref(), Some("completed"));
        assert_eq!(runs[0].result.as_deref(), Some("The parser is flat."));
        // Notification timestamp (10:03:00) bounds the launch (10:01:00).
        assert_eq!(runs[0].duration_ms, Some(120_000));
    }

    #[test]
    fn scan_without_subagents_dir_is_empty() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("plain.jsonl");
        std::fs::write(&session, "{}\n").unwrap();
        assert!(scan(&session).is_empty());
    }

    #[test]
    fn parse_transcript_returns_the_runs_own_events() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());

        let data = parse_subagent_trajectory(&session, "aaa").unwrap();
        assert_eq!(data.session_id, "aaa");
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[0].event_type, "user-message");
        assert_eq!(data.events[1].event_type, "assistant-message");
        // Nested dispatches inside the transcript still resolve their rows.
        assert_eq!(data.subagents.len(), 1);
    }

    #[test]
    fn parse_transcript_rejects_a_traversing_id() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());
        assert!(parse_subagent_trajectory(&session, "../secrets").is_err());
        assert!(parse_subagent_trajectory(&session, "").is_err());
    }

    #[test]
    fn parse_transcript_reports_a_missing_file() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());
        let err = parse_subagent_trajectory(&session, "nope").unwrap_err();
        assert!(err.contains("nope"), "unexpected error: {err}");
    }

    #[test]
    fn nested_runs_get_their_depth_from_the_parent_chain() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());
        let subagents = dir.path().join("proj").join("sess-1").join("subagents");
        std::fs::write(
            subagents.join("agent-bbb.meta.json"),
            r#"{"agentType":"Explore","toolUseId":"call_2","parentAgentId":"aaa"}"#,
        )
        .unwrap();

        let runs = scan(&session);
        let bbb = runs.iter().find(|r| r.agent_id == "bbb").unwrap();
        assert_eq!(bbb.spawn_depth, 2);
        assert_eq!(bbb.parent_agent_id.as_deref(), Some("aaa"));
        // No transcript written for bbb — the drill-down entry point is hidden.
        assert!(!bbb.has_transcript);
    }

    #[test]
    fn stats_and_usage_are_read_off_the_outcome() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("proj").join("sess-2.jsonl");
        let subagents = dir.path().join("proj").join("sess-2").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(
            &session,
            concat!(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_9","name":"Task","input":{}}]},"timestamp":"2026-03-06T10:00:00Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_9","content":[{"type":"text","text":"done"}]}]},"toolUseResult":{"status":"completed","agentId":"ccc","totalTokens":33051,"totalDurationMs":421136,"totalToolUseCount":29,"usage":{"input_tokens":31631,"output_tokens":1420},"toolStats":{"readCount":0,"searchCount":2,"bashCount":0,"editFileCount":0,"linesAdded":0,"linesRemoved":0,"otherToolCount":27}},"timestamp":"2026-03-06T10:07:01Z"}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            subagents.join("agent-ccc.meta.json"),
            r#"{"agentType":"claude-code-guide","description":"Look it up","toolUseId":"call_9"}"#,
        )
        .unwrap();

        let runs = scan(&session);
        let run = &runs[0];
        assert_eq!(run.status.as_deref(), Some("completed"));
        assert_eq!(run.result.as_deref(), Some("done"));
        assert_eq!(run.total_tokens, Some(33051));
        assert_eq!(run.duration_ms, Some(421136));
        assert_eq!(run.tool_use_count, Some(29));
        assert_eq!(run.usage.as_ref().unwrap().input_tokens, Some(31631));
        assert_eq!(run.stats.as_ref().unwrap().other_tool_count, 27);
    }

    #[test]
    fn a_user_line_quoting_a_notification_is_not_one() {
        let dir = tempdir().unwrap();
        let session = fixture(dir.path());
        let subagents = dir.path().join("proj").join("sess-1").join("subagents");
        // Same tool call, but the notification is absent from this session —
        // the run keeps its async status and the launch banner is discarded.
        std::fs::remove_file(subagents.join("agent-aaa.jsonl")).unwrap();
        std::fs::write(
            &session,
            concat!(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"Agent","input":{}}]},"timestamp":"2026-03-06T10:01:00Z"}"#,
                "\n",
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"Async agent launched successfully."}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"aaa"},"timestamp":"2026-03-06T10:01:01Z"}"#,
                "\n",
            ),
        )
        .unwrap();

        let runs = scan(&session);
        assert_eq!(runs[0].status.as_deref(), Some("async_launched"));
        assert_eq!(runs[0].result, None);
        assert!(!runs[0].has_transcript);
    }

    /// Smoke test over the real Claude corpus. Not run by default (it walks
    /// every session on the machine): `cargo test -- --ignored --nocapture`.
    ///
    /// Guards the two things fixtures cannot: that `scan` finds the runs a
    /// real session actually wrote, and that every transcript it offers to
    /// open parses.
    #[test]
    #[ignore]
    fn real_corpus_scan_and_transcripts() {
        let Some(home) = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
        else {
            eprintln!("no home dir; skipping");
            return;
        };
        let root = std::path::Path::new(&home).join(".claude").join("projects");
        if !root.is_dir() {
            eprintln!("no {}; skipping", root.display());
            return;
        }

        let (mut sessions, mut runs_total, mut linked, mut nested) = (0, 0, 0, 0);
        let (mut parsed, mut failed) = (0, 0);

        for project in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let Ok(entries) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                sessions += 1;
                for run in scan(&path) {
                    runs_total += 1;
                    if run.tool_call_id.is_some() {
                        linked += 1;
                    }
                    if run.spawn_depth > 1 {
                        nested += 1;
                    }
                    if !run.has_transcript {
                        continue;
                    }
                    match parse_subagent_trajectory(&path, &run.agent_id) {
                        Ok(data) => {
                            parsed += 1;
                            assert_eq!(data.session_id, run.agent_id);
                        }
                        Err(err) => {
                            failed += 1;
                            eprintln!("{} / {}: {err}", path.display(), run.agent_id);
                        }
                    }
                }
            }
        }

        eprintln!(
            "sessions={sessions} runs={runs_total} linked={linked} nested={nested} \
             transcripts parsed={parsed} failed={failed}"
        );
        assert!(runs_total > 0, "expected the corpus to contain subagents");
        assert_eq!(failed, 0, "every offered transcript must parse");
    }
}
