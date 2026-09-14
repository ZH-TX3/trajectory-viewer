// ── Config Hub ───────────────────────────────────────────────────────────
//
// Unified management of the configuration each AI CLI keeps in its own
// directory: skills and agents are linked from the `~/.agents/` single source
// of truth into every installed tool, so one edit reaches all of them.
//
// Layout mirrors `session_manager`: one file per tool implementing
// `ToolProvider`, plus shared modules for scanning, syncing and migration.
//
// Adding a tool:
//   1. create `config_hub/<tool>.rs` with a `ToolProvider` impl
//   2. register it in `registry::providers()`
//   3. add a `mod` line below

pub mod claude;
pub mod codex;
pub mod dsh;
pub mod mcp;
pub mod migrate;
pub mod opencode;
pub mod registry;
pub mod resource;
pub mod scan;
pub mod state;
pub mod sync;
pub mod utils;

use serde::Serialize;

use registry::{providers, ConfigFormat};
use resource::{Resource, ResourceKind};

/// One tool as the UI needs it: id, label, installed flag, capabilities.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub id: String,
    pub display_name: String,
    pub installed: bool,
    /// Resource kinds this tool can link (e.g. `["skill", "agent"]`).
    pub linkable_kinds: Vec<String>,
    pub has_prompt: bool,
    pub has_mcp: bool,
}

/// Every registered tool, installed or not, for the UI's tool list.
pub fn tool_infos() -> Vec<ToolInfo> {
    providers()
        .iter()
        .map(|tool| {
            let installed = tool.root().is_some();
            let linkable_kinds = if installed {
                tool.linkable_dirs()
                    .into_iter()
                    .map(|t| t.kind.as_str().to_string())
                    .collect()
            } else {
                Vec::new()
            };
            ToolInfo {
                id: tool.id().to_string(),
                display_name: tool.display_name().to_string(),
                installed,
                linkable_kinds,
                has_prompt: installed && tool.prompt_file().is_some(),
                has_mcp: installed && tool.mcp_file().is_some(),
            }
        })
        .collect()
}

/// The whole hub state in one call: tools, resources, and the read-only
/// config files each installed tool exposes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSnapshot {
    pub tools: Vec<ToolInfo>,
    pub resources: Vec<Resource>,
    pub configs: Vec<ToolConfigs>,
}

