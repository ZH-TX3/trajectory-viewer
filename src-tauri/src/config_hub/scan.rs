// ── Config Hub: resource scanning ────────────────────────────────────────
//
// Merges the SSOT and every installed tool's linkable directory into one
// `Resource` list. All state comes from the filesystem — the `.skill-lock.json`
// provenance file is deliberately not consulted, so a stale lock can't make
// the UI lie about what is actually on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::registry::providers;
use super::resource::{AppState, Resource, ResourceKind};
use super::state;
use super::utils::{list_entries, read_description, ssot_dir};

/// How one tool relates to one entry on disk.
pub fn state_of(target: &Path, kind: ResourceKind, name: &str, tool_id: &str) -> AppState {
    let Ok(meta) = std::fs::symlink_metadata(target) else {
        return AppState::Absent;
    };
    if meta.file_type().is_symlink() {
        return if points_into_ssot(target, kind) {
            AppState::Linked
        } else {
            AppState::Drifted
        };
    }
    if state::load().contains(kind, name, tool_id) {
        AppState::Copied
    } else {
        AppState::Drifted
    }
}

/// Whether a symlink's target resolves inside the SSOT directory for `kind`.
fn points_into_ssot(link: &Path, kind: ResourceKind) -> bool {
    let Some(ssot) = ssot_dir(kind) else {
        return false;
    };
    let Ok(dest) = std::fs::read_link(link) else {
        return false;
    };
    let dest = if dest.is_absolute() {
        dest
    } else {
        link.parent().map(|p| p.join(&dest)).unwrap_or(dest)
    };
    let resolved = std::fs::canonicalize(&dest).unwrap_or(dest);
    let root = std::fs::canonicalize(&ssot).unwrap_or(ssot);
    resolved.starts_with(root)
}

/// Every skill and agent across the SSOT and all installed tools.
pub fn scan_resources() -> Vec<Resource> {
    let mut merged: BTreeMap<String, Resource> = BTreeMap::new();

    for kind in [ResourceKind::Skill, ResourceKind::Agent] {
        let Some(ssot) = ssot_dir(kind) else {
            continue;
        };
        for (name, path) in list_entries(&ssot, kind) {
            let entry = merged
                .entry(Resource::id_of(kind, &name))
                .or_insert_with(|| new_resource(kind, &name));
            entry.in_ssot = true;
            entry.ssot_path = Some(path.to_string_lossy().into_owned());
            entry.description = read_description(&entry_description_source(&path, kind));
        }

        for tool in providers() {
            let Some(dir) = tool.linkable_dir(kind) else {
                continue;
            };
            for (name, path) in list_entries(&dir, kind) {
                let entry = merged
                    .entry(Resource::id_of(kind, &name))
                    .or_insert_with(|| new_resource(kind, &name));
                let state = state_of(&path, kind, &name, tool.id());
                entry.apps.insert(tool.id().to_string(), state);
                if !entry.in_ssot && entry.description.is_none() {
                    entry.description = read_description(&entry_description_source(&path, kind));
                }
            }
        }
    }

    // Tools with no entry for a resource are explicitly `Absent`, so the UI can
    // render a full switch row without guessing.
    for resource in merged.values_mut() {
        let kind = resource.kind;
        for tool in providers() {
            if tool.linkable_dir(kind).is_none() {
                continue;
            }
            resource
                .apps
                .entry(tool.id().to_string())
                .or_insert(AppState::Absent);
        }
    }

    merged.into_values().collect()
}

fn new_resource(kind: ResourceKind, name: &str) -> Resource {
    Resource {
        id: Resource::id_of(kind, name),
        kind,
        name: name.to_string(),
        description: None,
        ssot_path: None,
        in_ssot: false,
        apps: BTreeMap::new(),
    }
}

/// Where to look for a human-readable description.
///
/// A skill's description lives in its `SKILL.md`; an agent is the `.md` itself.
fn entry_description_source(path: &Path, kind: ResourceKind) -> PathBuf {
    match kind {
        ResourceKind::Skill => path.join("SKILL.md"),
        _ => path.to_path_buf(),
    }
}

