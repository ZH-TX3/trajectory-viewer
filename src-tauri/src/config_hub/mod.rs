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
pub mod profile;
pub mod registry;
pub mod resource;
pub mod scan;
pub mod settings;
pub mod state;
pub mod sync;
pub mod utils;

use std::path::PathBuf;

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

/// Every path a tool lets us edit: its prompt file and its declared configs.
fn editable_paths(tool_id: &str) -> Vec<PathBuf> {
    let Some(tool) = registry::provider(tool_id) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = tool
        .config_files()
        .into_iter()
        .map(|f| PathBuf::from(f.path))
        .collect();
    if let Some(prompt) = tool.prompt_file() {
        paths.push(prompt);
    }
    paths
}

/// Write one of a tool's own prompt/config files.
///
/// Refuses any path the tool doesn't declare, so a malformed request can never
/// write outside the tool's known config surface. The parent directory is
/// created when missing (some tools ship without their config file yet).
pub fn save_config_file(tool_id: &str, path: &str, content: &str) -> Result<(), String> {
    let target = PathBuf::from(path);
    let allowed = editable_paths(tool_id);
    if !allowed.iter().any(|p| p == &target) {
        return Err(format!(
            "Refusing to write a file {tool_id} does not own: {path}"
        ));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create dir: {e}"))?;
    }
    std::fs::write(&target, content).map_err(|e| format!("Cannot write {path}: {e}"))
}

/// Open one of a tool's config files in VS Code.
///
/// VS Code may be installed anywhere and its `code` CLI is often not on PATH,
/// so we ask the OS where it is (registry on Windows), then fall back to a
/// `code` on PATH, and finally to the OS default handler for the file. The
/// same path guard as `save_config_file` applies, so only the tool's own files
/// can be opened.
pub fn open_config_file(tool_id: &str, path: &str) -> Result<(), String> {
    let target = PathBuf::from(path);
    let allowed = editable_paths(tool_id);
    if !allowed.iter().any(|p| p == &target) {
        return Err(format!(
            "Refusing to open a file {tool_id} does not own: {path}"
        ));
    }
    if !target.exists() {
        return Err(format!("File does not exist: {path}"));
    }

    // 1) The CLI shim from the install the OS reports.
    if let Some(code) = discover_vscode_cli() {
        if spawn_code(&code, &target).is_ok() {
            return Ok(());
        }
    }

    // 2) `code` on PATH. On Windows it's a .cmd shim that must run via `cmd /c`
    //    (and `cmd /c` reports success even when the command is missing, so
    //    probe with `where` first).
    if code_on_path() && spawn_code_path_cli(&target).is_ok() {
        return Ok(());
    }

    // 3) Whatever the OS uses for this file type.
    open_with_default(&target)
}

/// The `code` CLI inside a VS Code install the OS knows about.
///
/// Nothing is hardcoded to a drive: on Windows the install location comes from
/// the uninstall registry keys, so a custom install directory is found too.
#[cfg(windows)]
fn discover_vscode_cli() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Query both the user and machine uninstall hives for VS Code's
    // InstallLocation, then look for the CLI shim there.
    let script = r#"
$paths = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
Get-ItemProperty $paths -ErrorAction SilentlyContinue |
  Where-Object { $_.DisplayName -like '*Visual Studio Code*' -and $_.InstallLocation } |
  Select-Object -ExpandProperty InstallLocation -Unique
