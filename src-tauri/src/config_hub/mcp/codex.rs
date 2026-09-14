// ── Config Hub: Codex MCP converter ──────────────────────────────────────
//
// Codex's MCP servers live at `~/.codex/config.toml` under the top-level
// `[mcp_servers.<id>]` table (the official format). The file is edited with
// `toml_edit` so comments and unrelated keys survive. A mistaken legacy
// `[mcp.servers]` layout is migrated out on write.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, Item, Table};

use crate::config_hub::registry::provider;
use crate::config_hub::utils::home_dir;

fn config_path() -> Option<std::path::PathBuf> {
    let home = home_dir()?;
    Some(home.join(".codex").join("config.toml"))
}

fn installed() -> bool {
    provider("codex").is_some_and(|t| t.root().is_some())
}

fn read_document() -> Result<Option<DocumentMut>, String> {
    let Some(path) = config_path() else {
        return Ok(None);
    };
    if !installed() && !path.exists() {
        return Ok(None);
    }
    if !path.exists() {
        return Ok(Some(DocumentMut::new()));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("Cannot read config.toml: {e}"))?;
    if text.trim().is_empty() {
        return Ok(Some(DocumentMut::new()));
    }
    text.parse::<DocumentMut>()
        .map(Some)
        .map_err(|e| format!("Invalid config.toml: {e}"))
}

fn write_document(doc: &DocumentMut) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create dir: {e}"))?;
    }
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("Cannot write config.toml: {e}"))
}

/// Drop the mistakenly-written `[mcp.servers]` block if present.
fn drop_legacy_mcp_servers(doc: &mut DocumentMut) {
    let Some(mcp) = doc.get_mut("mcp").and_then(|t| t.as_table_like_mut()) else {
        return;
    };
    if mcp.contains_key("servers") {
        mcp.remove("servers");
    }
}

/// Write one server into `[mcp_servers.<id>]`, cleaning the legacy block.
pub fn sync_single(id: &str, spec: &Value) -> Result<(), String> {
    let mut doc = read_document()?.unwrap_or_default();
    drop_legacy_mcp_servers(&mut doc);
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }
    doc["mcp_servers"][id] = Item::Table(json_to_toml_table(spec)?);
    write_document(&doc)
}

/// Remove one server from `[mcp_servers]` (and the legacy block).
pub fn remove(id: &str) -> Result<(), String> {
    let Some(mut doc) = read_document()? else {
        return Ok(());
    };
    let changed = {
        let mut changed = false;
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) {
            if servers.remove(id).is_some() {
                changed = true;
            }
        }
        if let Some(servers) = doc
            .get_mut("mcp")
            .and_then(|t| t.as_table_mut())
            .and_then(|t| t.get_mut("servers"))
            .and_then(|t| t.as_table_mut())
        {
            if servers.remove(id).is_some() {
                changed = true;
            }
        }
        changed
    };
    if changed {
        write_document(&doc)?;
    }
    Ok(())
}

/// Read Codex's live `[mcp_servers]` (id → canonical spec).
pub fn read_map() -> Result<Option<BTreeMap<String, Value>>, String> {
    let Some(doc) = read_document()? else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();

    let collect = |tbl: &Table, out: &mut BTreeMap<String, Value>| {
        for (id, entry) in tbl.iter() {
            let Some(entry_tbl) = entry.as_table() else {
                continue;
            };
            if let Ok(spec) = toml_table_to_json(entry_tbl, id) {
                out.insert(id.to_string(), spec);
            }
        }
    };

    if let Some(servers) = doc.get("mcp_servers").and_then(|v| v.as_table()) {
        collect(servers, &mut out);
    }
    // Legacy `[mcp.servers]` fallback.
    if let Some(servers) = doc
        .get("mcp")
        .and_then(|v| v.as_table())
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_table())
    {
        collect(servers, &mut out);
    }

    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