/// Read-only configuration shown for one tool.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigs {
    pub tool_id: String,
    pub display_name: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ConfigFileContent>,
    pub files: Vec<ConfigFileContent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileContent {
    pub label: String,
    pub path: String,
    pub format: ConfigFormat,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Full read of the hub.
pub fn snapshot() -> HubSnapshot {
    HubSnapshot {
        tools: tool_infos(),
        resources: scan::scan_resources(),
        configs: config_files(),
    }
}

/// Read-only view of every tool's prompt and config files.
pub fn config_files() -> Vec<ToolConfigs> {
    providers()
        .iter()
        .map(|tool| {
            let installed = tool.root().is_some();
            let prompt = tool
                .prompt_file()
                .map(|p| read_config("prompt", p, ConfigFormat::Markdown));
            let files = tool
                .config_files()
                .into_iter()
                .map(|f| read_config(&f.label, std::path::PathBuf::from(&f.path), f.format))
                .collect();
            ToolConfigs {
                tool_id: tool.id().to_string(),
                display_name: tool.display_name().to_string(),
                installed,
                prompt,
                files,
            }
        })
        .collect()
}

fn read_config(label: &str, path: std::path::PathBuf, format: ConfigFormat) -> ConfigFileContent {
    let content = std::fs::read_to_string(&path).ok();
    ConfigFileContent {
        label: label.to_string(),
        path: path.to_string_lossy().into_owned(),
        format,
        exists: content.is_some(),
        content,
    }
}

/// Parse a `<kind>:<name>` resource id.
pub fn split_id(id: &str) -> Result<(ResourceKind, String), String> {
    let (kind, name) = id
        .split_once(':')
        .ok_or_else(|| format!("Malformed resource id: {id}"))?;
    let kind = ResourceKind::parse(kind).ok_or_else(|| format!("Unknown resource kind: {kind}"))?;
    if name.is_empty() {
        return Err(format!("Malformed resource id: {id}"));
    }
    Ok((kind, name.to_string()))
}

// ── Tauri commands ───────────────────────────────────────────────────────

#[tauri::command]
pub fn config_hub_snapshot() -> HubSnapshot {
    snapshot()
}

#[tauri::command]
pub fn config_hub_toggle(
    id: String,
    tool_id: String,
    enabled: bool,
    method: Option<String>,
) -> Result<(), String> {
    let (kind, name) = split_id(&id)?;
    if enabled {
        let method = method
            .as_deref()
            .map(sync::SyncMethod::parse)
            .unwrap_or(sync::SyncMethod::Auto);
        sync::enable(kind, &name, &tool_id, method).map(|_| ())
    } else {
        sync::disable(kind, &name, &tool_id)
    }
}

#[tauri::command]
pub fn config_hub_import_preview(
    id: String,
    tool_id: String,
) -> Result<migrate::ImportPreview, String> {
    let (kind, name) = split_id(&id)?;
    migrate::preview(kind, &name, &tool_id)
}

#[tauri::command]
pub fn config_hub_import(id: String, tool_id: String, overwrite: bool) -> Result<(), String> {
    let (kind, name) = split_id(&id)?;
    migrate::import(kind, &name, &tool_id, overwrite)
}

#[tauri::command]
pub fn config_hub_undo_import(id: String, tool_id: String) -> Result<(), String> {
    let (kind, name) = split_id(&id)?;
    migrate::undo_import(kind, &name, &tool_id)
}

#[tauri::command]
pub fn config_hub_delete(id: String) -> Result<String, String> {
    let (kind, name) = split_id(&id)?;
    migrate::delete_resource(kind, &name)
}

// ── MCP commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn config_hub_mcp_list() -> Vec<mcp::McpServer> {
    mcp::list()
}

#[tauri::command]
pub fn config_hub_mcp_upsert(
    id: String,
    name: String,
    description: Option<String>,
    server: serde_json::Value,
    apps: std::collections::BTreeMap<String, bool>,
) -> Result<(), String> {
    mcp::upsert(id, name, description, server, apps)
}

#[tauri::command]
pub fn config_hub_mcp_toggle(id: String, tool_id: String, enabled: bool) -> Result<(), String> {
    mcp::toggle_app(&id, &tool_id, enabled)
}

#[tauri::command]
pub fn config_hub_mcp_delete(id: String) -> Result<(), String> {
    mcp::delete(&id)
}

#[tauri::command]
pub fn config_hub_mcp_import() -> Result<usize, String> {
    mcp::import_from_apps()
}

#[cfg(test)]
mod tests {
    use super::registry::{provider, provider_ids};
    use super::resource::AppState;
    use super::*;

