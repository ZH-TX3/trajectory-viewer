// ── Config Hub: tool settings ────────────────────────────────────────────
//
// Per-tool override for the config root directory. Each tool resolves its
// default location from `$HOME` automatically; a user who keeps a tool's
// config elsewhere can point this hub at the real directory here, and every
// derived path (skills/agents/prompt/config files) follows it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A tool's config root as the settings UI sees it: its auto-detected default
/// and whether the user has overridden it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRootInfo {
    pub tool_id: String,
    pub display_name: String,
    /// The auto-detected default (from `$HOME`), if the tool is installed at
    /// its usual place. Always shown so the user can see the baseline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_root: Option<String>,
    /// The user override, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_root: Option<String>,
}

/// Default root paths for the settings UI to show alongside overrides.
fn default_root(tool_id: &str) -> Option<PathBuf> {
    let home = super::utils::home_dir()?;
    Some(match tool_id {
        "claude" => home.join(".claude"),
        "codex" => home.join(".codex"),
        "dsh" => home.join(".dsh"),
        "opencode" => home.join(".config").join("opencode"),
        _ => return None,
    })
}

/// Every registered tool plus its default/override roots, for the UI.
pub fn tool_root_infos() -> Vec<ToolRootInfo> {
    super::registry::providers()
        .iter()
        .map(|tool| {
            let default_root = default_root(tool.id()).filter(|p| p.is_dir());
            let override_root = tool_root_override(tool.id());
            ToolRootInfo {
                tool_id: tool.id().to_string(),
                display_name: tool.display_name().to_string(),
                default_root: default_root.map(|p| p.to_string_lossy().into_owned()),
                override_root: override_root.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSettings {
    /// tool id → override config root (None = use the auto-detected default).
    #[serde(default)]
    pub tool_roots: BTreeMap<String, Option<String>>,
}

fn settings_path() -> Option<PathBuf> {
    Some(
        super::utils::home_dir()?
            .join(".trajectory-viewer")
            .join("config-hub-settings.json"),
    )
}

pub fn load() -> HubSettings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(settings: &HubSettings) -> Result<(), String> {
    let path = settings_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Cannot serialize settings: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write settings: {e}"))
}

/// The overridden config root for `tool_id`, or `None` when unset.
pub fn tool_root_override(tool_id: &str) -> Option<PathBuf> {
    load()
        .tool_roots
        .get(tool_id)
        .and_then(|dir| dir.as_ref())
        .map(PathBuf::from)
}

/// Resolve a tool's config root: the override when set, otherwise `default`.
///
/// An override is trusted as-is — the user explicitly pointed us at a
/// directory, so it's used even if it doesn't exist yet. The auto-detected
/// default is only reported (and the tool shown as installed) when it exists.
pub fn resolve_root(tool_id: &str, default: PathBuf) -> Option<PathBuf> {
    if let Some(override_root) = tool_root_override(tool_id) {
        return Some(override_root);
    }
    default.is_dir().then_some(default)
}

/// Set or clear the override for `tool_id`.
///
/// `dir` may be a plain path or a `~`-prefixed home-relative path. An empty
/// value clears the override.
pub fn set_tool_root(tool_id: &str, dir: &str) -> Result<HubSettings, String> {
    let mut settings = load();
    let trimmed = dir.trim();
    let resolved = if trimmed.is_empty() {
        None
    } else {
        let expanded = expand_home(trimmed);
        // Store the absolute path so a later $HOME change still resolves.
        Some(expanded.to_string_lossy().into_owned())
    };
    settings.tool_roots.insert(tool_id.to_string(), resolved);
    save(&settings)?;
    Ok(settings)
}

fn expand_home(dir: &str) -> PathBuf {
    if let Some(rest) = dir.strip_prefix("~/") {
        super::utils::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(dir))
    } else {
        PathBuf::from(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::parser::opencode::opencode_env_lock;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
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
    fn set_and_clear_a_tool_override() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            set_tool_root("claude", "D:/custom/claude").unwrap();
            assert_eq!(
                tool_root_override("claude").unwrap(),
                PathBuf::from("D:/custom/claude")
            );

            set_tool_root("claude", "").unwrap();
            assert_eq!(tool_root_override("claude"), None);
        });
    }

    #[test]
    fn tilde_expands_against_home() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            set_tool_root("codex", "~/codex-alt").unwrap();
            assert_eq!(tool_root_override("codex").unwrap(), home.join("codex-alt"));
        });
    }

    #[test]
    fn resolve_root_prefers_override_then_default() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            // Override is trusted as-is: the user pointed us at a directory, so
            // we don't fall back to the default even if it doesn't exist yet.
            let alt = home.join("custom-claude");
            set_tool_root("claude", &alt.to_string_lossy()).unwrap();
            assert_eq!(resolve_root("claude", home.join(".claude")).unwrap(), alt);

            // No override → the default, when it exists.
            set_tool_root("claude", "").unwrap();
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            assert_eq!(
                resolve_root("claude", home.join(".claude")).unwrap(),
                home.join(".claude")
            );

            // Neither override nor default exists → none.
            assert_eq!(resolve_root("codex", home.join(".codex")), None);
        });
    }
}
