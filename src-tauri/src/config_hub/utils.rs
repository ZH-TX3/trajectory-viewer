// ── Config Hub: shared helpers ───────────────────────────────────────────
//
// Locating the home directory, the `~/.agents/` single source of truth, and
// listing the entries that make up a linkable resource kind.

use std::path::{Path, PathBuf};

use super::resource::ResourceKind;

/// The user's home directory (`HOME`, or `USERPROFILE` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// The unified single source of truth root (`~/.agents`).
pub fn ssot_root() -> Option<PathBuf> {
    Some(home_dir()?.join(".agents"))
}

/// The SSOT directory holding one kind of resource.
pub fn ssot_dir(kind: ResourceKind) -> Option<PathBuf> {
    let root = ssot_root()?;
    match kind {
        ResourceKind::Skill => Some(root.join("skills")),
        ResourceKind::Agent => Some(root.join("agents")),
        _ => None,
    }
}

/// Where a named entry lives inside the SSOT.
///
/// Agents are single `.md` files; skills are directories holding a `SKILL.md`.
pub fn ssot_entry_path(kind: ResourceKind, name: &str) -> Option<PathBuf> {
    let dir = ssot_dir(kind)?;
    Some(match kind {
        ResourceKind::Agent => dir.join(format!("{name}.md")),
        _ => dir.join(name),
    })
}

/// Where a named entry lives inside a tool's linkable directory.
pub fn tool_entry_path(dir: &Path, kind: ResourceKind, name: &str) -> PathBuf {
    match kind {
        ResourceKind::Agent => dir.join(format!("{name}.md")),
        _ => dir.join(name),
    }
}

/// List the entries of one kind under `dir` as `(name, path)` pairs.
///
/// Names are normalized so the same resource is spelled the same way in the
/// SSOT and in every tool: agents drop their `.md`, skills keep the dir name.
/// Symlinks are listed too (including broken ones) so drift stays visible.
pub fn list_entries(dir: &Path, kind: ResourceKind) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let is_link = meta.file_type().is_symlink();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        // Dot-entries are a tool's own bookkeeping (Codex's `.system` skills),
        // not user resources — importing them into the store would be wrong.
        if file_name.starts_with('.') {
            continue;
        }
        let name = match kind {
            ResourceKind::Skill if is_link || path.is_dir() => file_name,
            ResourceKind::Agent if is_link || path.is_file() => {
                match file_name.strip_suffix(".md") {
                    Some(stem) if !stem.is_empty() => stem.to_string(),
                    _ => continue,
                }
            }
            _ => continue,
        };
        out.push((name, path));
    }
    out.sort();
    out
}

/// Read a one-line `description:` from a markdown file's frontmatter or body.
pub fn read_description(path: &Path) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines().take(30).flatten() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("description:") {
            let value = rest.trim().trim_matches(['"', '\'']).trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
