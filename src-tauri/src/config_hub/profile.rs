// ── Config Hub: configuration profiles ───────────────────────────────────
//
// A profile is a named snapshot of which resources are enabled for which
// tools. Capturing records the current state; applying reconciles the live
// state to match it — enabling what the profile wants and disabling what it
// doesn't.
//
// Applies are additive-safe: a tool holding an *unmanaged* copy of a resource
// is never touched, because disabling it would delete the user's own file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::resource::{AppState, ResourceKind};
use super::{mcp, scan, sync};

/// One resource's enablement in a profile: `<kind>:<name>` → tool ids.
pub type ProfileEntries = BTreeMap<String, Vec<String>>;

/// A named snapshot of the hub's enablement state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    /// Skills and agents: resource id → tool ids it's enabled for.
    #[serde(default)]
    pub resources: ProfileEntries,
    /// MCP servers: server id → tool ids it's enabled for.
    #[serde(default)]
    pub mcp_servers: ProfileEntries,
    /// Creation time (ms since epoch), for stable ordering.
    pub created_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileStore {
    #[serde(default)]
    profiles: Vec<Profile>,
}

fn store_path() -> Option<PathBuf> {
    Some(
        super::utils::home_dir()?
            .join(".trajectory-viewer")
            .join("config-hub-profiles.json"),
    )
}

fn load_store() -> Result<ProfileStore, String> {
    let Some(path) = store_path() else {
        return Err("Cannot locate home directory".to_string());
    };
    if !path.exists() {
        return Ok(ProfileStore::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("Cannot read profiles: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Invalid profiles file: {e}"))
}

fn save_store(store: &ProfileStore) -> Result<(), String> {
    let path = store_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|e| format!("Cannot serialize: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write profiles: {e}"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Every stored profile, newest first.
pub fn list() -> Vec<Profile> {
    let mut profiles = load_store().map(|s| s.profiles).unwrap_or_default();
    profiles.sort_by_key(|p| std::cmp::Reverse(p.created_at));
    profiles
}

/// Snapshot the current enablement state under `name`.
///
/// Overwrites an existing profile of the same name.
pub fn capture(name: &str) -> Result<Profile, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }

    let mut resources: ProfileEntries = BTreeMap::new();
    for resource in scan::scan_resources() {
        let enabled: Vec<String> = resource
            .apps
            .iter()
            .filter(|(_, state)| matches!(state, AppState::Linked | AppState::Copied))
            .map(|(tool, _)| tool.clone())
            .collect();
        if !enabled.is_empty() {
            resources.insert(resource.id.clone(), enabled);
        }
    }

    let mut mcp_servers: ProfileEntries = BTreeMap::new();
    for server in mcp::list() {
        let enabled = server.enabled_apps();
        if !enabled.is_empty() {
            mcp_servers.insert(server.id.clone(), enabled);
        }
    }

    let profile = Profile {
        name: name.to_string(),
        resources,
        mcp_servers,
        created_at: now_ms(),
    };

    let mut store = load_store()?;
    store.profiles.retain(|p| p.name != profile.name);
    store.profiles.push(profile.clone());
    save_store(&store)?;
    Ok(profile)
}

/// Create or replace a profile from explicit entries.
///
/// Used by the editor, where the user picks the enablement directly rather
/// than snapshotting whatever happens to be live. `created_at` is preserved
/// when overwriting an existing profile.
pub fn save(
    name: &str,
    resources: ProfileEntries,
    mcp_servers: ProfileEntries,
) -> Result<Profile, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }

    let mut store = load_store()?;
    let created_at = store
        .profiles
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.created_at)
        .unwrap_or_else(now_ms);

    // Drop empty tool lists so a profile only records real enablement.
    let clean = |mut entries: ProfileEntries| {
        entries.retain(|_, tools| !tools.is_empty());
        entries
    };

    let profile = Profile {
        name: name.to_string(),
        resources: clean(resources),
        mcp_servers: clean(mcp_servers),
        created_at,
    };

    store.profiles.retain(|p| p.name != profile.name);
    store.profiles.push(profile.clone());
    save_store(&store)?;
    Ok(profile)
}

/// A profile as the editor needs it: its entries plus the current live state,
/// so the UI can show what a resource is set to right now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetail {
    pub name: String,
    pub resources: ProfileEntries,
    pub mcp_servers: ProfileEntries,
    /// `resource id → tool id → enabled` for the current live state.
    pub live_resources: BTreeMap<String, BTreeMap<String, bool>>,
    /// `server id → tool id → enabled` for the current live state.
    pub live_mcp: BTreeMap<String, BTreeMap<String, bool>>,
}

