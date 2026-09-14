// ── Config Hub: Claude MCP converter ─────────────────────────────────────
//
// Claude's MCP servers live at `~/.claude.json` under `mcpServers`. The spec
// is written almost verbatim; on Windows the CLI commands Claude calls through
// `cmd` need wrapping so `npx ...` becomes `cmd /c npx ...`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::config_hub::registry::provider;
use crate::config_hub::utils::home_dir;

fn mcp_path() -> Option<PathBuf> {
    let home = home_dir()?;
    Some(home.join(".claude.json"))
}

/// Whether Claude is installed enough to write MCP config for it.
fn installed() -> bool {
    provider("claude").is_some_and(|t| t.root().is_some()) || mcp_path().is_some_and(|p| p.exists())
}

/// Commands that are `.cmd` batch files on Windows and need `cmd /c` wrapping.
#[cfg(windows)]
const WRAP_COMMANDS: &[&str] = &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// Windows: turn `npx args...` into `cmd /c npx args...`.
#[cfg(windows)]
fn wrap_command_for_windows(obj: &mut Map<String, Value>) {
    let server_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    if server_type != "stdio" {
        return;
    }
    let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) else {
        return;
    };
    if cmd.eq_ignore_ascii_case("cmd") || cmd.eq_ignore_ascii_case("cmd.exe") {
        return;
    }
    let cmd_name = std::path::Path::new(cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);
    if !WRAP_COMMANDS
        .iter()
        .any(|&c| cmd_name.eq_ignore_ascii_case(c))
    {
        return;
    }
    let original_args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut new_args = vec![Value::String("/c".into()), Value::String(cmd.into())];
    new_args.extend(original_args);
    obj.insert("command".into(), Value::String("cmd".into()));
    obj.insert("args".into(), Value::Array(new_args));
}

#[cfg(not(windows))]
fn wrap_command_for_windows(_obj: &mut Map<String, Value>) {}

/// Strip internal bookkeeping fields before writing into Claude's config.
fn clean_spec(spec: &Value) -> Value {
    let mut obj = match spec.as_object() {
        Some(o) => o.clone(),
        None => return spec.clone(),
    };
    for key in [
        "enabled",
        "source",
        "id",
        "name",
        "description",
        "tags",
        "homepage",
        "docs",
    ] {
        obj.remove(key);
    }
    wrap_command_for_windows(&mut obj);
    Value::Object(obj)
}

fn read_document() -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(mcp_path()?).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.as_object().cloned()
}

fn write_document(map: &Map<String, Value>) -> Result<(), String> {
    let path = mcp_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(map).map_err(|e| format!("Cannot serialize: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write ~/.claude.json: {e}"))
}

/// Write one server into Claude's `mcpServers`.
pub fn sync_single(id: &str, spec: &Value) -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let mut doc = read_document().unwrap_or_default();
    let serving = doc
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "mcpServers is not an object".to_string())?;
    serving.insert(id.to_string(), clean_spec(spec));
    write_document(&doc)
}

/// Remove one server from Claude's `mcpServers`.
pub fn remove(id: &str) -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let mut doc = read_document().unwrap_or_default();
    let Some(mcp_obj) = doc.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return Ok(());
    };
    mcp_obj.remove(id);
    write_document(&doc)
}

/// Read Claude's live `mcpServers` (id → canonical spec), or `None` when Claude
/// has no MCP config to import.
pub fn read_map() -> Result<Option<BTreeMap<String, Value>>, String> {
    let Some(path) = mcp_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Cannot read: {e}"))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let Some(mcp) = doc.get("mcpServers").and_then(|v| v.as_object()) else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (id, entry) in mcp {
        let spec = extract_spec(entry);
        out.insert(id.clone(), spec);
    }
    Ok(Some(out))
}

fn extract_spec(entry: &Value) -> Value {
    // Live entries may be wrapped as `{ enabled, server: {...} }`.
    if let Some(server) = entry.get("server") {
        if server.is_object() {
            return server.clone();
        }
    }
    branch_to_spec(entry.clone())
}