/// Convert the canonical JSON spec into a `toml_edit::Table`.
fn json_to_toml_table(spec: &Value) -> Result<Table, String> {
    let obj = spec
        .as_object()
        .ok_or_else(|| "MCP spec must be an object".to_string())?;
    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");

    let mut t = Table::new();
    t["type"] = toml_edit::value(typ);

    match typ {
        "stdio" => {
            let cmd = obj.get("command").and_then(|v| v.as_str()).unwrap_or("");
            t["command"] = toml_edit::value(cmd);
            if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                let mut arr = toml_edit::Array::default();
                for a in args.iter().filter_map(|v| v.as_str()) {
                    arr.push(a);
                }
                if !arr.is_empty() {
                    t["args"] = Item::Value(toml_edit::Value::Array(arr));
                }
            }
            if let Some(cwd) = obj.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    t["cwd"] = toml_edit::value(cwd);
                }
            }
            if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
                let mut env_tbl = Table::new();
                for (k, v) in env.iter() {
                    if let Some(s) = v.as_str() {
                        env_tbl[k.as_str()] = toml_edit::value(s);
                    }
                }
                if !env_tbl.is_empty() {
                    t["env"] = Item::Table(env_tbl);
                }
            }
        }
        "http" | "sse" => {
            let url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("");
            t["url"] = toml_edit::value(url);
            if let Some(headers) = obj.get("headers").and_then(|v| v.as_object()) {
                let mut h_tbl = Table::new();
                for (k, v) in headers.iter() {
                    if let Some(s) = v.as_str() {
                        h_tbl[k.as_str()] = toml_edit::value(s);
                    }
                }
                if !h_tbl.is_empty() {
                    t["http_headers"] = Item::Table(h_tbl);
                }
            }
        }
        _ => {}
    }

    // Pass through any unknown scalar fields (timeout, etc.).
    for (key, value) in obj {
        if ["type", "command", "args", "env", "cwd", "url", "headers"].contains(&key.as_str()) {
            continue;
        }
        if let Some(item) = json_value_to_toml(value) {
            t[key.as_str()] = item;
        }
    }

    Ok(t)
}

/// Best-effort generic JSON → TOML converter (strings, numbers, bools,
/// simple arrays, string-only maps).
fn json_value_to_toml(value: &Value) -> Option<Item> {
    match value {
        Value::String(s) => Some(toml_edit::value(s.as_str())),
        Value::Number(n) => n
            .as_i64()
            .map(toml_edit::value)
            .or_else(|| n.as_f64().map(toml_edit::value)),
        Value::Bool(b) => Some(toml_edit::value(*b)),
        Value::Array(arr) => {
            let mut out = toml_edit::Array::default();
            for item in arr {
                match item {
                    Value::String(s) => out.push(s.as_str()),
                    Value::Number(n) if n.is_i64() => n.as_i64().map(|i| out.push(i))?,
                    Value::Number(n) => n.as_f64().map(|f| out.push(f))?,
                    Value::Bool(b) => out.push(*b),
                    _ => return None,
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(Item::Value(toml_edit::Value::Array(out)))
            }
        }
        Value::Object(o) => {
            let mut out = toml_edit::InlineTable::new();
            for (k, v) in o.iter() {
                let Value::String(s) = v else {
                    return None;
                };
                out.insert(k, s.into());
            }
            if out.is_empty() {
                None
            } else {
                Some(Item::Value(toml_edit::Value::InlineTable(out)))
            }
        }
        Value::Null => None,
    }
}

/// Convert one `[mcp_servers.<id>]` table into the canonical JSON spec.
fn toml_table_to_json(entry: &Table, _id: &str) -> Result<Value, String> {
    let typ = entry
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio");
    let mut spec = Map::new();
    spec.insert("type".into(), json!(typ));

    match typ {
        "stdio" => {
            if let Some(cmd) = entry.get("command").and_then(|v| v.as_str()) {
                spec.insert("command".into(), json!(cmd));
            }
            if let Some(args) = entry.get("args").and_then(|v| v.as_array()) {
                let mut arr: Vec<Value> = Vec::new();
                for a in args.iter() {
                    if let Some(s) = a.as_str() {
                        arr.push(json!(s));
                    }
                }
                if !arr.is_empty() {
                    spec.insert("args".into(), Value::Array(arr));
                }
            }
            if let Some(cwd) = entry.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    spec.insert("cwd".into(), json!(cwd));
                }
            }
            if let Some(env) = entry.get("env").and_then(|v| v.as_table()) {
                let mut obj = Map::new();
                for (k, v) in env.iter() {
                    if let Some(s) = v.as_str() {
                        obj.insert(k.to_string(), json!(s));
                    }
                }
                if !obj.is_empty() {
                    spec.insert("env".into(), Value::Object(obj));
                }
            }
        }
        "http" | "sse" => {
            if let Some(url) = entry.get("url").and_then(|v| v.as_str()) {
                spec.insert("url".into(), json!(url));
            }
            let headers = entry
                .get("http_headers")
                .and_then(|v| v.as_table())
                .or_else(|| entry.get("headers").and_then(|v| v.as_table()));
            if let Some(headers) = headers {
                let mut obj = Map::new();
                for (k, v) in headers.iter() {
                    if let Some(s) = v.as_str() {
                        obj.insert(k.to_string(), json!(s));
                    }
                }
                if !obj.is_empty() {
                    spec.insert("headers".into(), Value::Object(obj));
                }
            }
        }
        _ => {}
    }

    // Pass through any remaining scalar fields.
    for (key, val) in entry.iter() {
        if matches!(
            key,
            "type" | "command" | "args" | "env" | "cwd" | "url" | "http_headers" | "headers"
        ) {
            continue;
        }
        if let Some(v) = toml_item_to_json(val) {
            spec.insert(key.to_string(), v);
        }
    }

    Ok(Value::Object(spec))
}

