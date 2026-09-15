// ── Config Hub: Claude Code ──────────────────────────────────────────────
//
// Root `~/.claude`. Skills and agents are both linkable; the system prompt is
// `CLAUDE.md` and MCP servers live in `~/.claude.json` (a sibling of the root,
// not inside it).

use std::path::PathBuf;

use crate::config_hub::registry::{ConfigFile, ConfigFormat, LinkTarget, ToolProvider};
use crate::config_hub::resource::ResourceKind;
use crate::config_hub::utils::home_dir;

pub struct ClaudeTool;

impl ToolProvider for ClaudeTool {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn root(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".claude");
        crate::config_hub::settings::resolve_root(self.id(), dir)
    }

    fn linkable_dirs(&self) -> Vec<LinkTarget> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        vec![
            LinkTarget {
                kind: ResourceKind::Skill,
                dir: root.join("skills"),
            },
            LinkTarget {
                kind: ResourceKind::Agent,
                dir: root.join("agents"),
            },
        ]
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(self.root()?.join("CLAUDE.md"))
    }

    fn config_files(&self) -> Vec<ConfigFile> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        vec![
            ConfigFile {
                label: "settings.json".to_string(),
                path: root.join("settings.json").to_string_lossy().into_owned(),
                format: ConfigFormat::Json,
            },
            ConfigFile {
                label: "settings.local.json".to_string(),
                path: root
                    .join("settings.local.json")
                    .to_string_lossy()
                    .into_owned(),
                format: ConfigFormat::Json,
            },
        ]
    }

    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)> {
        let path = home_dir()?.join(".claude.json");
        Some((path, ConfigFormat::Json))
    }
}
