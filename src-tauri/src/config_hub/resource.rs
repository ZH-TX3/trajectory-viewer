// ── Config Hub: resource model ───────────────────────────────────────────
//
// One `Resource` is a skill or agent seen across the SSOT and every installed
// tool. Its per-tool `AppState` is the thing the UI actually renders: whether
// that tool links to the SSOT, holds a copy, has an unmanaged (drifted) entry,
// or has nothing at all.

use std::collections::BTreeMap;

use serde::Serialize;

/// The kinds of thing the hub manages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Skill,
    Agent,
    Prompt,
    Mcp,
    Config,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Skill => "skill",
            ResourceKind::Agent => "agent",
            ResourceKind::Prompt => "prompt",
            ResourceKind::Mcp => "mcp",
            ResourceKind::Config => "config",
        }
    }

    /// Parse the `<kind>:<name>` prefix of a resource id.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "skill" => Some(ResourceKind::Skill),
            "agent" => Some(ResourceKind::Agent),
            "prompt" => Some(ResourceKind::Prompt),
            "mcp" => Some(ResourceKind::Mcp),
            "config" => Some(ResourceKind::Config),
            _ => None,
        }
    }
}

/// How one tool relates to a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    /// A symlink pointing into the SSOT.
    Linked,
    /// A real copy produced by copy-mode sync.
    Copied,
    /// Present but unmanaged: a real copy, or a link pointing elsewhere.
    Drifted,
    /// The tool has nothing for this resource.
    Absent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    /// Stable across scans: `<kind>:<name>`.
    pub id: String,
    pub kind: ResourceKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Absolute path in the SSOT, when the resource exists there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssot_path: Option<String>,
    pub in_ssot: bool,
    /// Per-tool state, keyed by tool id.
    pub apps: BTreeMap<String, AppState>,
}

impl Resource {
    pub fn id_of(kind: ResourceKind, name: &str) -> String {
        format!("{}:{}", kind.as_str(), name)
    }
}

/// A tool's copy-mode markers, persisted so a copy we made can be told apart
/// from a hand-placed file that merely looks the same.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyMarkers {
    /// `<kind>:<name>@<tool>` entries synced by copy.
    #[serde(default)]
    pub copied: Vec<String>,
}

impl CopyMarkers {
    pub fn key(kind: ResourceKind, name: &str, tool: &str) -> String {
        format!("{}:{name}@{tool}", kind.as_str())
    }

    pub fn contains(&self, kind: ResourceKind, name: &str, tool: &str) -> bool {
        self.copied.contains(&Self::key(kind, name, tool))
    }
}