/// Best-effort generic TOML → JSON converter.
fn toml_item_to_json(item: &toml_edit::Item) -> Option<Value> {
    match item.as_value()? {
        toml_edit::Value::String(s) => Some(json!(s.value())),
        toml_edit::Value::Integer(i) => Some(json!(i.value())),
        toml_edit::Value::Float(f) => Some(json!(f.value())),
        toml_edit::Value::Boolean(b) => Some(json!(b.value())),
        toml_edit::Value::Array(a) => {
            let arr: Vec<Value> = a.iter().filter_map(toml_value_to_json).collect();
            if arr.is_empty() {
                None
            } else {
                Some(Value::Array(arr))
            }
        }
        toml_edit::Value::InlineTable(t) => {
            let mut obj = Map::new();
            for (k, v) in t.iter() {
                let toml_edit::Value::String(s) = v else {
                    return None;
                };
                obj.insert(k.to_string(), json!(s.value()));
            }
            if obj.is_empty() {
                None
            } else {
                Some(Value::Object(obj))
            }
        }
        _ => None,
    }
}

/// Best-effort generic TOML value → JSON converter.
fn toml_value_to_json(value: &toml_edit::Value) -> Option<Value> {
    match value {
        toml_edit::Value::String(s) => Some(json!(s.value())),
        toml_edit::Value::Integer(i) => Some(json!(i.value())),
        toml_edit::Value::Float(f) => Some(json!(f.value())),
        toml_edit::Value::Boolean(b) => Some(json!(b.value())),
        _ => None,
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
        // ~/.codex must exist for the tool to count as installed.
        std::fs::create_dir_all(home.join(".codex")).unwrap();
    }

    #[test]
    fn sync_single_writes_toml_and_preserves_unrelated_keys() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::write(
                home.join(".codex/config.toml"),
                "model = \"gpt-5\"\n\n# keep my comment\nother = 1\n",
            )
            .unwrap();

            let spec = serde_json::json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-time"],
            });
            sync_single("time", &spec).unwrap();

            let text = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
            assert!(text.contains("model = \"gpt-5\""), "unrelated key kept");
            assert!(text.contains("# keep my comment"), "comment kept");
            assert!(text.contains("other = 1"), "unrelated key kept");

            let servers = read_map().unwrap().unwrap();
            let s = &servers["time"];
            assert_eq!(s["type"], "stdio");
            assert_eq!(s["command"], "npx");
            assert_eq!(s["args"][0], "-y");
        });
    }

    #[test]
    fn json_to_toml_roundtrips_env_as_a_subtable() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let spec = serde_json::json!({
                "type": "stdio",
                "command": "uvx",
                "env": {"API_KEY": "sk-123"},
            });
            sync_single("srv", &spec).unwrap();

            let text = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
            assert!(text.contains("API_KEY = \"sk-123\""), "got:\n{text}");
            let servers = read_map().unwrap().unwrap();
            assert_eq!(servers["srv"]["env"]["API_KEY"], "sk-123");
        });
    }

    #[test]
    fn remove_deletes_only_the_target_server() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            sync_single("a", &json!({"type": "stdio", "command": "x"})).unwrap();
            sync_single("b", &json!({"type": "stdio", "command": "y"})).unwrap();
            remove("a").unwrap();

            let servers = read_map().unwrap().unwrap();
            assert!(!servers.contains_key("a"));
            assert!(servers.contains_key("b"));
        });
    }

    // The official Codex format is top-level `[mcp_servers]`; reading a legacy
    // `[mcp.servers]` layout still finds the servers.
    #[test]
    fn read_map_falls_back_to_legacy_mcp_servers() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::write(
                home.join(".codex/config.toml"),
                "[mcp.servers.legacy]\ncommand = \"python\"\ntype = \"stdio\"\n",
            )
            .unwrap();
            let servers = read_map().unwrap().unwrap();
            assert_eq!(servers["legacy"]["command"], "python");
        });
    }
}
