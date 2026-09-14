// ── Config Hub: DSH ──────────────────────────────────────────────────────
//
// Root `~/.dsh`. DSH has no skills or agents directory this hub manages (its
// agents are `.agent-presets/<name>/` directories, a different shape), so it
// is read-only for now: system prompt `AGENTS.md`, config `settings.yaml`.

use std::path::PathBuf;

use crate::config_hub::registry::{ConfigFile, ConfigFormat, LinkTarget, ToolProvider};
use crate::config_hub::utils::home_dir;

pub struct DshTool;

impl ToolProvider for DshTool {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn display_name(&self) -> &'static str {
        "DSH"
    }

    fn root(&self) -> Option<PathBuf> {
        let dir = home_dir()?.join(".dsh");
        dir.is_dir().then_some(dir)
    }

    fn linkable_dirs(&self) -> Vec<LinkTarget> {
        Vec::new()
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(self.root()?.join("AGENTS.md"))
    }

    fn config_files(&self) -> Vec<ConfigFile> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        vec![ConfigFile {
            label: "settings.yaml".to_string(),
            path: root.join("settings.yaml").to_string_lossy().into_owned(),
            format: ConfigFormat::Yaml,
        }]
    }

    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)> {
        None
    }
}