/// Load one profile together with the current live enablement.
pub fn detail(name: &str) -> Result<ProfileDetail, String> {
    let profile = load_store()?
        .profiles
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Profile not found: {name}"))?;

    let mut live_resources: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for resource in scan::scan_resources() {
        if !matches!(resource.kind, ResourceKind::Skill | ResourceKind::Agent) {
            continue;
        }
        let per_tool: BTreeMap<String, bool> = resource
            .apps
            .iter()
            .map(|(tool, state)| {
                (
                    tool.clone(),
                    matches!(state, AppState::Linked | AppState::Copied),
                )
            })
            .collect();
        live_resources.insert(resource.id.clone(), per_tool);
    }

    let mut live_mcp: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for server in mcp::list() {
        live_mcp.insert(server.id.clone(), server.apps.clone());
    }

    Ok(ProfileDetail {
        name: profile.name,
        resources: profile.resources,
        mcp_servers: profile.mcp_servers,
        live_resources,
        live_mcp,
    })
}

/// What applying a profile would change, and what it actually did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub enabled: usize,
    pub disabled: usize,
    /// Resources skipped because the tool holds an unmanaged copy.
    pub skipped: Vec<String>,
    /// Per-item failures (the rest of the apply still ran).
    pub errors: Vec<String>,
}

/// Reconcile the live state to match `profile`.
pub fn apply(name: &str) -> Result<ApplyReport, String> {
    let profile = load_store()?
        .profiles
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Profile not found: {name}"))?;

    let mut report = ApplyReport::default();

    // ── Skills / agents ──────────────────────────────────────────────────
    let wanted_resources = &profile.resources;
    for resource in scan::scan_resources() {
        let kind = resource.kind;
        if !matches!(kind, ResourceKind::Skill | ResourceKind::Agent) {
            continue;
        }
        let (_, res_name) = match super::split_id(&resource.id) {
            Ok(parts) => parts,
            Err(_) => continue,
        };
        let wanted_tools = wanted_resources.get(&resource.id);

        for (tool_id, state) in &resource.apps {
            let should_be_on = wanted_tools
                .map(|tools| tools.contains(tool_id))
                .unwrap_or(false);

            match (state, should_be_on) {
                // Already correct.
                (AppState::Linked | AppState::Copied, true) | (AppState::Absent, false) => {}
                // A tool's own copy: never touch it, just report — deleting it
                // would destroy the user's file, and enabling over it would
                // clobber it.
                (AppState::Drifted, _) => {
                    report.skipped.push(format!("{} @ {tool_id}", resource.id));
                }
                (AppState::Absent, true) => {
                    match sync::enable(kind, &res_name, tool_id, sync::SyncMethod::Auto) {
                        Ok(_) => report.enabled += 1,
                        Err(err) => report
                            .errors
                            .push(format!("enable {} @ {tool_id}: {err}", resource.id)),
                    }
                }
                (AppState::Linked | AppState::Copied, false) => {
                    match sync::disable(kind, &res_name, tool_id) {
                        Ok(()) => report.disabled += 1,
                        Err(err) => report
                            .errors
                            .push(format!("disable {} @ {tool_id}: {err}", resource.id)),
                    }
                }
            }
        }
    }

    // ── MCP servers ──────────────────────────────────────────────────────
    for server in mcp::list() {
        let wanted_tools = profile.mcp_servers.get(&server.id);
        for tool_id in mcp::MCP_APPS {
            let currently_on = server.apps.get(tool_id).copied().unwrap_or(false);
            let should_be_on = wanted_tools
                .map(|tools| tools.iter().any(|t| t == tool_id))
                .unwrap_or(false);
            if currently_on == should_be_on {
                continue;
            }
            match mcp::toggle_app(&server.id, tool_id, should_be_on) {
                Ok(()) => {
                    if should_be_on {
                        report.enabled += 1;
                    } else {
                        report.disabled += 1;
                    }
                }
                Err(err) => report
                    .errors
                    .push(format!("mcp {} @ {tool_id}: {err}", server.id)),
            }
        }
    }

    Ok(report)
}

