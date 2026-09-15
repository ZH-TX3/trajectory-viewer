// ── Config Hub: MCP server management ────────────────────────────────────
//
// MCP servers are configuration entries, not linkable directories, so they
// can't be soft-linked the way skills are. Instead there is one unified store
// — `~/.trajectory-viewer/mcp-servers.json` — and each server is *merged*
// into the live config of the tools that have it enabled:
//
//   claude   → `~/.claude.json`          `mcpServers.<id>`
//   codex    → `~/.codex/config.toml`    `[mcp_servers.<id>]`
//   opencode → `~/.config/opencode/opencode.json`  `mcp.<id>`
//
// Writing is read-modify-write: only our entry changes, everything else in the
// tool's file survives.

pub mod claude;
pub mod codex;
pub mod opencode;
pub mod test;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_hub::utils::home_dir;

/// Tools that have a live MCP config this hub writes into.
pub const MCP_APPS: [&str; 3] = ["claude", "codex", "opencode"];

/// One MCP server in the unified store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Canonical spec: `type` (stdio/http/sse), plus `command`/`args`/`env`/
    /// `cwd` for stdio or `url`/`headers` for http/sse.
    pub server: Value,
    /// Per-tool enablement (`claude` / `codex` / `opencode`).
    pub apps: BTreeMap<String, bool>,
}