"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let dir = line.trim();
        if dir.is_empty() {
            continue;
        }
        for shim in ["bin/code.cmd", "bin/code.exe", "Code.exe"] {
            let candidate = PathBuf::from(dir).join(shim);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn discover_vscode_cli() -> Option<PathBuf> {
    let path =
        PathBuf::from("/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code");
    path.is_file().then_some(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn discover_vscode_cli() -> Option<PathBuf> {
    // `which code` is the reliable answer on Linux.
    let output = std::process::Command::new("which")
        .arg("code")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    path.is_file().then_some(path)
}

/// Launch a `code` shim (a .cmd/.exe path or the bare command name).
///
/// On Windows a `.cmd` must be run through `cmd /c`; a real `.exe` can be
/// spawned directly.
#[cfg(windows)]
fn spawn_code(code: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    let is_cmd = code
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    let mut cmd = if is_cmd {
        let mut c = std::process::Command::new("cmd");
        c.arg("/c").arg(code);
        c
    } else {
        std::process::Command::new(code)
    };
    cmd.arg("--reuse-window")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot launch VS Code: {e}"))
}

#[cfg(not(windows))]
fn spawn_code(code: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(code)
        .arg("--reuse-window")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot launch VS Code: {e}"))
}

/// Launch the bare `code` command (already confirmed on PATH).
#[cfg(windows)]
fn spawn_code_path_cli(target: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/c", "code", "--reuse-window"])
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot launch VS Code: {e}"))
}

#[cfg(not(windows))]
fn spawn_code_path_cli(target: &std::path::Path) -> Result<(), String> {
    spawn_code(std::path::Path::new("code"), target)
}

/// Whether a `code` command is actually runnable from PATH.
#[cfg(windows)]
fn code_on_path() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/c", "where", "code"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn code_on_path() -> bool {
    std::process::Command::new("sh")
        .args(["-c", "command -v code"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Hand the file to the OS default program.
#[cfg(windows)]
fn open_with_default(target: &std::path::Path) -> Result<(), String> {
    // `cmd /c start "" <file>` — the empty title keeps `start` from treating a
    // quoted path as its window title.
    std::process::Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open file: {e}"))
}

#[cfg(target_os = "macos")]
fn open_with_default(target: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open file: {e}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_default(target: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open file: {e}"))
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

#[tauri::command]
pub fn config_hub_save_config(
    tool_id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    save_config_file(&tool_id, &path, &content)
}

#[tauri::command]
pub fn config_hub_open_config(tool_id: String, path: String) -> Result<(), String> {
    open_config_file(&tool_id, &path)
}

/// Current per-tool config-root overrides, plus each tool's auto-detected
/// default, so the settings UI can show what's in play.
#[tauri::command]
pub fn config_hub_tool_roots() -> Vec<crate::config_hub::settings::ToolRootInfo> {
    crate::config_hub::settings::tool_root_infos()
}

/// Set (or clear, with an empty dir) a tool's config-root override.
#[tauri::command]
pub fn config_hub_set_tool_root(tool_id: String, dir: String) -> Result<(), String> {
    crate::config_hub::settings::set_tool_root(&tool_id, &dir).map(|_| ())
}

// ── Profile commands ─────────────────────────────────────────────────────

#[tauri::command]
pub fn config_hub_profiles() -> Vec<profile::Profile> {
    profile::list()
}

/// Snapshot the current enablement state under `name`.
#[tauri::command]
pub fn config_hub_save_profile(name: String) -> Result<profile::Profile, String> {
    profile::capture(&name)
}

/// Create or replace a profile from explicit entries (the editor's save).
#[tauri::command]
pub fn config_hub_upsert_profile(
    name: String,
    resources: profile::ProfileEntries,
    mcp_servers: profile::ProfileEntries,
) -> Result<profile::Profile, String> {
    profile::save(&name, resources, mcp_servers)
}

/// One profile plus the current live state, for the editor.
#[tauri::command]
pub fn config_hub_profile_detail(name: String) -> Result<profile::ProfileDetail, String> {
    profile::detail(&name)
}

/// Reconcile the live state to match a stored profile.
#[tauri::command]
pub fn config_hub_apply_profile(name: String) -> Result<profile::ApplyReport, String> {
    profile::apply(&name)
}

#[tauri::command]
pub fn config_hub_delete_profile(name: String) -> Result<(), String> {
    profile::delete(&name)
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

/// Run a live handshake against one stored MCP server.
///
/// Long-running by nature (spawns a process or makes a network call), so it
/// runs off the main thread.
#[tauri::command]
pub async fn config_hub_mcp_test(id: String) -> Result<mcp::test::McpTestResult, String> {
    let server = mcp::list()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("MCP server not found: {id}"))?;
    tauri::async_runtime::spawn_blocking(move || mcp::test::test_server(&server.server))
        .await
        .map_err(|e| format!("Test task failed: {e}"))
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
    fn save_config_writes_a_declared_file() {
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);

        let target = home.join(".claude/CLAUDE.md");
        save_config_file("claude", &target.to_string_lossy(), "# hello").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "# hello");

        #[allow(deprecated)]
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }

    // The whole point of the guard: a request naming an arbitrary path must be
    // rejected, so a malformed call can't write outside the tool's own config.
    #[test]
    fn save_config_refuses_undeclared_paths() {
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);

        let outsider = home.join("elsewhere/evil.txt");
        let err = save_config_file("claude", &outsider.to_string_lossy(), "x").unwrap_err();
        assert!(err.contains("does not own"), "got: {err}");
        assert!(!outsider.exists(), "nothing written");

        // An unknown tool is refused too.
        assert!(save_config_file("nope", "/tmp/x", "x").is_err());

        #[allow(deprecated)]
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
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

#[cfg(test)]
mod open_tests {
    use super::*;

    /// The OS-reported VS Code CLI must be discovered without any hardcoded
    /// path — this machine has it at a non-default location.
    #[test]
    #[ignore]
    fn discovers_the_installed_vscode_cli() {
        let found = discover_vscode_cli();
        eprintln!("discovered: {found:?}");
        assert!(found.is_some(), "VS Code CLI not discovered");
    }

    #[test]
    #[ignore]
    fn code_on_path_reports_a_bool() {
        eprintln!("code_on_path: {}", code_on_path());
    }
}

#[cfg(test)]
mod open_e2e_tests {
    use super::*;

    /// End-to-end: opening a real tool config launches VS Code. Ignored by
    /// default since it spawns a GUI app. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn open_config_file_launches_the_editor() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let original = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", &home);

        let target = home.join(".claude/CLAUDE.md");
        std::fs::write(&target, "# test").unwrap();

        let result = open_config_file("claude", &target.to_string_lossy());
        eprintln!("open result: {result:?}");
        assert!(result.is_ok(), "open failed: {result:?}");

        #[allow(deprecated)]
        match original {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