/// Remove a stored profile.
pub fn delete(name: &str) -> Result<(), String> {
    let mut store = load_store()?;
    let before = store.profiles.len();
    store.profiles.retain(|p| p.name != name);
    if store.profiles.len() == before {
        return Err(format!("Profile not found: {name}"));
    }
    save_store(&store)
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

    /// A home with the SSOT, two tools, and one shared skill.
    fn setup(home: &Path) {
        std::fs::create_dir_all(home.join(".agents/skills/alpha")).unwrap();
        std::fs::write(home.join(".agents/skills/alpha/SKILL.md"), "alpha").unwrap();
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
    }

    #[test]
    fn capture_records_enabled_pairs() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();

            let profile = capture("work").unwrap();
            assert_eq!(profile.name, "work");
            assert_eq!(
                profile.resources.get("skill:alpha"),
                Some(&vec!["claude".to_string()])
            );
        });
    }

    #[test]
    fn capture_rejects_an_empty_name() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            assert!(capture("   ").is_err());
        });
    }

    #[test]
    fn apply_enables_what_the_profile_wants() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            // Profile says: alpha on for claude only.
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();
            capture("work").unwrap();

            // Turn everything off, then apply.
            sync::disable(ResourceKind::Skill, "alpha", "claude").unwrap();
            let report = apply("work").unwrap();
            assert_eq!(report.enabled, 1);
            assert_eq!(report.disabled, 0);
            assert_eq!(
                scan::state_of(
                    &home.join(".claude/skills/alpha"),
                    ResourceKind::Skill,
                    "alpha",
                    "claude"
                ),
                AppState::Linked
            );
        });
    }

    #[test]
    fn apply_disables_what_the_profile_omits() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            // Capture with nothing enabled.
            capture("empty").unwrap();

            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();
            let report = apply("empty").unwrap();
            assert_eq!(report.disabled, 1);
            assert!(!home.join(".claude/skills/alpha").exists(), "link removed");
            assert!(
                home.join(".agents/skills/alpha/SKILL.md").exists(),
                "SSOT intact"
            );
        });
    }

    // A tool's own unmanaged copy must survive an apply that would turn it off.
    #[test]
    fn apply_skips_drifted_entries() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            capture("empty").unwrap();

            // A real copy in codex, not a link we made.
            std::fs::create_dir_all(home.join(".codex/skills/alpha")).unwrap();
            std::fs::write(home.join(".codex/skills/alpha/SKILL.md"), "mine").unwrap();

            let report = apply("empty").unwrap();
            assert_eq!(report.disabled, 0);
            assert_eq!(report.skipped.len(), 1);
            assert!(report.skipped[0].contains("codex"));
            assert_eq!(
                std::fs::read_to_string(home.join(".codex/skills/alpha/SKILL.md")).unwrap(),
                "mine",
                "user's copy untouched"
            );
        });
    }

    #[test]
    fn apply_reports_an_unknown_profile() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            assert!(apply("ghost").is_err());
        });
    }

    #[test]
    fn delete_removes_a_profile() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            capture("temp").unwrap();
            assert_eq!(list().len(), 1);
            delete("temp").unwrap();
            assert!(list().is_empty());
            assert!(delete("temp").is_err(), "deleting twice errors");
        });
    }

    #[test]
    fn capture_overwrites_a_same_named_profile() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            capture("work").unwrap();
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();
            capture("work").unwrap();

            assert_eq!(list().len(), 1, "same name replaces, not duplicates");
            assert!(list()[0].resources.contains_key("skill:alpha"));
        });
    }

    // The editor's save: explicit entries, not a snapshot of the live state.
    #[test]
    fn save_stores_explicit_entries() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            // Nothing is enabled live, but the profile says alpha→claude.
            let resources =
                BTreeMap::from([("skill:alpha".to_string(), vec!["claude".to_string()])]);
            save("frontend", resources, BTreeMap::new()).unwrap();

            let stored = list();
            assert_eq!(stored.len(), 1);
            assert_eq!(
                stored[0].resources.get("skill:alpha"),
                Some(&vec!["claude".to_string()])
            );

            // Applying it turns alpha on for claude.
            let report = apply("frontend").unwrap();
            assert_eq!(report.enabled, 1);
        });
    }

    #[test]
    fn save_drops_empty_tool_lists() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let resources = BTreeMap::from([("skill:alpha".to_string(), Vec::new())]);
            save("blank", resources, BTreeMap::new()).unwrap();
            assert!(
                list()[0].resources.is_empty(),
                "a resource with no tools isn't recorded"
            );
        });
    }

    #[test]
    fn save_rejects_an_empty_name() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            assert!(save("  ", BTreeMap::new(), BTreeMap::new()).is_err());
        });
    }

    #[test]
    fn detail_returns_the_profile_and_live_state() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            save(
                "work",
                BTreeMap::from([("skill:alpha".to_string(), vec!["claude".to_string()])]),
                BTreeMap::new(),
            )
            .unwrap();

            let d = detail("work").unwrap();
            assert_eq!(d.name, "work");
            assert!(d.resources.contains_key("skill:alpha"));
            // Live state is reported for the scanned resource.
            assert!(d.live_resources.contains_key("skill:alpha"));
        });
    }

    #[test]
    fn detail_rejects_an_unknown_profile() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            assert!(detail("ghost").is_err());
        });
    }

    // Saving over an existing profile keeps its original creation time.
    #[test]
    fn save_preserves_created_at_on_overwrite() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let first = save("work", BTreeMap::new(), BTreeMap::new()).unwrap();
            let second = save(
                "work",
                BTreeMap::from([("skill:alpha".to_string(), vec!["claude".to_string()])]),
                BTreeMap::new(),
            )
            .unwrap();
            assert_eq!(first.created_at, second.created_at);
        });
    }
}

