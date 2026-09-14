// ── Config Hub: import / migration ───────────────────────────────────────
//
// A tool directory often holds a real copy of a resource the SSOT doesn't have
// (or holds a different version of). Importing moves that copy into the SSOT
// and leaves a link behind. Every import is previewed first and can be undone,
// so the user never loses the original file to a surprise.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::registry::provider;
use super::resource::{AppState, ResourceKind};
use super::scan;
use super::state;
use super::sync;
use super::utils::{ssot_entry_path, tool_entry_path};

/// What an import would do, shown to the user before it runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub kind: ResourceKind,
    pub name: String,
    pub tool_id: String,
    /// The tool's existing copy that would move into the SSOT.
    pub source_path: String,
    /// Where it would land in the SSOT.
    pub target_path: String,
    /// Whether the SSOT already holds a same-named entry.
    pub conflict: bool,
    /// Human-readable summary of the planned action.
    pub action: String,
}

/// Describe what importing `name` from `tool_id` would do.
pub fn preview(kind: ResourceKind, name: &str, tool_id: &str) -> Result<ImportPreview, String> {
    let tool = provider(tool_id).ok_or_else(|| format!("Unsupported tool: {tool_id}"))?;
    let dir = tool
        .linkable_dir(kind)
        .ok_or_else(|| format!("{} does not support {}", tool.display_name(), kind.as_str()))?;
    let source = tool_entry_path(&dir, kind, name);

    match scan::state_of(&source, kind, name, tool_id) {
        AppState::Absent => Err(format!(
            "{} has no {} named {name}",
            tool.display_name(),
            kind.as_str()
        )),
        AppState::Linked => Err(format!(
            "{} already links {name} to the unified store",
            tool.display_name()
        )),
        AppState::Copied | AppState::Drifted => {
            let target = ssot_entry_path(kind, name)
                .ok_or_else(|| "Cannot locate the unified store".to_string())?;
            let conflict = target.exists();
            let action = if conflict {
                format!(
                    "Replace the unified-store copy with {}'s version and link it back",
                    tool.display_name()
                )
            } else {
                format!(
                    "Move {}'s copy into the unified store and link it back",
                    tool.display_name()
                )
            };
            Ok(ImportPreview {
                kind,
                name: name.to_string(),
                tool_id: tool_id.to_string(),
                source_path: source.to_string_lossy().into_owned(),
                target_path: target.to_string_lossy().into_owned(),
                conflict,
                action,
            })
        }
    }
}

/// Import a tool's copy into the SSOT and leave a link behind.
///
/// `overwrite` decides what happens when the SSOT already has a same-named
/// entry: `true` replaces it (the old one goes to the trash), `false` refuses.
pub fn import(
    kind: ResourceKind,
    name: &str,
    tool_id: &str,
    overwrite: bool,
) -> Result<(), String> {
    let plan = preview(kind, name, tool_id)?;
    let source = PathBuf::from(&plan.source_path);
    let target = PathBuf::from(&plan.target_path);

    if plan.conflict {
        if !overwrite {
            return Err(format!(
                "The unified store already has a {name}; choose overwrite or skip"
            ));
        }
        // Keep the replaced copy recoverable rather than destroying it.
        crate::trash::trash_paths(
            "config-hub",
            &format!("{}-{name}", kind.as_str()),
            &plan.action,
            std::slice::from_ref(&target),
        )?;
    }

    // A link can't be moved into the SSOT; materialize it first so the store
    // ends up with real content.
    let materialized = if is_link(&source) {
        let resolved = std::fs::canonicalize(&source)
            .map_err(|e| format!("Cannot resolve {}: {e}", source.display()))?;
        copy_to(&resolved, &target)?;
        sync::remove_path(&source).map_err(|e| format!("Cannot replace link: {e}"))?;
        true
    } else {
        false
    };

    if !materialized {
        sync::move_path(&source, &target)
            .map_err(|e| format!("Cannot move into the unified store: {e}"))?;
    }

    // Link the tool back to what is now the canonical copy.
    sync::create_symlink(&target, &source)
        .map_err(|e| format!("Imported, but cannot link back: {e}"))?;
    state::set_marker(kind, name, tool_id, false)?;
    Ok(())
}

/// Undo an import: move the SSOT entry back and restore the plain copy.
pub fn undo_import(kind: ResourceKind, name: &str, tool_id: &str) -> Result<(), String> {
    let tool = provider(tool_id).ok_or_else(|| format!("Unsupported tool: {tool_id}"))?;
    let dir = tool
        .linkable_dir(kind)
        .ok_or_else(|| format!("{} does not support {}", tool.display_name(), kind.as_str()))?;
    let tool_path = tool_entry_path(&dir, kind, name);
    let ssot_path = ssot_entry_path(kind, name)
        .filter(|p| p.exists())
        .ok_or_else(|| format!("Not in the unified store: {name}"))?;

    if scan::state_of(&tool_path, kind, name, tool_id) != AppState::Linked {
        return Err(format!(
            "{} does not link {name} to the unified store; nothing to undo",
            tool.display_name()
        ));
    }

    sync::remove_path(&tool_path).map_err(|e| format!("Cannot remove link: {e}"))?;
    sync::move_path(&ssot_path, &tool_path)
        .map_err(|e| format!("Cannot move the copy back: {e}"))?;
    state::set_marker(kind, name, tool_id, false)?;
    Ok(())
}

