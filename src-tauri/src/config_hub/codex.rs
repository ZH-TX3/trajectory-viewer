// ── Config Hub: Codex ────────────────────────────────────────────────────
//
// Root `~/.codex`. Skills are linkable; Codex has no agents directory. The
// system prompt is `AGENTS.md` and MCP servers live under `[mcp_servers]` in
// `config.toml`.

use std::path::PathBuf;

use crate::config_hub::registry::{ConfigFile, ConfigFormat, LinkTarget, ToolProvider};
use crate::config_hub::resource::ResourceKind;
use crate::config_hub::utils::home_dir;

pub struct CodexTool;

impl ToolProvider for CodexTool {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn root(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".codex");
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
            label: "config.toml".to_string(),
            path: root.join("config.toml").to_string_lossy().into_owned(),
            format: ConfigFormat::Toml,
        }]
    }

    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)> {
        Some((self.root()?.join("config.toml"), ConfigFormat::Toml))
    }
}
