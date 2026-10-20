// ── DSH sessions ─────────────────────────────────────────────────────────
//
// Zstd-compressed JSONL at ~/.dsh/sessions/{project}/{session-id}/session.jsonl.zstd.
// The first line is session metadata (`id`, `createdAt`, `cwd`); titles come
// from `session/title` events (LLM-generated), falling back to the first real
// user message.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::session_manager::provider::SessionProvider;
use crate::session_manager::utils::{home_dir, truncate};
use crate::session_manager::{SessionMessage, SessionMeta};

/// Titles are generated early, so this much decompressed input is plenty.
const TITLE_SCAN_LIMIT: usize = 1_048_576;

pub struct DshProvider;

impl SessionProvider for DshProvider {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn sessions_dir(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".dsh").join("sessions");
        dir.exists().then_some(dir)
    }

    fn file_extensions(&self) -> Option<&'static [&'static str]> {
        Some(&["zstd", "zst"])
    }

    fn parse_session(&self, path: &Path) -> Option<SessionMeta> {
        let text = read_head(path, TITLE_SCAN_LIMIT)?;

        let first_line = text.lines().next()?;
        let value: serde_json::Value = serde_json::from_str(first_line).ok()?;

        let session_id = value["id"].as_str()?.to_string();
        let created_at = value["createdAt"].as_i64();
        let project_dir = value["cwd"].as_str().map(String::from);

        let mut title: Option<String> = None;
        let mut first_user: Option<String> = None;
        for line in text.lines().skip(1) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            match v["type"].as_str() {
                Some("session/title") => {
                    if let Some(t) = v["data"]["title"]
                        .as_str()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        title = Some(t);
                    }
                }
                Some("user/message") if first_user.is_none() => {
                    first_user = first_text_block(&v["data"]["content"]);
                }
                _ => {}
            }
        }

        // Computed before the literal below moves `project_dir`.
        let project_group = project_group(path, project_dir.as_deref());

        Some(SessionMeta {
            provider_id: "dsh".to_string(),
            session_id: session_id.clone(),
            title: title.or(first_user).map(|t| truncate(&t, 80)),
            summary: None,
            project_dir,
            project_group,
            created_at,
            last_active_at: created_at,
            source_path: Some(path.to_string_lossy().to_string()),
            resume_command: crate::resume::display_command("dsh", &session_id),
        })
    }

    fn load_messages(&self, source: &str) -> Result<Vec<SessionMessage>, String> {
        let text = read_head(Path::new(source), usize::MAX)
            .ok_or_else(|| format!("Cannot read DSH session: {source}"))?;

        let mut messages = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            let ts = json["time"].as_i64();

            let (role, content) = match json["type"].as_str() {
                Some("user/message") => ("user", text_blocks(&json["data"]["content"])),
                Some("assistant/message") => {
                    let content = json["data"]["message"]["content"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| match item["type"].as_str() {
                                    Some("text") => item["text"].as_str().map(String::from),
                                    Some("tool-call") => Some(format!(
                                        "[Tool: {}]",
                                        item["name"].as_str().unwrap_or("unknown")
                                    )),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    ("assistant", content)
                }
                Some("tool/result") => ("tool", text_blocks(&json["data"]["message"]["content"])),
                _ => continue,
            };

            if !content.trim().is_empty() {
                messages.push(SessionMessage {
                    role: role.to_string(),
                    content,
                    ts,
                    ..Default::default()
                });
            }
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

/// Decompress at most `limit` bytes of a zstd session file.
fn read_head(path: &Path, limit: usize) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed).ok()?;

    let mut decompressed = Vec::new();
    let mut decoder = zstd::Decoder::new(std::io::Cursor::new(&compressed)).ok()?;
    let mut buf = [0u8; 65536];
    loop {
        let n = decoder.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        decompressed.extend_from_slice(&buf[..n]);
        if decompressed.len() >= limit {
            break;
        }
    }

    String::from_utf8(decompressed).ok()
}

/// Concatenate the `text` blocks of a DSH content array.
fn text_blocks(value: &serde_json::Value) -> String {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if item["type"].as_str() == Some("text") {
                        item["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The first text block that isn't an injected `<...>` marker.
fn first_text_block(value: &serde_json::Value) -> Option<String> {
    value.as_array()?.iter().find_map(|item| {
        item["text"]
            .as_str()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.starts_with('<'))
            .map(String::from)
    })
}

/// DSH groups by project: `sessions/{project}/{session-id}/session.jsonl.zstd`.
///
/// The `cwd` recorded in the session header is authoritative, exactly as it is
/// for Claude: it is the real path, while the directory name is an encoding.
/// The name is decoded only when a session predates the header.
fn project_group(path: &Path, cwd: Option<&str>) -> Option<String> {
    if let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) {
        return Some(cwd.to_string());
    }
    let name = path.parent()?.parent()?.file_name()?.to_str()?;
    Some(decode_dsh_project(name))
}

/// Decode a DSH project directory name.
///
/// DSH writes `--<path>--`, using `-` as the path separator and escaping each
/// character that is not path-safe as `~XXXX`, a hex code point:
/// `--C-Users-~9648~5F3A--` is `C:/Users/陈强`.
///
/// Claude's decoder does not apply to this shape — its leading `--` is the drive
/// separator, whereas DSH's is a wrapper. A literal `-` is not escaped, so it
/// cannot be told apart from a separator; that ambiguity is why the recorded
/// `cwd` wins whenever it is there.
fn decode_dsh_project(name: &str) -> String {
    let inner = name.strip_prefix("--").unwrap_or(name);
    let inner = inner.strip_suffix("--").unwrap_or(inner);
    let segments: Vec<String> = inner.split('-').map(unescape_code_points).collect();

    // A lone ASCII letter in front is a Windows drive.
    if segments.len() >= 2
        && segments[0].len() == 1
        && segments[0].chars().all(|c| c.is_ascii_alphabetic())
    {
        return format!(
            "{}:/{}",
            segments[0].to_ascii_uppercase(),
            segments[1..].join("/")
        );
    }
    segments.join("/")
}

/// Expand the `~XXXX` escapes in one path segment. Anything that is not a
/// four-digit code point is left as a literal tilde.
fn unescape_code_points(segment: &str) -> String {
    let mut out = String::new();
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '~' {
            out.push(c);
            continue;
        }
        let hex: String = chars.clone().take(4).collect();
        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            Some(decoded) if hex.len() == 4 => {
                out.push(decoded);
                for _ in 0..4 {
                    chars.next();
                }
            }
            _ => out.push('~'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_session(path: &Path, lines: &[&str]) {
        use std::io::Write;
        let content = lines.join("\n");
        let compressed = zstd::encode_all(std::io::Cursor::new(content.as_bytes()), 0).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(&compressed).unwrap();
    }

    #[test]
    fn parse_prefers_the_llm_title() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("D--DSH/ses-1/session.jsonl.zstd");
        write_session(
            &file,
            &[
                r#"{"type":"session","id":"ses-1","createdAt":1787023345223,"cwd":"/tmp/project"}"#,
                r#"{"type":"session/title","data":{"title":"Generated Title"}}"#,
                r#"{"type":"user/message","data":{"content":[{"type":"text","text":"hello"}]}}"#,
            ],
        );

        let meta = DshProvider.parse_session(&file).unwrap();
        assert_eq!(meta.provider_id, "dsh");
        assert_eq!(meta.session_id, "ses-1");
        assert_eq!(meta.title.as_deref(), Some("Generated Title"));
        assert_eq!(meta.project_dir.as_deref(), Some("/tmp/project"));
        // The recorded cwd wins over the directory name, as it does for Claude.
        assert_eq!(meta.project_group.as_deref(), Some("/tmp/project"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("dsh-tui --resume ses-1")
        );
    }

    // DSH's own encoding, which Claude's decoder does not understand: `--`
    // wraps the whole path, and every path-unsafe character is `~XXXX`.
    #[test]
    fn decode_dsh_project_expands_its_escapes() {
        assert_eq!(decode_dsh_project("--D-DSH--"), "D:/DSH");
        assert_eq!(
            decode_dsh_project("--C-Users-~9648~5F3A--"),
            "C:/Users/陈强"
        );
        assert_eq!(
            decode_dsh_project("--D-Program~0020Files-nodejs--"),
            "D:/Program Files/nodejs"
        );
        // A literal `-` is indistinguishable from a separator, so it splits.
        assert_eq!(
            decode_dsh_project("--D-~0040deepseek-ai--"),
            "D:/@deepseek/ai"
        );
        // A tilde that is not a code point stays a tilde.
        assert_eq!(decode_dsh_project("--D-a~zz--"), "D:/a~zz");
    }

    #[test]
    fn falls_back_to_the_decoded_directory_without_a_cwd() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("--D-DSH--/ses-3/session.jsonl.zstd");
        write_session(
            &file,
            &[r#"{"type":"session","id":"ses-3","createdAt":1787023345223}"#],
        );

        let meta = DshProvider.parse_session(&file).unwrap();
        assert_eq!(meta.project_dir, None);
        assert_eq!(meta.project_group.as_deref(), Some("D:/DSH"));
    }

    #[test]
    fn parse_falls_back_to_the_first_user_message() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("proj/ses-2/session.jsonl.zstd");
        write_session(
            &file,
            &[
                r#"{"type":"session","id":"ses-2","createdAt":1787023345223}"#,
                r#"{"type":"user/message","data":{"content":[{"type":"text","text":"<system>skip</system>"}]}}"#,
                r#"{"type":"user/message","data":{"content":[{"type":"text","text":"real question"}]}}"#,
            ],
        );

        let meta = DshProvider.parse_session(&file).unwrap();
        assert_eq!(meta.title.as_deref(), Some("real question"));
    }

    #[test]
    fn load_messages_covers_all_three_roles() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("proj/ses-3/session.jsonl.zstd");
        write_session(
            &file,
            &[
                r#"{"type":"session","id":"ses-3","createdAt":1787023345223}"#,
                r#"{"type":"user/message","time":1787023346000,"data":{"content":[{"type":"text","text":"list files"}]}}"#,
                r#"{"type":"assistant/message","time":1787023347000,"data":{"message":{"content":[{"type":"text","text":"Checking"},{"type":"tool-call","name":"Bash"}]}}}"#,
                r#"{"type":"tool/result","time":1787023348000,"data":{"message":{"content":[{"type":"text","text":"a.txt"}]}}}"#,
            ],
        );

        let msgs = DshProvider.load_messages(file.to_str().unwrap()).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("[Tool: Bash]"));
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].content, "a.txt");
        assert_eq!(msgs[0].ts, Some(1787023346000));
    }
}