/// Tool ids, exposed for the ignored smoke test below.
#[cfg(test)]
fn tool_ids_for_test() -> Vec<&'static str> {
    super::registry::providers()
        .iter()
        .map(|t| t.id())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_hub::utils::home_dir;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        // Recover from a poisoned lock so one failing test doesn't cascade.
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

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn ssot_path_is_under_the_agents_root() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            assert_eq!(home_dir().unwrap(), home);
            assert_eq!(
                ssot_dir(ResourceKind::Skill).unwrap(),
                home.join(".agents/skills")
            );
            assert_eq!(
                ssot_dir(ResourceKind::Agent).unwrap(),
                home.join(".agents/agents")
            );
        });
    }

    #[test]
    fn a_real_directory_is_drifted_and_a_link_is_linked() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let ssot = home.join(".agents/skills/alpha");
            std::fs::create_dir_all(&ssot).unwrap();
            let tool_dir = home.join(".claude/skills");
            std::fs::create_dir_all(&tool_dir).unwrap();

            // Real copy → drifted.
            let copy = tool_dir.join("alpha");
            std::fs::create_dir_all(&copy).unwrap();
            assert_eq!(
                state_of(&copy, ResourceKind::Skill, "alpha", "claude"),
                AppState::Drifted
            );
            std::fs::remove_dir_all(&copy).unwrap();

            // Link into the SSOT → linked.
            super::super::sync::create_symlink(&ssot, &copy).unwrap();
            assert_eq!(
                state_of(&copy, ResourceKind::Skill, "alpha", "claude"),
                AppState::Linked
            );
        });
    }

    #[test]
    fn a_link_pointing_outside_the_ssot_is_drifted() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let elsewhere = home.join("elsewhere/alpha");
            std::fs::create_dir_all(&elsewhere).unwrap();
            let tool_dir = home.join(".claude/skills");
            std::fs::create_dir_all(&tool_dir).unwrap();
            let link = tool_dir.join("alpha");
            super::super::sync::create_symlink(&elsewhere, &link).unwrap();

            assert_eq!(
                state_of(&link, ResourceKind::Skill, "alpha", "claude"),
                AppState::Drifted
            );
        });
    }

    #[test]
    fn a_missing_entry_is_absent() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let target = home.join(".claude/skills/ghost");
            assert_eq!(
                state_of(&target, ResourceKind::Skill, "ghost", "claude"),
                AppState::Absent
            );
        });
    }

    #[test]
    fn a_marked_copy_is_copied() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            let copy = home.join(".claude/skills/beta");
            std::fs::create_dir_all(&copy).unwrap();
            super::super::state::set_marker(ResourceKind::Skill, "beta", "claude", true).unwrap();
            assert_eq!(
                state_of(&copy, ResourceKind::Skill, "beta", "claude"),
                AppState::Copied
            );
        });
    }

    #[test]
    fn scanning_merges_ssot_and_tool_entries() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            write(
                &home.join(".agents/skills/brainstorming/SKILL.md"),
                "---\ndescription: Brainstorm ideas\n---\n",
            );
            write(
                &home.join(".agents/agents/search-agent.md"),
                "---\ndescription: Search the web\n---\n",
            );
            std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
            std::fs::create_dir_all(home.join(".claude/agents")).unwrap();
            std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
            // A drifted copy only in claude.
            std::fs::create_dir_all(home.join(".claude/skills/brainstorming")).unwrap();

            let resources = scan_resources();
            let skill = resources
                .iter()
                .find(|r| r.id == "skill:brainstorming")
                .unwrap();
            assert!(skill.in_ssot);
            assert_eq!(skill.apps.get("claude"), Some(&AppState::Drifted));
            assert_eq!(skill.apps.get("codex"), Some(&AppState::Absent));
            assert_eq!(skill.description.as_deref(), Some("Brainstorm ideas"));

            let agent = resources.iter().find(|r| r.id == "agent:search-agent");
            assert!(agent.is_some(), "agent from SSOT must be listed");
        });
    }

    #[test]
    fn scanning_does_not_modify_the_filesystem() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            write(&home.join(".agents/skills/x/SKILL.md"), "x");
            std::fs::create_dir_all(home.join(".claude/skills")).unwrap();

            let before = snapshot(&home);
            let _ = scan_resources();
            assert_eq!(before, snapshot(&home), "scan must be read-only");
        });
    }

    fn snapshot(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(dir: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                out.push(path.to_string_lossy().into_owned());
                if path.is_dir() {
                    walk(&path, out);
                }
            }
        }
        walk(root, &mut out);
        out.sort();
        out
    }

    /// Smoke test against the real home directory: read-only, so it is safe
    /// to run anywhere. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn scan_real_home_reports_drift() {
        let resources = scan_resources();
        let drifted: Vec<_> = resources
            .iter()
            .filter(|r| r.apps.values().any(|s| *s == AppState::Drifted))
            .collect();
        let mut out = String::new();
        out.push_str(&format!("tools: {:?}\n", super::tool_ids_for_test()));
        out.push_str(&format!("resources: {}\n", resources.len()));
        out.push_str(&format!("drifted: {}\n", drifted.len()));
        for r in &drifted {
            let states: Vec<String> = r
                .apps
                .iter()
                .map(|(tool, state)| format!("{tool}={state:?}"))
                .collect();
            out.push_str(&format!("  {} [{}]\n", r.id, states.join(", ")));
        }
        let _ = std::fs::write(std::env::temp_dir().join("config_hub_scan.txt"), &out);
        println!("{out}");
    }
}
