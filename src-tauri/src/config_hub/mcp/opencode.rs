// ── Config Hub: OpenCode MCP converter ───────────────────────────────────
//
// OpenCode's MCP servers live at `~/.config/opencode/opencode.json` under the
// `mcp` key. The file is JSON5 (it tolerates comments/trailing commas), so it
// is read with `json5` and written back as pretty JSON.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::config_hub::registry::provider;
use crate::config_hub::utils::home_dir;

fn config_path() -> Option<std::path::PathBuf> {
    let home = home_dir()?;
    Some(home.join(".config").join("opencode").join("opencode.json"))
}

fn installed() -> bool {
    provider("opencode").is_some_and(|t| t.root().is_some())
}

fn read_document() -> Result<Option<Value>, String> {
    let Some(path) = config_path() else {
        return Ok(None);
    };
    if !installed() && !path.exists() {
        return Ok(None);
    }
    if !path.exists() {
        return Ok(Some(Value::Object(Map::new())));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("Cannot read opencode.json: {e}"))?;
    if text.trim().is_empty() {
        return Ok(Some(Value::Object(Map::new())));
    }
    // JSON5 parse: opencode.json may contain comments or trailing commas.
    let value = json5::from_str(&text).map_err(|e| format!("Invalid opencode.json: {e}"))?;
    Ok(Some(value))
}

fn write_document(doc: &Value) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(doc).map_err(|e| format!("Cannot serialize: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write opencode.json: {e}"))
}

/// Convert the canonical spec to OpenCode's shape.
fn to_opencode_spec(spec: &Value) -> Result<Value, String> {
    let obj = spec
        .as_object()
        .ok_or_else(|| "MCP spec must be an object".to_string())?;
    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    let mut out = Map::new();

    match typ {
        "stdio" => {
            out.insert("type".into(), json!("local"));
            let cmd = obj.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let mut command_arr = vec![json!(cmd)];
            if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                command_arr.extend(args.clone());
            }
            out.insert("command".into(), Value::Array(command_arr));
            if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
                if !env.is_empty() {
                    out.insert("environment".into(), Value::Object(env.clone()));
                }
            }
        }
        "http" | "sse" => {
            out.insert("type".into(), json!("remote"));
            if let Some(url) = obj.get("url") {
                out.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers").and_then(|v| v.as_object()) {
                if !headers.is_empty() {
                    out.insert("headers".into(), Value::Object(headers.clone()));
                }
            }
        }
        other => return Err(format!("Unsupported MCP server type: {other}")),
    }

    out.insert("enabled".into(), json!(true));
    Ok(Value::Object(out))
}

/// Convert OpenCode's shape back into the canonical spec.
fn from_opencode_spec(spec: &Value) -> Result<Value, String> {
    let obj = spec
        .as_object()
        .ok_or_else(|| "OpenCode MCP spec must be an object".to_string())?;
    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("local");
    let mut out = Map::new();

    match typ {
        "local" => {
            out.insert("type".into(), json!("stdio"));
            if let Some(cmd_arr) = obj.get("command").and_then(|v| v.as_array()) {
                if let Some(cmd) = cmd_arr.first().and_then(|v| v.as_str()) {
                    out.insert("command".into(), json!(cmd));
                }
                if cmd_arr.len() > 1 {
                    out.insert("args".into(), Value::Array(cmd_arr[1..].to_vec()));
                }
            }
            if let Some(env) = obj.get("environment").and_then(|v| v.as_object()) {
                if !env.is_empty() {
                    out.insert("env".into(), Value::Object(env.clone()));
                }
            }
        }
        "remote" => {
            // OpenCode doesn't distinguish http from sse; default to sse.
            out.insert("type".into(), json!("sse"));
            if let Some(url) = obj.get("url") {
                out.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers").and_then(|v| v.as_object()) {
                if !headers.is_empty() {
                    out.insert("headers".into(), Value::Object(headers.clone()));
                }
            }
        }
        other => return Err(format!("Unsupported OpenCode MCP type: {other}")),
    }

    Ok(Value::Object(out))
}

fn mcp_map(doc: &Value) -> Option<&Map<String, Value>> {
    doc.get("mcp").and_then(|v| v.as_object())
}

fn mcp_map_mut(doc: &mut Value) -> Option<&mut Map<String, Value>> {
    doc.get_mut("mcp").and_then(|v| v.as_object_mut())
}

/// Write one server into OpenCode's `mcp` key.
pub fn sync_single(id: &str, spec: &Value) -> Result<(), String> {
    let mut doc = read_document()?.unwrap_or_else(|| Value::Object(Map::new()));
    // Ensure `mcp` exists before taking a second borrow for mutation.
    if doc.get("mcp").and_then(|v| v.as_object()).is_none() {
        let obj = doc
            .as_object_mut()
            .ok_or_else(|| "document is not an object".to_string())?;
        obj.insert("mcp".into(), Value::Object(Map::new()));
    }
    let serving = doc
        .get_mut("mcp")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| "mcp is not an object".to_string())?;
    serving.insert(id.to_string(), to_opencode_spec(spec)?);
    write_document(&doc)
}

