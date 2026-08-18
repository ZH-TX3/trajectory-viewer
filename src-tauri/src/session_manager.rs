// ── Session Manager ──────────────────────────────────────────────────────
//
// Scans Claude Code and Codex session directories, loads messages.

use std::path::{Path, PathBuf};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

/// Scan all sessions from Claude Code, Codex, and DSH.
pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();

    // Claude Code: ~/.claude/projects/*/*.jsonl
    if let Some(claude_dir) = claude_projects_dir() {
        let mut claude_files = Vec::new();
        collect_jsonl_files_recursive(&claude_dir, &mut claude_files);
        for path in claude_files {
            if let Some(meta) = parse_claude_session(&path) {
                sessions.push(meta);
            }
        }
    }

    // Codex: ~/.codex/sessions/YYYY/MM/DD/*.jsonl
    if let Some(codex_dir) = codex_sessions_dir() {
        let mut codex_files = Vec::new();
        collect_jsonl_files_recursive(&codex_dir, &mut codex_files);
        for path in codex_files {
            if let Some(meta) = parse_codex_session(&path) {
                sessions.push(meta);
            }
        }
    }

    // DSH: ~/.dsh/sessions/{project}/{session-id}/session.jsonl.zstd
    if let Some(dsh_dir) = dsh_sessions_dir() {
        let mut dsh_files = Vec::new();
        collect_zstd_files_recursive(&dsh_dir, &mut dsh_files);
        for path in dsh_files {
            if let Some(meta) = parse_dsh_session(&path) {
                sessions.push(meta);
            }
        }
    }

    // Sort by last_active_at descending
    sessions.sort_by(|a, b| {
        let a_ts = a.last_active_at.or(a.created_at).unwrap_or(0);
        let b_ts = b.last_active_at.or(b.created_at).unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    sessions
}

/// Load messages from a session file.
pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    let path = Path::new(source_path);
    match provider_id {
        "claude" => load_claude_messages(path),
        "codex" => load_codex_messages(path),
        "dsh" => crate::trajectory::parser::dsh::load_messages(path),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

// ── Claude Code ──────────────────────────────────────────────────────────

fn claude_projects_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let dir = PathBuf::from(home).join(".claude").join("projects");
    if dir.exists() { Some(dir) } else { None }
}

fn parse_claude_session(path: &Path) -> Option<SessionMeta> {
    use crate::trajectory::utils::{parse_timestamp_to_ms, read_head_tail_lines};

    // Skip agent sessions
    let fname = path.file_name()?.to_str()?;
    if fname.starts_with("agent-") {
        return None;
    }

    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;

    for line in &head {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if session_id.is_none() {
            session_id = value.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
        if project_dir.is_none() {
            project_dir = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if first_user_message.is_none() {
            let is_user = value.get("type").and_then(|v| v.as_str()) == Some("user")
                || value.get("message").and_then(|m| m.get("role")).and_then(|v| v.as_str()) == Some("user");
            if is_user {
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
        }
        if session_id.is_some() && project_dir.is_some() && created_at.is_some() && first_user_message.is_some() {
            break;
        }
    }

    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;
    let mut custom_title: Option<String> = None;

    for line in tail.iter().rev() {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if custom_title.is_none() && value.get("type").and_then(|v| v.as_str()) == Some("custom-title") {
            custom_title = value.get("customTitle").and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if summary.is_none() {
            if value.get("isMeta").and_then(|v| v.as_bool()) == Some(true) { continue; }
            if let Some(message) = value.get("message") {
                let text = extract_text(message);
                if !text.trim().is_empty() {
                    summary = Some(text);
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() && custom_title.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
    })?;

    let title = custom_title
        .or_else(|| first_user_message.map(|t| truncate(&t, 80)))
        .or_else(|| {
            project_dir.as_deref()
                .and_then(|d| d.trim_end_matches(['/', '\\']).split(['/', '\\']).next_back())
                .filter(|s| !s.is_empty())
                .map(|v| v.to_string())
        });

    let summary = summary.map(|text| truncate(&text, 160));

    Some(SessionMeta {
        provider_id: "claude".to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        project_group: claude_project_group(path),
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("claude --resume {session_id}")),
    })
}

fn load_claude_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {e}"))?;
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("isMeta").and_then(|v| v.as_bool()) == Some(true) { continue; }

        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };

        let mut role = message.get("role").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        // Reclassify tool_result-only user messages as "tool" role
        if role == "user" {
            if let Some(items) = message.get("content").and_then(|v| v.as_array()) {
                if !items.is_empty() && items.iter().all(|item| {
                    item.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                }) {
                    role = "tool".to_string();
                }
            }
        }

        let content = extract_text(message);
        if content.trim().is_empty() { continue; }

        let ts = value.get("timestamp").and_then(crate::trajectory::utils::parse_timestamp_to_ms);

        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

// ── Codex ────────────────────────────────────────────────────────────────

fn codex_sessions_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let dir = PathBuf::from(home).join(".codex").join("sessions");
    if dir.exists() { Some(dir) } else { None }
}

fn parse_codex_session(path: &Path) -> Option<SessionMeta> {
    use crate::trajectory::utils::{parse_timestamp_to_ms, read_head_tail_lines};

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
                    session_id = payload.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                if project_dir.is_none() {
                    project_dir = payload.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                if let Some(ts) = payload.get("timestamp").and_then(parse_timestamp_to_ms) {
                    created_at.get_or_insert(ts);
                }
            }
        }
        if first_user_message.is_none() && value.get("type").and_then(|v| v.as_str()) == Some("response_item") {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(|v| v.as_str()) == Some("message")
                    && payload.get("role").and_then(|v| v.as_str()) == Some("user")
                {
                    let text = extract_text(payload.get("content").unwrap_or(&serde_json::Value::Null));
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("# AGENTS.md") && !trimmed.starts_with("<environment_context>") {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
        }
        if session_id.is_some() && project_dir.is_some() && created_at.is_some() && first_user_message.is_some() {
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
        if summary.is_none() && value.get("type").and_then(|v| v.as_str()) == Some("response_item") {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(|v| v.as_str()) == Some("message") {
                    let text = extract_text(payload.get("content").unwrap_or(&serde_json::Value::Null));
                    if !text.trim().is_empty() {
                        summary = Some(text);
                    }
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() { break; }
    }

    let session_id = session_id.or_else(|| {
        path.file_name().and_then(|s| s.to_str())
            .and_then(|name| {
                // Extract UUID from filename
                use regex::Regex;
                let re = Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").ok()?;
                re.find(name).map(|m| m.as_str().to_string())
            })
    })?;

    let title = first_user_message
        .map(|t| truncate(&t, 80))
        .or_else(|| {
            project_dir.as_deref()
                .and_then(|d| d.trim_end_matches(['/', '\\']).split(['/', '\\']).next_back())
                .filter(|s| !s.is_empty())
                .map(|v| v.to_string())
        });

    let summary = summary.map(|text| truncate(&text, 160));

    Some(SessionMeta {
        provider_id: "codex".to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        project_group: codex_project_group(path),
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("codex resume {session_id}")),
    })
}

fn load_codex_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {e}"))?;
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("type").and_then(|v| v.as_str()) != Some("response_item") { continue; }

        let payload = match value.get("payload") {
            Some(p) => p,
            None => continue,
        };

        let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

        let (role, content) = match payload_type {
            "message" => {
                let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let content = extract_text(payload.get("content").unwrap_or(&serde_json::Value::Null));
                (role, content)
            }
            "function_call" => {
                let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                ("assistant".to_string(), format!("[Tool: {name}]"))
            }
            "function_call_output" => {
                let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string();
                ("tool".to_string(), output)
            }
            _ => continue,
        };

        if content.trim().is_empty() { continue; }

        let ts = value.get("timestamp").and_then(crate::trajectory::utils::parse_timestamp_to_ms);

        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract the project group name for a Claude session.
/// Uses the parent directory name of the JSONL file.
fn claude_project_group(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| decode_project_name(s))
}

/// Extract the project group name for a Codex session.
/// Uses the date-based directory structure: YYYY/MM/DD → "YYYY-MM"
fn codex_project_group(path: &Path) -> Option<String> {
    // Codex path: .../sessions/YYYY/MM/DD/rollout-*.jsonl
    let day = path.parent()?;
    let month = day.parent()?;
    let year = month.parent()?;
    let y = year.file_name()?.to_str()?;
    let m = month.file_name()?.to_str()?;
    Some(format!("{}-{}", y, m))
}

// ── DSH ──────────────────────────────────────────────────────────────────

fn dsh_sessions_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let dir = PathBuf::from(home).join(".dsh").join("sessions");
    if dir.exists() { Some(dir) } else { None }
}

fn dsh_project_group(path: &Path) -> Option<String> {
    // DSH path: .../sessions/{project}/{session-id}/session.jsonl.zstd
    // project group is the grandparent directory name
    let session_dir = path.parent()?;
    let project_dir = session_dir.parent()?;
    let name = project_dir.file_name()?.to_str()?;
    Some(decode_project_name(name))
}

/// Parse a DSH session by reading its metadata line.
fn parse_dsh_session(path: &Path) -> Option<SessionMeta> {
    use std::io::Read;

    // Decompress just enough to get the session metadata line
    let mut file = std::fs::File::open(path).ok()?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed).ok()?;

    let mut decompressed = Vec::new();
    let mut decoder = zstd::Decoder::new(std::io::Cursor::new(&compressed)).ok()?;
    let mut buf = [0u8; 4096];
    let n = decoder.read(&mut buf).ok()?;
    decompressed.extend_from_slice(&buf[..n]);

    let text = std::str::from_utf8(&decompressed).ok()?;
    let first_line = text.lines().next()?;
    let value: serde_json::Value = serde_json::from_str(first_line).ok()?;

    let session_id = value["id"].as_str()?.to_string();
    let created_at = value["createdAt"].as_i64();
    let project_dir = value["cwd"].as_str().map(|s| s.to_string());

    // Look for user/message to get title
    let mut title: Option<String> = None;
    for line in text.lines().skip(1).take(10) {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v["type"].as_str() == Some("user/message") {
            if let Some(content) = v["data"]["content"].as_array() {
                for item in content {
                    if let Some(text) = item["text"].as_str() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() && !trimmed.starts_with('<') {
                            title = Some(trimmed.to_string());
                            break;
                        }
                    }
                }
            }
            if title.is_some() { break; }
        }
    }

    Some(SessionMeta {
        provider_id: "dsh".to_string(),
        session_id,
        title: title.map(|t| truncate(&t, 80)),
        summary: None,
        project_dir,
        project_group: dsh_project_group(path),
        created_at,
        last_active_at: created_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: None,
    })
}

fn collect_zstd_files_recursive(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() { return; }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_zstd_files_recursive(&path, files);
        } else if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            if ext == "zstd" || ext == "zst" {
                files.push(path);
            }
        }
    }
}

/// Decode a project directory name like "D--DSH" → "D:/DSH"
fn decode_project_name(name: &str) -> String {
    // DSH/Claude Code encodes paths like "D--DSH" or "C--Users---"
    let mut result = name.to_string();
    if let Some(idx) = result.find("--") {
        let drive = &result[..idx];
        let rest = &result[idx + 2..];
        result = format!("{}:/{}", drive, rest.replace("--", "/").replace("---", "/"));
    }
    result
}

fn collect_jsonl_files_recursive(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() { return; }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files_recursive(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn extract_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items.iter()
            .filter_map(|item| {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if item_type == "tool_use" {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    Some(format!("[Tool: {name}]"))
                } else if item_type == "tool_result" {
                    item.get("content").map(extract_text)
                } else {
                    item.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                        .or_else(|| item.get("content").and_then(|v| v.as_str()).map(|s| s.to_string()))
                }
            })
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(map) => {
            map.get("text").and_then(|v| v.as_str())
                .or_else(|| map.get("content").and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string()
        }
        _ => String::new(),
    }
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