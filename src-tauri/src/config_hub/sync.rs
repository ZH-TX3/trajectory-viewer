// ── Config Hub: sync engine ──────────────────────────────────────────────
//
// Turns a resource on or off for one tool. Enabling links the SSOT entry into
// the tool's directory — or copies it when a link can't be created (Windows
// without developer mode, cross-volume targets). Disabling removes only what
// we put there; the SSOT is never touched.

use std::fs;
use std::io;
use std::path::Path;

use super::registry::provider;
use super::resource::{AppState, ResourceKind};
use super::state;
use super::utils::{ssot_entry_path, tool_entry_path};

/// How a resource is delivered into a tool's directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMethod {
    /// Prefer a symlink; fall back to a copy when linking fails.
    Auto,
    /// Require a symlink; fail rather than copy.
    Symlink,
    /// Always copy.
    Copy,
}

impl SyncMethod {
    pub fn parse(value: &str) -> SyncMethod {
        match value {
            "symlink" => SyncMethod::Symlink,
            "copy" => SyncMethod::Copy,
            _ => SyncMethod::Auto,
        }
    }
}

/// Enable a resource for one tool. Idempotent.
pub fn enable(
    kind: ResourceKind,
    name: &str,
    tool_id: &str,
    method: SyncMethod,
) -> Result<AppState, String> {
    let tool = provider(tool_id).ok_or_else(|| format!("Unsupported tool: {tool_id}"))?;
    let dir = tool
        .linkable_dir(kind)
        .ok_or_else(|| format!("{} does not support {}", tool.display_name(), kind.as_str()))?;
    let source = ssot_entry_path(kind, name)
        .filter(|p| p.exists())
        .ok_or_else(|| format!("Not in the unified store: {name}"))?;
    let target = tool_entry_path(&dir, kind, name);

    // An existing entry is fine when it is already ours; anything else is the
    // user's data and must not be silently replaced.
    match super::scan::state_of(&target, kind, name, tool_id) {
        AppState::Linked | AppState::Copied => {
            return Ok(super::scan::state_of(&target, kind, name, tool_id))
        }
        AppState::Drifted => {
            return Err(format!(
                "{} already has an unmanaged {} named {name}; import it first",
                tool.display_name(),
                kind.as_str()
            ))
        }
        AppState::Absent => {}
    }

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

    let linked = match method {
        SyncMethod::Copy => {
            copy_recursive(&source, &target).map_err(|e| format!("Cannot copy into tool: {e}"))?;
            false
        }
        SyncMethod::Symlink => {
            create_symlink(&source, &target).map_err(|e| format!("Cannot create symlink: {e}"))?;
            true
        }
        SyncMethod::Auto => match create_symlink(&source, &target) {
            Ok(()) => true,
            Err(_) => {
                copy_recursive(&source, &target)
                    .map_err(|e| format!("Cannot copy into tool: {e}"))?;
                false
            }
        },
    };

    state::set_marker(kind, name, tool_id, !linked)?;
    Ok(if linked {
        AppState::Linked
    } else {
        AppState::Copied
    })
}

/// Disable a resource for one tool, removing only what we placed.
pub fn disable(kind: ResourceKind, name: &str, tool_id: &str) -> Result<(), String> {
    let tool = provider(tool_id).ok_or_else(|| format!("Unsupported tool: {tool_id}"))?;
    let dir = tool
        .linkable_dir(kind)
        .ok_or_else(|| format!("{} does not support {}", tool.display_name(), kind.as_str()))?;
    let target = tool_entry_path(&dir, kind, name);

    match super::scan::state_of(&target, kind, name, tool_id) {
        AppState::Absent => Ok(()),
        AppState::Linked | AppState::Copied => {
            remove_path(&target).map_err(|e| format!("Cannot remove {}: {e}", target.display()))?;
            state::set_marker(kind, name, tool_id, false)
        }
        AppState::Drifted => Err(format!(
            "Refusing to remove unmanaged entry at {}",
            target.display()
        )),
    }
}

/// Create a directory or file symlink, depending on what `source` is.
#[cfg(unix)]
pub fn create_symlink(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
pub fn create_symlink(source: &Path, target: &Path) -> io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
    } else {
        std::os::windows::fs::symlink_file(source, target)
    }
}

