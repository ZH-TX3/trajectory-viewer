// ── Trajectory Data Model ──────────────────────────────────────────────────

use serde::Serialize;

/// Unified trajectory data returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryData {
    pub session_id: String,
    pub provider_id: String,
    pub events: Vec<TrajectoryEvent>,
    pub metadata: TrajectoryMetadata,
}

/// Session-level trajectory metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryMetadata {
    pub model: Option<String>,
    pub total_input_tokens: Option<i64>,
    pub total_output_tokens: Option<i64>,
    pub total_duration_ms: Option<i64>,
    pub event_count: usize,
}

/// One trajectory event in the session timeline.
///
/// `event_type` values:
/// - `"user-message"`     — user input
/// - `"assistant-message"` — assistant response
/// - `"tool-call"`        — tool invocation
/// - `"tool-result"`      — tool execution result
/// - `"turn-boundary"`    — turn separator
/// - `"compaction"`       — context compaction record
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryEvent {
    pub seq: usize,
    pub ts: i64,
    pub event_type: String,
    pub role: Option<String>,
    pub content: Option<String>,
    pub content_blocks: Option<Vec<ContentBlock>>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub tool_result: Option<String>,
    pub is_error: Option<bool>,
    pub turn: Option<usize>,
    pub step: Option<usize>,
    pub duration_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub model: Option<String>,
    pub provider: Option<String>,
}

/// A single content block within a message.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentBlock {
    pub block_type: String, // "text" | "reasoning" | "tool-call" | "tool-result" | "image"
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub image_src: Option<String>,
}

pub mod parser;
pub mod utils;

use std::path::Path;

/// Inspect the first lines of a JSONL file and identify the provider format.
pub fn detect_provider(path: &Path) -> Result<String, String> {
    // Check if it's a zstd-compressed DSH file
    let is_zstd = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "zstd" || ext == "zst")
        .unwrap_or(false);

    if is_zstd {
        // DSH files are zstd-compressed JSONL
        if let Ok(content) = decompress_head(path, 5) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let json: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let line_type = json["type"].as_str().unwrap_or("");
                if line_type == "session"
                    || line_type == "user/message"
                    || line_type == "assistant/message"
                {
                    return Ok("dsh".to_string());
                }
            }
        }
        return Err("Unable to detect provider in zstd file".to_string());
    }

    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let reader = BufReader::new(file);

    for (index, line) in reader.lines().enumerate() {
        if index >= 200 {
            break;
        }
        let line = line.map_err(|e| format!("Failed to read file: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let json: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let line_type = json["type"].as_str().unwrap_or("").to_string();

        // DSH format (plain JSONL, not compressed)
        if matches!(
            line_type.as_str(),
            "user/message" | "assistant/message" | "tool/call" | "tool/result"
        ) {
            return Ok("dsh".to_string());
        }
        // DSH session metadata
        if line_type == "session" && json["id"].as_str().is_some() {
            return Ok("dsh".to_string());
        }

        // Codex marks every payload-bearing line with `type: "response_item"`.
        if line_type == "response_item" {
            return Ok("codex".to_string());
        }
        // Codex session_meta appears before response_item in real rollouts.
        if line_type == "session_meta" && json["payload"]["id"].as_str().is_some() {
            return Ok("codex".to_string());
        }
        // Claude Code session files carry `sessionId` and a `message` object.
        if json["sessionId"].as_str().is_some() {
            return Ok("claude".to_string());
        }
        if json["message"]["role"].as_str().is_some() {
            return Ok("claude".to_string());
        }
        if matches!(
            line_type.as_str(),
            "user" | "assistant" | "tool_call" | "tool_result" | "tool_use" | "custom-title"
        ) {
            return Ok("claude".to_string());
        }
    }

    Err("Unable to detect provider. Supported formats: Claude Code, Codex, DSH.".to_string())
}

/// Decompress the first few lines of a zstd file.
fn decompress_head(path: &Path, max_lines: usize) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot open: {e}"))?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)
        .map_err(|e| format!("Read error: {e}"))?;

    // Decompress in chunks, only read enough to get the first few lines
    let mut decompressed = Vec::new();
    let mut decoder = zstd::Decoder::new(std::io::Cursor::new(&compressed))
        .map_err(|e| format!("Zstd error: {e}"))?;

    let mut buf = [0u8; 8192];
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|e| format!("Decompress error: {e}"))?;
        if n == 0 {
            break;
        }
        decompressed.extend_from_slice(&buf[..n]);

        // Check if we have enough lines
        let text = std::str::from_utf8(&decompressed).map_err(|_| "Invalid UTF-8".to_string())?;
        if text.lines().count() >= max_lines {
            break;
        }
    }

    String::from_utf8(decompressed).map_err(|e| format!("UTF-8 error: {e}"))
}

/// Parse a trajectory file, auto-detecting the provider.
pub fn parse_trajectory_file(source_path: &str) -> Result<TrajectoryData, String> {
    let provider_id = detect_provider(Path::new(source_path))?;
    parse_trajectory(&provider_id, source_path)
}

/// Parse trajectory data for a specific provider.
pub fn parse_trajectory(provider_id: &str, source_path: &str) -> Result<TrajectoryData, String> {
    let path = Path::new(source_path);
    let (session_id, events) = match provider_id {
        "codex" => parser::codex::parse_trajectory(path)?,
        "dsh" => parser::dsh::parse_trajectory(path)?,
        "claude" => parser::claude::parse_trajectory(path)?,
        _ => {
            return Err(format!(
                "Trajectory not yet supported for provider: {provider_id}"
            ))
        }
    };

    let mut total_input_tokens: Option<i64> = None;
    let mut total_output_tokens: Option<i64> = None;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;
    let mut model: Option<String> = None;

    for event in &events {
        if first_ts.is_none() {
            first_ts = Some(event.ts);
        }
        last_ts = Some(event.ts);

        if let Some(t) = event.input_tokens {
            total_input_tokens = Some(total_input_tokens.unwrap_or(0) + t);
        }
        if let Some(t) = event.output_tokens {
            total_output_tokens = Some(total_output_tokens.unwrap_or(0) + t);
        }
        if model.is_none() {
            model = event.model.clone();
        }
    }

    let total_duration_ms = match (first_ts, last_ts) {
        (Some(f), Some(l)) if l >= f => Some(l - f),
        _ => None,
    };

    let event_count = events.len();

    Ok(TrajectoryData {
        session_id,
        provider_id: provider_id.to_string(),
        events,
        metadata: TrajectoryMetadata {
            model,
            total_input_tokens,
            total_output_tokens,
            total_duration_ms,
            event_count,
        },
    })
}
