// ── Config Hub: tool registry ────────────────────────────────────────────
//
// Every managed tool implements `ToolProvider`. Adding a tool means writing
// one file in this directory and appending it to `PROVIDERS` below — the
// scanner, the sync engine and the UI's tool list all read from there.
//
// A provider is a zero-sized type (no state); the registry holds statics.

use std::path::PathBuf;

use serde::Serialize;

use super::resource::ResourceKind;

/// A directory inside a tool that resources can be linked into.
#[derive(Debug, Clone)]
pub struct LinkTarget {
    pub kind: ResourceKind,
    pub dir: PathBuf,
}

/// How a tool's config file is formatted (read-only display for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFormat {
    Json,
    Toml,
    Yaml,
    Markdown,
}

/// One config file a tool exposes for read-only display.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub label: String,
    pub path: String,
    pub format: ConfigFormat,
}

/// One AI tool whose configuration this app can manage.
pub trait ToolProvider: Sync {
    /// Stable identifier used across the frontend and the settings UI
    /// (`"claude"`, `"codex"`, `"dsh"`, `"opencode"`).
    fn id(&self) -> &'static str;

    /// Display name shown in the UI.
    fn display_name(&self) -> &'static str;

    /// Root directory holding this tool's configuration, when installed.
    /// `None` disables the tool.
    fn root(&self) -> Option<PathBuf>;

    /// Directories whose entries can be linked from the SSOT.
    fn linkable_dirs(&self) -> Vec<LinkTarget>;

    /// The tool's system-prompt / memory file, if it has one.
    fn prompt_file(&self) -> Option<PathBuf>;

    /// Other config files, exposed read-only.
    fn config_files(&self) -> Vec<ConfigFile>;

    /// The tool's MCP config file and its format, if supported.
    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)>;

    /// A linkable directory for one resource kind, if the tool supports it.
    fn linkable_dir(&self, kind: ResourceKind) -> Option<PathBuf> {
        self.linkable_dirs()
            .into_iter()
            .find(|t| t.kind == kind)
            .map(|t| t.dir)
    }
}

/// Every registered tool, in display order.
///
/// To add a tool: implement `ToolProvider` in its own module and append it
/// here. Nothing else in the app needs to change.
pub fn providers() -> &'static [&'static dyn ToolProvider] {
    static PROVIDERS: &[&dyn ToolProvider] = &[
        &super::claude::ClaudeTool,
        &super::codex::CodexTool,
        &super::dsh::DshTool,
        &super::opencode::OpenCodeTool,
    ];
    PROVIDERS
}

/// Look up a tool by its id.
pub fn provider(id: &str) -> Option<&'static dyn ToolProvider> {
    providers().iter().copied().find(|p| p.id() == id)
}

/// The ids of every registered tool, in display order.
#[cfg_attr(not(test), allow(dead_code))]
pub fn provider_ids() -> Vec<&'static str> {
    providers().iter().map(|p| p.id()).collect()
}