/// Delete a resource: unlink every tool, then trash the SSOT entry.
///
/// Returns the trash entry id so the caller can offer a restore.
pub fn delete_resource(kind: ResourceKind, name: &str) -> Result<String, String> {
    let ssot_path = ssot_entry_path(kind, name)
        .filter(|p| p.exists())
        .ok_or_else(|| format!("Not in the unified store: {name}"))?;

    // Unlink first: a dangling link would otherwise be left behind, and the
    // link target would be gone before we could clean it up.
    for tool in super::registry::providers() {
        if tool.linkable_dir(kind).is_none() {
            continue;
        }
        let _ = sync::disable(kind, name, tool.id());
    }

    let title = format!("{} {}", kind.as_str(), name);
    crate::trash::trash_paths("config-hub", name, &title, &[ssot_path])
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn copy_to(from: &Path, to: &Path) -> Result<(), String> {
    sync::copy_recursive(from, to).map_err(|e| format!("Cannot copy into the unified store: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_hub::scan::state_of;
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

    fn setup(home: &Path) {
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(home.join(".claude/agents")).unwrap();
        std::fs::create_dir_all(home.join(".agents/agents")).unwrap();
    }

    #[test]
    fn import_moves_a_drifted_copy_into_the_ssot_and_links_it_back() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let copy = home.join(".claude/skills/alpha/SKILL.md");
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(&copy, "body").unwrap();

            import(ResourceKind::Skill, "alpha", "claude", false).unwrap();

            let ssot = home.join(".agents/skills/alpha/SKILL.md");
            assert_eq!(std::fs::read_to_string(&ssot).unwrap(), "body");
            let link = home.join(".claude/skills/alpha");
            assert_eq!(
                state_of(&link, ResourceKind::Skill, "alpha", "claude"),
                AppState::Linked
            );
            assert_eq!(
                std::fs::read_to_string(link.join("SKILL.md")).unwrap(),
                "body"
            );
        });
    }

    #[test]
    fn import_then_undo_restores_the_plain_copy() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let copy = home.join(".claude/skills/beta/SKILL.md");
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(&copy, "body").unwrap();

            import(ResourceKind::Skill, "beta", "claude", false).unwrap();
            undo_import(ResourceKind::Skill, "beta", "claude").unwrap();

            let restored = home.join(".claude/skills/beta/SKILL.md");
            assert!(restored.exists(), "copy came back");
            assert!(
                !is_link(&home.join(".claude/skills/beta")),
                "no longer a link"
            );
            assert!(
                !home.join(".agents/skills/beta").exists(),
                "SSOT entry moved out"
            );
            assert_eq!(
                state_of(
                    &home.join(".claude/skills/beta"),
                    ResourceKind::Skill,
                    "beta",
                    "claude"
                ),
                AppState::Drifted
            );
        });
    }

    #[test]
    fn import_refuses_a_conflict_without_overwrite() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".agents/skills/gamma")).unwrap();
            std::fs::write(home.join(".agents/skills/gamma/SKILL.md"), "old").unwrap();
            let copy = home.join(".claude/skills/gamma/SKILL.md");
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(&copy, "new").unwrap();

            let err = import(ResourceKind::Skill, "gamma", "claude", false).unwrap_err();
            assert!(err.contains("already has"), "got: {err}");
            assert_eq!(
                std::fs::read_to_string(home.join(".agents/skills/gamma/SKILL.md")).unwrap(),
                "old",
                "unified store untouched"
            );
        });
    }

    #[test]
    fn import_overwrite_replaces_and_keeps_the_old_copy_in_trash() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".agents/skills/delta")).unwrap();
            std::fs::write(home.join(".agents/skills/delta/SKILL.md"), "old").unwrap();
            let copy = home.join(".claude/skills/delta/SKILL.md");
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(&copy, "new").unwrap();

            import(ResourceKind::Skill, "delta", "claude", true).unwrap();
            assert_eq!(
                std::fs::read_to_string(home.join(".agents/skills/delta/SKILL.md")).unwrap(),
                "new"
            );
            assert_eq!(crate::trash::list_trash().len(), 1, "old copy recoverable");
        });
    }

    #[test]
    fn delete_unlinks_every_tool_and_trashes_the_entry() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
            std::fs::create_dir_all(home.join(".agents/skills/eps")).unwrap();
            std::fs::write(home.join(".agents/skills/eps/SKILL.md"), "x").unwrap();

            sync::enable(ResourceKind::Skill, "eps", "claude", sync::SyncMethod::Auto).unwrap();
            sync::enable(ResourceKind::Skill, "eps", "codex", sync::SyncMethod::Auto).unwrap();

            let trash_id = delete_resource(ResourceKind::Skill, "eps").unwrap();

            assert!(!home.join(".claude/skills/eps").exists(), "link removed");
            assert!(!home.join(".codex/skills/eps").exists(), "link removed");
            assert!(
                !home.join(".agents/skills/eps").exists(),
                "SSOT entry moved"
            );
            assert_eq!(crate::trash::list_trash().len(), 1);

            crate::trash::restore_entry(&trash_id).unwrap();
            assert!(home.join(".agents/skills/eps/SKILL.md").exists());
        });
    }

    #[test]
    fn preview_reports_a_missing_entry() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let err = preview(ResourceKind::Skill, "ghost", "claude").unwrap_err();
            assert!(err.contains("no skill"), "got: {err}");
        });
    }

    #[test]
    fn preview_notes_a_conflict() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".agents/skills/zeta")).unwrap();
            std::fs::create_dir_all(home.join(".claude/skills/zeta")).unwrap();

            let plan = preview(ResourceKind::Skill, "zeta", "claude").unwrap();
            assert!(plan.conflict);
            assert!(plan.action.contains("Replace"));
            assert!(home_dir().is_some());
        });
    }
}