/// Normalize whatever shape Claude stores into the canonical spec.
fn branch_to_spec(entry: Value) -> Value {
    let mut obj = match entry {
        Value::Object(o) => o,
        other => return other,
    };
    obj.remove("enabled");
    for key in [
        "source",
        "id",
        "name",
        "description",
        "tags",
        "homepage",
        "docs",
    ] {
        obj.remove(key);
    }
    Value::Object(obj)
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

    #[test]
    fn sync_single_writes_only_the_mcp_servers_key() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            // Pre-seed ~/.claude.json with unrelated keys, and create ~/.claude
            // so the guard treats Claude as installed.
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            let path = mcp_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"theme":"dark","mcpServers":{"existing":{"command":"keep"}}}"#,
            )
            .unwrap();

            let spec = serde_json::json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "pkg"],
            });
            sync_single("newserver", &spec).unwrap();

            let doc: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(doc["theme"], "dark", "unrelated key preserved");
            assert_eq!(doc["mcpServers"]["existing"]["command"], "keep");
            // npx may be wrapped as `cmd /c npx` on Windows — assert content
            // survives rather than the exact command spelling.
            assert_eq!(doc["mcpServers"]["newserver"]["type"], "stdio");
            let args = doc["mcpServers"]["newserver"]["args"]
                .as_array()
                .expect("args array");
            assert!(args.iter().any(|a| a == "-y"), "args: {args:?}");
        });
    }

    #[test]
    fn sync_single_creates_the_file_when_claude_installed() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            let spec = serde_json::json!({"type": "stdio", "command": "python"});
            sync_single("py", &spec).unwrap();
            let doc: Value =
                serde_json::from_str(&std::fs::read_to_string(mcp_path().unwrap()).unwrap())
                    .unwrap();
            assert_eq!(doc["mcpServers"]["py"]["command"], "python");
        });
    }

    // Not writing to an uninitialized Claude is by design, so an app that is
    // simply not set up yet never gets a stray ~/.claude.json created for it.
    #[test]
    fn sync_single_skips_when_claude_is_not_installed() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let spec = serde_json::json!({"type": "stdio", "command": "python"});
            sync_single("py", &spec).unwrap();
            assert!(
                !mcp_path().unwrap().exists(),
                "no ~/.claude.json created for an uninstalled tool"
            );
        });
    }

    // On Windows, `npx ...` must become `cmd /c npx ...` so Claude can run the
    // `.cmd` shim. `node` and other CLI wrappers get wrapped too.
    #[test]
    fn clean_spec_wraps_known_commands_on_windows() {
        let spec = serde_json::json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "pkg"],
            "enabled": true,
        });
        let cleaned = clean_spec(&spec);
        #[cfg(windows)]
        {
            assert_eq!(cleaned["command"], "cmd");
            assert_eq!(cleaned["args"][0], "/c");
            assert_eq!(cleaned["args"][1], "npx");
            assert_eq!(cleaned.get("enabled"), None, "UI field stripped");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(cleaned["command"], "npx");
        }
    }

    #[test]
    fn clean_spec_wraps_already_wrapped_commands_only_once() {
        #[cfg(windows)]
        {
            let spec = serde_json::json!({
                "type": "stdio",
                "command": "cmd",
                "args": ["/c", "npx", "-y", "pkg"],
            });
            let cleaned = clean_spec(&spec);
            assert_eq!(cleaned["command"], "cmd");
            assert_eq!(cleaned["args"][0], "/c", "no double wrap");
        }
    }

    #[test]
    fn remove_deletes_only_the_target_server() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let path = mcp_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"mcpServers":{"a":{"command":"x"},"b":{"command":"y"}}}"#,
            )
            .unwrap();

            remove("a").unwrap();
            let doc: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(doc["mcpServers"].get("a").is_none());
            assert_eq!(doc["mcpServers"]["b"]["command"], "y");
        });
    }
}
