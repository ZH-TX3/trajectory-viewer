// ── Config Hub: OpenCode ─────────────────────────────────────────────────
//
// Root `~/.config/opencode`. Skills are linkable; OpenCode has no agents
// directory. The system prompt is `AGENTS.md` and MCP servers live under the
// `mcp` key of `opencode.json`.

use std::path::PathBuf;

use crate::config_hub::registry::{ConfigFile, ConfigFormat, LinkTarget, ToolProvider};
use crate::config_hub::resource::ResourceKind;
use crate::config_hub::utils::home_dir;

pub struct OpenCodeTool;

impl ToolProvider for OpenCodeTool {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn root(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".config").join("opencode");
        dir.is_dir().then_some(dir)
    }

    fn linkable_dirs(&self) -> Vec<LinkTarget> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        vec![LinkTarget {
            kind: ResourceKind::Skill,
            dir: root.join("skills"),
        }]
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(self.root()?.join("AGENTS.md"))
    }

    fn config_files(&self) -> Vec<ConfigFile> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        vec![ConfigFile {
            label: "opencode.json".to_string(),
            path: root.join("opencode.json").to_string_lossy().into_owned(),
            format: ConfigFormat::Json,
        }]
    }

    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)> {
        Some((self.root()?.join("opencode.json"), ConfigFormat::Json))
    }
}