    #[test]
    fn every_registered_tool_has_a_unique_id() {
        let ids = provider_ids();
        for expected in ["claude", "codex", "dsh", "opencode"] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "tool ids must be unique");
    }

    #[test]
    fn tool_lookup_finds_registered_and_rejects_unknown() {
        assert_eq!(provider("claude").map(|t| t.id()), Some("claude"));
        assert!(provider("nope").is_none());
    }

    #[test]
    fn split_id_round_trips_and_rejects_malformed_ids() {
        assert_eq!(
            split_id("skill:brainstorming").unwrap(),
            (ResourceKind::Skill, "brainstorming".to_string())
        );
        assert!(split_id("nokind").is_err());
        assert!(split_id("bogus:name").is_err());
        assert!(split_id("skill:").is_err());
    }

    #[test]
    fn split_id_keeps_colons_in_the_name() {
        let (kind, name) = split_id("skill:a:b").unwrap();
        assert_eq!(kind, ResourceKind::Skill);
        assert_eq!(name, "a:b");
    }

    #[test]
    fn tools_report_no_linkable_dirs_when_not_installed() {
        // Point HOME at an empty directory so no tool root exists.
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);
        let infos = tool_infos();
        #[allow(deprecated)]
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        assert!(infos.iter().all(|t| !t.installed));
        assert!(infos.iter().all(|t| t.linkable_kinds.is_empty()));
        assert!(infos.iter().all(|t| !t.has_prompt && !t.has_mcp));
    }

    /// Drives the exact commands the UI calls through a full cycle: scan →
    /// import a drifted copy → toggle a tool on and off → delete → restore.
    #[test]
    fn full_cycle_through_the_ui_commands() {
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");

        // A tool copy the store doesn't own, plus two installed tools.
        std::fs::create_dir_all(home.join(".claude/skills/brainstorming")).unwrap();
        std::fs::write(
            home.join(".claude/skills/brainstorming/SKILL.md"),
            "---\ndescription: Brainstorm ideas\n---\nbody\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(home.join(".codex/skills")).unwrap();

        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);

        // Scan reports the copy as drifted.
        let snapshot = config_hub_snapshot();
        let resource = snapshot
            .resources
            .iter()
            .find(|r| r.id == "skill:brainstorming")
            .expect("scanned");
        assert_eq!(resource.apps.get("claude"), Some(&AppState::Drifted));
        assert_eq!(resource.description.as_deref(), Some("Brainstorm ideas"));

        // Import preview, then import: the copy moves into the store.
        let preview =
            config_hub_import_preview("skill:brainstorming".into(), "claude".into()).unwrap();
        assert!(!preview.conflict);
        config_hub_import("skill:brainstorming".into(), "claude".into(), false).unwrap();
        assert!(home.join(".agents/skills/brainstorming/SKILL.md").is_file());
        assert_eq!(
            scan::state_of(
                &home.join(".claude/skills/brainstorming"),
                ResourceKind::Skill,
                "brainstorming",
                "claude"
            ),
            AppState::Linked
        );

        // Toggle Codex on, then off again.
        config_hub_toggle(
            "skill:brainstorming".into(),
            "codex".into(),
            true,
            Some("copy".into()),
        )
        .unwrap();
        assert_eq!(
            scan::state_of(
                &home.join(".codex/skills/brainstorming"),
                ResourceKind::Skill,
                "brainstorming",
                "codex"
            ),
            AppState::Copied
        );
        config_hub_toggle("skill:brainstorming".into(), "codex".into(), false, None).unwrap();
        assert!(!home.join(".codex/skills/brainstorming").exists());

        // Delete unlinks Claude and trashes the store entry, then restore.
        let trash_id = config_hub_delete("skill:brainstorming".into()).unwrap();
        assert!(!home.join(".claude/skills/brainstorming").exists());
        assert!(!home.join(".agents/skills/brainstorming").exists());
        crate::trash::restore_entry(&trash_id).unwrap();
        assert!(home.join(".agents/skills/brainstorming/SKILL.md").is_file());

        #[allow(deprecated)]
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }

    /// A drifted copy must not be toggled off — that would delete the user's
    /// own file. It can only be imported.
    #[test]
    fn toggling_off_a_drifted_entry_is_refused() {
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".claude/skills/keepme")).unwrap();
        std::fs::create_dir_all(home.join(".agents/skills")).unwrap();

        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);

        let err =
            config_hub_toggle("skill:keepme".into(), "claude".into(), false, None).unwrap_err();
        assert!(err.contains("unmanaged"), "got: {err}");
        assert!(
            home.join(".claude/skills/keepme").is_dir(),
            "user's copy must survive"
        );

        #[allow(deprecated)]
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
}