/// Remove one server from OpenCode's `mcp` key.
pub fn remove(id: &str) -> Result<(), String> {
    let Some(mut doc) = read_document()? else {
        return Ok(());
    };
    let Some(mcp_obj) = mcp_map_mut(&mut doc) else {
        return Ok(());
    };
    mcp_obj.remove(id);
    write_document(&doc)
}

/// Read OpenCode's live `mcp` map (id → canonical spec).
pub fn read_map() -> Result<Option<BTreeMap<String, Value>>, String> {
    let Some(doc) = read_document()? else {
        return Ok(None);
    };
    let Some(mcp) = mcp_map(&doc) else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (id, entry) in mcp {
        if entry.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        if let Ok(spec) = from_opencode_spec(entry) {
            out.insert(id.clone(), spec);
        }
    }
    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::parser::opencode::opencode_env_lock;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        opencode_env_lock()
    }

    fn with_home(home: &std::path::Path, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::create_dir_all(home);
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", home);
        f();
        match original {
            Some(value) => {
                #[allow(deprecated)]
                std::env::set_var("HOME", value);
            }
            None => {
                #[allow(deprecated)]
                std::env::remove_var("HOME");
            }
        }
    }

    fn setup(home: &std::path::Path) {
        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
    }

    #[test]
    fn sync_single_writes_local_format_with_environment() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let spec = serde_json::json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            });
            sync_single("fs", &spec).unwrap();

            let doc: Value =
                json5::from_str(&std::fs::read_to_string(config_path().unwrap()).unwrap()).unwrap();
            let mcp = &doc["mcp"]["fs"];
            assert_eq!(mcp["type"], "local");
            assert_eq!(mcp["command"][0], "npx");
            assert_eq!(mcp["command"][2], "@modelcontextprotocol/server-filesystem");
            assert_eq!(mcp["enabled"], true);
        });
    }

    #[test]
    fn sync_single_writes_remote_format() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let spec = serde_json::json!({
                "type": "http",
                "url": "https://mcp.example.com/mcp",
                "headers": {"Authorization": "Bearer x"},
            });
            sync_single("remote-srv", &spec).unwrap();

            let doc: Value =
                json5::from_str(&std::fs::read_to_string(config_path().unwrap()).unwrap()).unwrap();
            let mcp = &doc["mcp"]["remote-srv"];
            assert_eq!(mcp["type"], "remote");
            assert_eq!(mcp["url"], "https://mcp.example.com/mcp");
            assert_eq!(mcp["headers"]["Authorization"], "Bearer x");
        });
    }

    #[test]
    fn json5_comments_survive_the_read() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            // opencode.json is JSON5: comments and trailing commas allowed.
            std::fs::write(
                config_path().unwrap(),
                "{\n  // a comment\n  \"mcp\": {\n    \"exists\": { \"type\": \"local\", \"command\": [\"keep\"], \"enabled\": true },\n  },\n}\n",
            )
            .unwrap();

            sync_single("new", &json!({"type": "stdio", "command": "python"})).unwrap();
            let doc: Value =
                json5::from_str(&std::fs::read_to_string(config_path().unwrap()).unwrap()).unwrap();
            assert_eq!(
                doc["mcp"]["exists"]["command"][0], "keep",
                "unrelated mcp kept"
            );
            assert_eq!(doc["mcp"]["new"]["command"][0], "python");
        });
    }

    #[test]
    fn read_map_converts_local_and_remote_back_to_canonical() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::write(
                config_path().unwrap(),
                r#"{
  "mcp": {
    "local-srv": { "type": "local", "command": ["npx", "-y", "pkg"], "enabled": true },
    "remote-srv": { "type": "remote", "url": "https://x/mcp", "enabled": true },
    "disabled": { "type": "local", "command": ["x"], "enabled": false }
  }
}"#,
            )
            .unwrap();

            let map = read_map().unwrap().unwrap();
            let local = &map["local-srv"];
            assert_eq!(local["type"], "stdio");
            assert_eq!(local["command"], "npx");
            assert_eq!(local["args"][0], "-y");
            assert_eq!(map["remote-srv"]["type"], "sse");
            assert!(!map.contains_key("disabled"), "disabled servers skipped");
        });
    }

    #[test]
    fn remove_deletes_only_the_target() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            sync_single("a", &json!({"type": "stdio", "command": "x"})).unwrap();
            sync_single("b", &json!({"type": "stdio", "command": "y"})).unwrap();
            remove("a").unwrap();

            let map = read_map().unwrap().unwrap();
            assert!(!map.contains_key("a"));
            assert!(map.contains_key("b"));
        });
    }
}