/// Remove a link or a copied entry, never the SSOT it points at.
///
/// A directory symlink on Windows must be removed with `remove_dir`; a file
/// symlink with `remove_file`. Trying both keeps this platform-agnostic.
pub fn remove_path(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(_) => fs::remove_dir(path),
        };
    }
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Copy a file or directory tree.
pub fn copy_recursive(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to).map(|_| ())
    }
}

/// Move a file or directory, falling back to copy+remove across volumes.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(from, to)?;
            remove_path(from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_hub::scan::state_of;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        // Recover from a poisoned lock so one failing test doesn't cascade.
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _ = fs::create_dir_all(home);
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
        std::fs::create_dir_all(home.join(".claude/skills/alpha")).unwrap();
        std::fs::write(home.join(".claude/skills/alpha/SKILL.md"), "body").unwrap();
        std::fs::create_dir_all(home.join(".codex/skills")).unwrap();
    }

    #[test]
    fn auto_links_when_it_can() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let ssot = home.join(".agents/skills/alpha");
            std::fs::create_dir_all(&ssot).unwrap();
            std::fs::write(ssot.join("SKILL.md"), "body").unwrap();

            let state = enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Auto).unwrap();
            assert_eq!(state, AppState::Linked);
            let link = home.join(".codex/skills/alpha");
            assert!(std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
        });
    }

    #[test]
    fn copy_method_produces_a_copied_state() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let ssot = home.join(".agents/skills/alpha");
            std::fs::create_dir_all(&ssot).unwrap();
            std::fs::write(ssot.join("SKILL.md"), "body").unwrap();

            let state = enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Copy).unwrap();
            assert_eq!(state, AppState::Copied);
            assert_eq!(
                state_of(
                    &home.join(".codex/skills/alpha"),
                    ResourceKind::Skill,
                    "alpha",
                    "codex"
                ),
                AppState::Copied
            );
        });
    }

    #[test]
    fn disabling_removes_the_link_but_never_the_ssot() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let ssot = home.join(".agents/skills/alpha");
            std::fs::create_dir_all(&ssot).unwrap();
            std::fs::write(ssot.join("SKILL.md"), "body").unwrap();

            enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Auto).unwrap();
            disable(ResourceKind::Skill, "alpha", "codex").unwrap();

            assert!(!home.join(".codex/skills/alpha").exists(), "link removed");
            assert_eq!(
                std::fs::read_to_string(ssot.join("SKILL.md")).unwrap(),
                "body",
                "SSOT content intact"
            );
        });
    }

    #[test]
    fn enabling_twice_is_idempotent() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let ssot = home.join(".agents/skills/alpha");
            std::fs::create_dir_all(&ssot).unwrap();
            std::fs::write(ssot.join("SKILL.md"), "body").unwrap();

            enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Auto).unwrap();
            let second = enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Auto).unwrap();
            assert_eq!(second, AppState::Linked);
            assert_eq!(
                std::fs::read_to_string(home.join(".codex/skills/alpha/SKILL.md")).unwrap(),
                "body"
            );
        });
    }

    #[test]
    fn enabling_a_missing_source_errors_without_a_dangling_link() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            let err = enable(ResourceKind::Skill, "ghost", "codex", SyncMethod::Auto).unwrap_err();
            assert!(err.contains("unified store"), "got: {err}");
            assert!(!home.join(".codex/skills/ghost").exists());
        });
    }

    #[test]
    fn enabling_refuses_to_clobber_a_drifted_entry() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".agents/skills/alpha")).unwrap();
            // Codex already holds a real, unmanaged copy.
            std::fs::create_dir_all(home.join(".codex/skills/alpha")).unwrap();

            let err = enable(ResourceKind::Skill, "alpha", "codex", SyncMethod::Auto).unwrap_err();
            assert!(err.contains("import it first"), "got: {err}");
            assert!(home.join(".codex/skills/alpha").is_dir(), "copy untouched");
        });
    }

    #[test]
    fn a_tool_without_the_kind_is_rejected() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        with_home(&home, || {
            setup(&home);
            std::fs::create_dir_all(home.join(".agents/agents")).unwrap();
            std::fs::write(home.join(".agents/agents/x.md"), "x").unwrap();
            let err = enable(ResourceKind::Agent, "x", "codex", SyncMethod::Auto).unwrap_err();
            assert!(err.contains("does not support"), "got: {err}");
        });
    }
}