#[cfg(test)]
mod compat_tests {
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

    fn add_skill(home: &Path, name: &str) {
        let dir = home.join(format!(".agents/skills/{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), name).unwrap();
    }

    fn setup(home: &Path) {
        add_skill(home, "alpha");
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
    }

    /// Documents the current semantics: a profile is an EXACT desired state.
    /// A resource added AFTER the profile was captured is not in the profile,
    /// so applying it turns that resource off.
    #[test]
    fn applying_an_old_profile_disables_newly_added_resources() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            // Profile captured with only alpha enabled for claude.
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();
            capture("old").unwrap();

            // A new skill appears later, enabled for claude.
            add_skill(&home, "beta");
            sync::enable(
                ResourceKind::Skill,
                "beta",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();

            apply("old").unwrap();

            // alpha stays on; beta — absent from the profile — is turned OFF.
            assert_eq!(
                scan::state_of(
                    &home.join(".claude/skills/alpha"),
                    ResourceKind::Skill,
                    "alpha",
                    "claude"
                ),
                AppState::Linked
            );
            assert_eq!(
                scan::state_of(
                    &home.join(".claude/skills/beta"),
                    ResourceKind::Skill,
                    "beta",
                    "claude"
                ),
                AppState::Absent,
                "a resource the profile doesn't mention gets disabled"
            );
        });
    }

    /// Documents the same for tools: a tool installed after the profile was
    /// captured has no entries, so applying the profile turns everything off
    /// for it.
    #[test]
    fn applying_an_old_profile_disables_everything_for_a_new_tool() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            add_skill(&home, "alpha");
            std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "claude",
                sync::SyncMethod::Auto,
            )
            .unwrap();
            capture("old").unwrap();

            // codex shows up later with alpha enabled.
            std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
            sync::enable(
                ResourceKind::Skill,
                "alpha",
                "codex",
                sync::SyncMethod::Auto,
            )
            .unwrap();

            apply("old").unwrap();
            assert_eq!(
                scan::state_of(
                    &home.join(".codex/skills/alpha"),
                    ResourceKind::Skill,
                    "alpha",
                    "codex"
                ),
                AppState::Absent,
                "new tool's enablement is wiped by an old profile"
            );
        });
    }

    /// A profile entry for a resource that no longer exists is a silent no-op.
    #[test]
    fn stale_profile_entries_are_ignored() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            save(
                "stale",
                BTreeMap::from([("skill:ghost".to_string(), vec!["claude".to_string()])]),
                BTreeMap::new(),
            )
            .unwrap();
            let report = apply("stale").unwrap();
            assert_eq!(report.enabled, 0);
            assert!(report.errors.is_empty(), "no error for a missing resource");
        });
    }

    /// A profile entry for a tool that isn't installed is ignored.
    #[test]
    fn entries_for_uninstalled_tools_are_ignored() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            save(
                "toolgap",
                BTreeMap::from([(
                    "skill:alpha".to_string(),
                    vec!["claude".to_string(), "opencode".to_string()],
                )]),
                BTreeMap::new(),
            )
            .unwrap();
            let report = apply("toolgap").unwrap();
            // claude gets it; opencode isn't installed so there's nothing to do.
            assert_eq!(report.enabled, 1);
            assert!(report.errors.is_empty());
        });
    }
}