impl McpServer {
    pub fn enabled_apps(&self) -> Vec<String> {
        MCP_APPS
            .iter()
            .filter(|app| self.apps.get(**app).copied().unwrap_or(false))
            .map(|a| a.to_string())
            .collect()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpStore {
    #[serde(default)]
    servers: Vec<McpServer>,
}

fn store_path() -> Option<PathBuf> {
    Some(
        home_dir()?
            .join(".trajectory-viewer")
            .join("mcp-servers.json"),
    )
}

fn load_store() -> Result<McpStore, String> {
    let Some(path) = store_path() else {
        return Err("Cannot locate home directory".to_string());
    };
    if !path.exists() {
        return Ok(McpStore::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("Cannot read MCP store: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid MCP store: {e}"))
}

fn save_store(store: &McpStore) -> Result<(), String> {
    let path = store_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create store dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Cannot serialize MCP store: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write MCP store: {e}"))
}

/// Validate the canonical server spec: stdio/http/sse with required fields.
pub fn validate_spec(server: &Value) -> Result<(), String> {
    let obj = server
        .as_object()
        .ok_or_else(|| "MCP server spec must be a JSON object".to_string())?;
    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    match typ {
        "stdio" => {
            let cmd = obj.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.trim().is_empty() {
                return Err("stdio MCP server requires a command".to_string());
            }
        }
        "http" | "sse" => {
            let url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.trim().is_empty() {
                return Err(format!("{typ} MCP server requires a url"));
            }
        }
        other => return Err(format!("Unsupported MCP server type: {other}")),
    }
    Ok(())
}

/// All servers in the unified store.
pub fn list() -> Vec<McpServer> {
    load_store().map(|s| s.servers).unwrap_or_default()
}

fn upsert_app_bools(apps: &mut BTreeMap<String, bool>) {
    for app in MCP_APPS {
        apps.entry(app.to_string()).or_insert(false);
    }
}

/// Create or update a server: store it, then reconcile every tool's live
/// config to match the new app flags (write enabled ones, remove disabled).
pub fn upsert(
    id: String,
    name: String,
    description: Option<String>,
    server: Value,
    apps: BTreeMap<String, bool>,
) -> Result<(), String> {
    validate_spec(&server)?;
    if id.trim().is_empty() || id == "." || id == ".." {
        return Err("Invalid server id".to_string());
    }

    let mut store = load_store()?;
    let prev = store
        .servers
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .unwrap_or_else(|| McpServer {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            server: server.clone(),
            apps: BTreeMap::new(),
        });

    let mut apps = apps;
    upsert_app_bools(&mut apps);
    store.servers.retain(|s| s.id != id);
    store.servers.push(McpServer {
        id,
        name,
        description,
        server,
        apps: apps.clone(),
    });
    save_store(&store)?;

    // Reconcile live configs: enabled → write, disabled → remove.
    for app in MCP_APPS {
        let wanted = apps.get(app).copied().unwrap_or(false);
        let had = prev.apps.get(app).copied().unwrap_or(false);
        match (wanted, had) {
            (true, _) => {
                let server = store
                    .servers
                    .iter()
                    .find(|s| s.id == prev.id)
                    .map(|s| s.server.clone())
                    .ok_or_else(|| "MCP server not found".to_string())?;
                sync_single(app, &prev.id, &server)?;
            }
            (false, true) => {
                remove_from_live(app, &prev.id)?;
            }
            (false, false) => {}
        }
    }
    Ok(())
}

/// Toggle one tool's flag and sync the live config accordingly.
pub fn toggle_app(id: &str, app: &str, enabled: bool) -> Result<(), String> {
    if !MCP_APPS.contains(&app) {
        return Err(format!("MCP is not supported for {app}"));
    }
    let mut store = load_store()?;
    let server = store
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("MCP server not found: {id}"))?;
    server.apps.insert(app.to_string(), enabled);
    let spec = server.server.clone();
    save_store(&store)?;

    if enabled {
        sync_single(app, id, &spec)
    } else {
        remove_from_live(app, id)
    }
}

/// Remove a server from the store and from every tool that had it enabled.
pub fn delete(id: &str) -> Result<(), String> {
    let mut store = load_store()?;
    let Some(server) = store.servers.iter().find(|s| s.id == id).cloned() else {
        return Err(format!("MCP server not found: {id}"));
    };
    store.servers.retain(|s| s.id != id);
    save_store(&store)?;

    for app in server.enabled_apps() {
        let _ = remove_from_live(&app, id);
    }
    Ok(())
}

/// Read each installed tool's live MCP config and merge it into the store.
/// Existing servers only gain the tool flag; their spec is not overwritten.
pub fn import_from_apps() -> Result<usize, String> {
    let mut store = load_store()?;
    let mut changed = 0;

    if let Ok(Some(servers)) = claude::read_map() {
        for (id, spec) in servers {
            changed += merge_imported(&mut store, id, spec, "claude")?;
        }
    }
    if let Ok(Some(servers)) = codex::read_map() {
        for (id, spec) in servers {
            changed += merge_imported(&mut store, id, spec, "codex")?;
        }
    }
    if let Ok(Some(servers)) = opencode::read_map() {
        for (id, spec) in servers {
            changed += merge_imported(&mut store, id, spec, "opencode")?;
        }
    }

    save_store(&store)?;
    Ok(changed)
}

fn merge_imported(
    store: &mut McpStore,
    id: String,
    spec: Value,
    app: &str,
) -> Result<usize, String> {
    if validate_spec(&spec).is_err() {
        return Ok(0);
    }
    let Some(server) = store.servers.iter_mut().find(|s| s.id == id) else {
        store.servers.push(McpServer {
            id: id.clone(),
            name: id.clone(),
            description: None,
            server: spec,
            apps: BTreeMap::from([(app.to_string(), true)]),
        });
        return Ok(1);
    };
    upsert_app_bools(&mut server.apps);
    if !server.apps.get(app).copied().unwrap_or(false) {
        server.apps.insert(app.to_string(), true);
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Push one server into one tool's live config.
fn sync_single(app: &str, id: &str, spec: &Value) -> Result<(), String> {
    match app {
        "claude" => claude::sync_single(id, spec),
        "codex" => codex::sync_single(id, spec),
        "opencode" => opencode::sync_single(id, spec),
        other => Err(format!("MCP is not supported for {other}")),
    }
}

/// Remove one server from one tool's live config.
fn remove_from_live(app: &str, id: &str) -> Result<(), String> {
    match app {
        "claude" => claude::remove(id),
        "codex" => codex::remove(id),
        "opencode" => opencode::remove(id),
        other => Err(format!("MCP is not supported for {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::parser::opencode::opencode_env_lock;
    use serde_json::json;
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

    fn stdio_spec(command: &str, args: &[&str]) -> Value {
        json!({
            "type": "stdio",
            "command": command,
            "args": args,
        })
    }

    fn apps(claude: bool, codex: bool, opencode: bool) -> BTreeMap<String, bool> {
        BTreeMap::from([
            ("claude".into(), claude),
            ("codex".into(), codex),
            ("opencode".into(), opencode),
        ])
    }

    #[test]
    fn validate_spec_accepts_stdio_and_http() {
        assert!(validate_spec(&stdio_spec("npx", &["-y", "pkg"])).is_ok());
        assert!(validate_spec(&json!({"type": "http", "url": "https://x"})).is_ok());
        assert!(validate_spec(&json!({"type": "sse", "url": "https://x"})).is_ok());

        // stdio with no command → rejected.
        assert!(validate_spec(&json!({"type": "stdio"})).is_err());
        // http with no url → rejected.
        assert!(validate_spec(&json!({"type": "http"})).is_err());
        // unknown type → rejected.
        assert!(validate_spec(&json!({"type": "bogus", "command": "x"})).is_err());
    }

    #[test]
    fn upsert_persists_to_the_store() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            upsert(
                "time".to_string(),
                "time".to_string(),
                None,
                stdio_spec("npx", &["-y", "@modelcontextprotocol/server-time"]),
                apps(true, false, false),
            )
            .unwrap();

            let servers = list();
            assert_eq!(servers.len(), 1);
            assert_eq!(servers[0].id, "time");
            assert_eq!(servers[0].server["command"], "npx");
            assert!(servers[0].apps["claude"]);
        });
    }

    #[test]
    fn upsert_rejects_an_invalid_spec_before_persisting() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let err = upsert(
                "bad".to_string(),
                "bad".to_string(),
                None,
                json!({"type": "stdio"}),
                apps(true, false, false),
            )
            .unwrap_err();
            assert!(err.contains("command"), "got: {err}");
            assert!(list().is_empty(), "nothing persisted");
        });
    }

    #[test]
    fn toggle_inverts_the_app_flag() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            upsert(
                "time".to_string(),
                "time".to_string(),
                None,
                stdio_spec("npx", &[]),
                apps(true, false, false),
            )
            .unwrap();
            assert!(list()[0].apps["claude"]);

            toggle_app("time", "claude", false).unwrap();
            assert!(!list()[0].apps["claude"]);

            toggle_app("time", "claude", true).unwrap();
            assert!(list()[0].apps["claude"]);
        });
    }

    #[test]
    fn delete_removes_the_server_entirely() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            upsert(
                "time".to_string(),
                "time".to_string(),
                None,
                stdio_spec("npx", &[]),
                apps(true, true, true),
            )
            .unwrap();
            delete("time").unwrap();
            assert!(list().is_empty());
        });
    }

    #[test]
    fn delete_and_toggle_reject_unknown_ids() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            assert!(delete("ghost").is_err());
            assert!(toggle_app("ghost", "claude", true).is_err());
        });
    }
}
