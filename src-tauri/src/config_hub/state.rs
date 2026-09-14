// ── Config Hub: copy markers ─────────────────────────────────────────────
//
// A copy-mode sync produces a real file/dir that is byte-identical to the
// SSOT but has no link to prove it came from us. These markers record which
// `<kind>:<name>@<tool>` entries we created, so a copy can be told apart from
// a hand-placed file (which stays `Drifted`).

use std::path::PathBuf;

use super::resource::{CopyMarkers, ResourceKind};

fn store_path() -> Option<PathBuf> {
    Some(
        super::utils::home_dir()?
            .join(".trajectory-viewer")
            .join("config-hub.json"),
    )
}

pub fn load() -> CopyMarkers {
    store_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(markers: &CopyMarkers) -> Result<(), String> {
    let path = store_path().ok_or_else(|| "Cannot locate home directory".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(markers)
        .map_err(|e| format!("Cannot serialize config-hub state: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot write config-hub state: {e}"))
}

/// Record (or clear) that `tool` holds a copy-mode sync of this resource.
pub fn set_marker(kind: ResourceKind, name: &str, tool: &str, copied: bool) -> Result<(), String> {
    let mut markers = load();
    let key = CopyMarkers::key(kind, name, tool);
    let present = markers.copied.iter().any(|k| k == &key);
    match (copied, present) {
        (true, false) => markers.copied.push(key),
        (false, true) => markers.copied.retain(|k| k != &key),
        _ => return Ok(()),
    }
    save(&markers)
}
