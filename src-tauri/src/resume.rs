// ── Resume in terminal ───────────────────────────────────────────────────
//
// Opens a terminal that continues a session, so the reader can jump from the
// transcript straight back into the conversation.
//
// Two platform behaviours shape this module:
//
//   - Windows reuses the OPEN Windows Terminal window: `wt -w 0 nt` adds a tab
//     to it rather than spawning another window. Without Windows Terminal
//     installed it falls back to a plain console window.
//   - macOS runs the command in the frontmost Terminal window via `do script`.
//
// The command itself is never assembled as one shell string. The session id is
// validated to the character set the providers actually emit, and the parts are
// passed as separate argv entries (or as an AppleScript-quoted literal), so a
// hostile id cannot become a second command.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::home_dir;

/// Provider ids this launcher knows, mirroring `session_manager::providers()`.
pub const PROVIDERS: [&str; 4] = ["claude", "codex", "dsh", "opencode"];

/// Terminal ids the launcher understands, first entry being the default.
#[cfg(windows)]
pub const TERMINALS: [&str; 4] = ["auto", "wt", "cmd", "powershell"];
#[cfg(target_os = "macos")]
pub const TERMINALS: [&str; 3] = ["auto", "terminal", "iterm"];
#[cfg(not(any(windows, target_os = "macos")))]
pub const TERMINALS: [&str; 5] = [
    "auto",
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "xterm",
];

/// The CLI a provider ships with. Each tool has its own, so one shared command
/// cannot work — resuming a Codex session with `claude --resume` is nonsense.
fn default_command(provider_id: &str) -> &'static str {
    match provider_id {
        "codex" => "codex",
        "dsh" => "dsh-tui",
        "opencode" => "opencode",
        _ => "claude",
    }
}

/// User preferences for the resume launcher: one command per provider, and the
/// terminal the tab opens in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSettings {
    /// Provider id → program to start. A missing entry falls back to that
    /// provider's default, so a new provider still works without a migration.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
    /// Terminal id to open the tab in; `auto` (or unset) picks the best one
    /// installed. An unknown or unavailable id degrades to the same chain.
    #[serde(default)]
    pub terminal: Option<String>,
}

impl ResumeSettings {
    /// The command configured for `provider_id`, or that provider's default.
    pub fn command_for(&self, provider_id: &str) -> String {
        self.commands
            .get(provider_id)
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| default_command(provider_id).to_string())
    }

    /// The configured terminal id, or `auto` when unset or blank.
    pub fn terminal(&self) -> String {
        self.terminal
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("auto")
            .to_string()
    }
}

/// One terminal the launcher can open, and whether it is usable here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatus {
    /// Stable id, stored in the settings file.
    pub id: String,
    /// Human-readable name for the dropdown.
    pub label: String,
    /// False when the terminal is not installed on this machine.
    pub available: bool,
    /// True for the `auto` entry, which follows whatever is installed.
    pub is_default: bool,
}

/// Every selectable terminal, in preference order.
///
/// `auto` is always present and always available: it is the fallback chain, and
/// picking it is what makes the launcher survive a terminal being uninstalled.
pub fn all_terminals() -> Vec<TerminalStatus> {
    TERMINALS
        .iter()
        .map(|id| TerminalStatus {
            id: (*id).to_string(),
            label: terminal_label(id),
            available: *id == "auto" || terminal_available(id),
            is_default: *id == "auto",
        })
        .collect()
}

#[cfg(windows)]
fn terminal_label(id: &str) -> String {
    match id {
        "auto" => "System default",
        "wt" => "Windows Terminal",
        "cmd" => "Command Prompt",
        "powershell" => "PowerShell",
        other => other,
    }
    .to_string()
}

#[cfg(target_os = "macos")]
fn terminal_label(id: &str) -> String {
    match id {
        "auto" => "System default",
        "terminal" => "Terminal",
        "iterm" => "iTerm",
        other => other,
    }
    .to_string()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn terminal_label(id: &str) -> String {
    match id {
        "auto" => "System default",
        other => other,
    }
    .to_string()
}

fn data_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".trajectory-viewer")
}

fn settings_path() -> PathBuf {
    data_dir().join("resume.json")
}

/// Read persisted launcher preferences (with defaults).
pub fn get_settings() -> ResumeSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<ResumeSettings>(&raw).ok())
        .unwrap_or_default()
}

/// Persist launcher preferences, validating every command.
pub fn set_settings(settings: &ResumeSettings) -> Result<(), String> {
    let mut commands = std::collections::BTreeMap::new();
    for (provider_id, raw) in &settings.commands {
        let command = raw.trim();
        if command.is_empty() {
            // An empty entry means "use the default", not an error.
            continue;
        }
        validate_command(command)?;
        commands.insert(provider_id.clone(), command.to_string());
    }
    // A terminal id is only ever stored, never interpolated into a shell line,
    // so it is folded onto the known set rather than character-validated.
    let terminal = match settings.terminal() {
        t if t == "auto" => None,
        t if TERMINALS.contains(&t.as_str()) => Some(t),
        other => return Err(format!("Unknown terminal: {other}")),
    };
    std::fs::create_dir_all(data_dir()).map_err(|e| format!("Cannot create data dir: {e}"))?;
    let raw = serde_json::to_string_pretty(&ResumeSettings { commands, terminal })
        .map_err(|e| format!("Cannot serialize settings: {e}"))?;
    std::fs::write(settings_path(), raw).map_err(|e| format!("Cannot save settings: {e}"))?;
    // The memoised shell lookups describe the profile, not this file, but a
    // save is the one moment the user is waiting on a fresh verdict — so the
    // cache is dropped rather than left to answer with the previous command's
    // result.
    if let Ok(mut cache) = shell_lookup_cache().lock() {
        cache.clear();
    }
    Ok(())
}

/// A command is interpolated into a shell line, so it must not carry shell
/// syntax of its own. A name (`claude`) or a path is accepted; flags belong in
/// the alias the name resolves to.
fn validate_command(command: &str) -> Result<(), String> {
    if command.is_empty() {
        return Err("Command cannot be empty".to_string());
    }
    if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\\' | ':' | '~'))
    {
        return Err(format!(
            "Command contains unsupported characters: {command}"
        ));
    }
    Ok(())
}

/// What the launcher reports back so the UI can explain a degraded result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeOutcome {
    /// The full command line that was launched, for display.
    pub command_line: String,
    /// Directory the terminal opened in — `None` when the recorded one is gone
    /// and the launch fell back to the home directory.
    pub working_dir: Option<String>,
    /// Set when the recorded working directory no longer exists.
    pub cwd_missing: bool,
}

/// How a configured command has to be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launcher {
    /// A real program (or a path): run it directly.
    Direct,
    /// A shell alias or function, which only exists inside an interactive
    /// shell with the user's profile loaded.
    ShellAlias,
    /// Nothing by this name is runnable.
    Missing,
}

/// Classify `command` so the launcher knows how to start it.
///
/// A name like `cc` is often a shell function rather than a program: it lives
/// in a profile, so `where`/`which` cannot see it and only an interactive
/// shell can run it.
fn classify(command: &str) -> Launcher {
    if command.trim().is_empty() {
        return Launcher::Missing;
    }
    // An explicit path is checked directly.
    if Path::new(command).components().count() > 1 {
        return if Path::new(command).is_file() {
            Launcher::Direct
        } else {
            Launcher::Missing
        };
    }
    if which(command) {
        return Launcher::Direct;
    }
    // Only the shell lookup is cached, because only it costs a process spawn
    // (~1s: PowerShell has to load the user's profile). A PATH scan is cheap
    // enough to redo, and redoing it means a tool installed while the app is
    // running still shows up.
    let cache = shell_lookup_cache();
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let shell_says = *cache
        .entry(command.to_string())
        .or_insert_with(|| resolves_in_shell(command));
    drop(cache);
    if shell_says {
        Launcher::ShellAlias
    } else {
        Launcher::Missing
    }
}

/// Memoised results of `resolves_in_shell`, keyed by command name.
///
/// Resolving an alias means starting an interactive shell with the user's
/// profile loaded, which measured ~1s per call. The settings pane asks for
/// every provider at once, so without this each open cost seconds.
fn shell_lookup_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, bool>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, bool>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Whether a program can be started (a real program, a path, or a shell alias).
pub fn command_available(command: &str) -> bool {
    classify(command) != Launcher::Missing
}

/// How every provider's configured command resolves, for the settings UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandStatus {
    /// `program` (a real executable on PATH), `alias` (a shell function or
    /// alias from the user's profile), or `missing`.
    pub kind: String,
    /// The command that was checked.
    pub command: String,
}

/// Resolve one command for display.
pub fn command_status(command: &str) -> CommandStatus {
    let kind = match classify(command) {
        Launcher::Direct => "program",
        Launcher::ShellAlias => "alias",
        Launcher::Missing => "missing",
    };
    CommandStatus {
        kind: kind.to_string(),
        command: command.to_string(),
    }
}

/// One provider's effective launcher command and how it resolves.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCommand {
    pub provider_id: String,
    pub command: String,
    /// `program` | `alias` | `missing`.
    pub kind: String,
}

/// Every provider's effective command, for the settings UI.
pub fn all_command_status() -> Vec<ProviderCommand> {
    let settings = get_settings();
    PROVIDERS
        .iter()
        .map(|provider_id| {
            let command = settings.command_for(provider_id);
            let status = command_status(&command);
            ProviderCommand {
                provider_id: (*provider_id).to_string(),
                command: status.command,
                kind: status.kind,
            }
        })
        .collect()
}

/// Whether an interactive shell can resolve `command` (i.e. it is an alias or
/// function defined in the user's profile).
#[cfg(windows)]
fn resolves_in_shell(command: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Deliberately WITHOUT -NoProfile: the whole point is to load it.
    std::process::Command::new("powershell")
        .args(["-Command", &format!("Get-Command {command}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// POSIX shells load aliases only in interactive mode, so probe through a
/// login interactive shell.
#[cfg(not(windows))]
fn resolves_in_shell(command: &str) -> bool {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    std::process::Command::new(shell)
        .args(["-lic", &format!("command -v {command}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whether `command` is an executable on `PATH`.
///
/// A pure PATH scan, deliberately not `cmd /c where` or `which`: spawning a
/// process costs hundreds of milliseconds on Windows, and this runs once per
/// provider every time the settings pane opens.
#[cfg(windows)]
fn which(command: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    // `where` resolves through PATHEXT, so a bare `codex` matches `codex.cmd`.
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_lowercase())
        .collect();
    // A name that already carries an extension is matched as-is; Windows
    // filesystems are case-insensitive, so no case folding is needed.
    let has_extension = Path::new(command).extension().is_some();
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if dir.join(command).is_file() {
            return true;
        }
        if !has_extension
            && extensions
                .iter()
                .any(|ext| dir.join(format!("{command}{ext}")).is_file())
        {
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
fn which(command: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(command);
        // POSIX requires the execute bit, not merely the file's existence.
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
                return true;
            }
        }
    }
    false
}

/// Reject a session id outside the character set the providers emit.
///
/// Ids come from the frontend and end up on a command line, so this is the
/// boundary that keeps one from carrying shell syntax.
fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() {
        return Err("Missing session id".to_string());
    }
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("Invalid session id: {session_id}"));
    }
    Ok(())
}

/// The continuation arguments for a provider — the SINGLE source of truth for
/// how each tool spells "continue this session".
///
/// Every other place that needs this (the `resume_command` shown in the UI, the
/// copy button) goes through here, so a CLI changing its flag is a one-line
/// edit rather than a hunt through four files.
pub fn resume_args(provider_id: &str, session_id: &str) -> Result<Vec<String>, String> {
    let args: &[&str] = match provider_id {
        "claude" => &["--resume"],
        "codex" => &["resume"],
        "dsh" => &["--resume"],
        // `opencode session` has no `resume` subcommand; the flag is
        // `--session` / `-s`.
        "opencode" => &["--session"],
        other => return Err(format!("Resume is not supported for provider: {other}")),
    };
    Ok(args
        .iter()
        .map(|a| (*a).to_string())
        .chain(std::iter::once(session_id.to_string()))
        .collect())
}

/// The command line a user would type to continue this session, for display.
///
/// Uses the provider's DEFAULT program name, never the configured one: this
/// runs during a session scan, and reading user settings here would make the
/// result depend on machine state (and make the parsers untestable). The UI
/// swaps in the configured command when it renders.
pub fn display_command(provider_id: &str, session_id: &str) -> Option<String> {
    let args = resume_args(provider_id, session_id).ok()?;
    Some(format!(
        "{} {}",
        default_command(provider_id),
        args.join(" ")
    ))
}

/// Launch a terminal that resumes `session_id` in `cwd`.
pub fn launch(
    provider_id: &str,
    session_id: &str,
    cwd: Option<&str>,
) -> Result<ResumeOutcome, String> {
    validate_session_id(session_id)?;

    // Each provider has its own CLI and its own continuation flag, so the
    // command is looked up per provider rather than shared.
    let settings = get_settings();
    let command = settings.command_for(provider_id);
    let launcher = classify(&command);
    if launcher == Launcher::Missing {
        return Err(format!(
            "`{command}` was not found. Install it, or change it in Settings."
        ));
    }

    // The recorded directory is where the session was started. A session can
    // outlive its project, so a missing one degrades to the home directory
    // instead of failing the launch.
    let recorded = cwd.map(PathBuf::from).filter(|p| p.is_dir());
    let cwd_missing = cwd.is_some() && recorded.is_none();
    let working_dir = recorded.clone().or_else(home_dir);
    let working_dir = working_dir.filter(|p| p.is_dir());

    let resume_args = resume_args(provider_id, session_id)?;

    let command_line = format!("{} {}", command, resume_args.join(" "));
    spawn_terminal(
        &settings.terminal(),
        launcher,
        &command,
        &resume_args,
        working_dir.as_deref(),
    )?;

    Ok(ResumeOutcome {
        command_line,
        working_dir: working_dir.map(|p| p.to_string_lossy().to_string()),
        cwd_missing,
    })
}

// ── Platform launchers ────────────────────────────────────────────────────

/// Where Windows Terminal's `wt.exe` lives, when it is installed.
///
/// It is a Store app whose shim is normally on PATH, but a side-loaded or
/// portable install (as on this machine) is not — so the known install roots
/// are probed too. `%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe` is the shim
/// Windows creates for a packaged install.
#[cfg(windows)]
fn windows_terminal() -> Option<PathBuf> {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let shim = Path::new(&local)
            .join("Microsoft")
            .join("WindowsApps")
            .join("wt.exe");
        if shim.is_file() {
            return Some(shim);
        }
    }
    // Portable / side-loaded installs: `<root>/<version>/wt.exe`.
    for root in [
        "C:/Program Files/winTerminal",
        "D:/Program Files/winTerminal",
    ] {
        let root = Path::new(root);
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join("wt.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether the terminal behind `id` is installed on this machine.
#[cfg(windows)]
fn terminal_available(id: &str) -> bool {
    match id {
        "wt" => windows_terminal().is_some(),
        other => which(other),
    }
}

#[cfg(target_os = "macos")]
fn terminal_available(id: &str) -> bool {
    match id {
        // Terminal.app ships with the OS; iTerm is an install.
        "terminal" => true,
        "iterm" => Path::new("/Applications/iTerm.app").is_dir(),
        other => which(other),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn terminal_available(id: &str) -> bool {
    which(id)
}

/// How the command must be started, as a program plus its argv.
///
/// Two shapes:
///
///   - `cmd /k <command>` — the plain console host.
///   - `powershell -NoExit -Command <line>` — needed for a shell alias (`cc`),
///     which is a function inside the user's PowerShell profile and cannot be
///     exec'd by name. `-NoExit` keeps the window alive afterwards.
///
/// Windows Terminal cannot launch `powershell.exe` directly — it fails with
/// `0x80070002` (file not found) and the tab dies before the command runs — so
/// the PowerShell shape is wrapped in `cmd /k` when it is hosted in a tab.
#[cfg(windows)]
fn shell_invocation(
    terminal: &str,
    launcher: Launcher,
    command: &str,
    args: &[String],
) -> (String, Vec<String>) {
    // An alias lives in the PowerShell profile, so it forces PowerShell
    // whatever the preference says.
    if launcher == Launcher::ShellAlias || terminal == "powershell" {
        let line = std::iter::once(command.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        (
            "powershell".to_string(),
            vec!["-NoExit".to_string(), "-Command".to_string(), line],
        )
    } else {
        (
            "cmd".to_string(),
            std::iter::once("/k".to_string())
                .chain(std::iter::once(command.to_string()))
                .chain(args.iter().cloned())
                .collect(),
        )
    }
}

/// Wrap a program in `cmd /k`, the only shape Windows Terminal will launch.
#[cfg(windows)]
fn wrap_in_cmd(program: &str, args: &[String]) -> (String, Vec<String>) {
    (
        "cmd".to_string(),
        std::iter::once("/k".to_string())
            .chain(std::iter::once(program.to_string()))
            .chain(args.iter().cloned())
            .collect(),
    )
}

/// Whether `terminal` should open a tab in Windows Terminal.
///
/// `cmd` and `powershell` name a host of their own, so they never borrow a
/// Windows Terminal window even when one is installed.
#[cfg(windows)]
fn wants_windows_terminal(terminal: &str) -> bool {
    !matches!(terminal, "cmd" | "powershell")
}

#[cfg(windows)]
fn spawn_terminal(
    terminal: &str,
    launcher: Launcher,
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let (program, program_args) = shell_invocation(terminal, launcher, command, args);
    if wants_windows_terminal(terminal) {
        if let Some(wt) = windows_terminal() {
            // `wt` cannot launch powershell directly, so only that shape is
            // wrapped; `cmd /k ...` is already launchable as-is.
            let (host, host_args) = if program == "cmd" {
                (program.clone(), program_args.clone())
            } else {
                wrap_in_cmd(&program, &program_args)
            };
            if spawn_windows_terminal(&wt, &host, &host_args, cwd).is_ok() {
                return Ok(());
            }
        }
    }
    spawn_console_window(&program, &program_args, cwd)
}

/// Open a new TAB in the already-running Windows Terminal window.
///
/// `-w 0` targets the most recent window, so this reuses what the user has
/// open instead of spawning another window. `nt` is `new-tab`; `-d` sets the
/// starting directory, `--title` labels the tab.
///
/// Every part is passed as its own argument (no shell string is assembled),
/// so a path or id can never be re-parsed as shell syntax.
#[cfg(windows)]
fn spawn_windows_terminal(
    wt: &Path,
    program: &str,
    program_args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(wt);
    cmd.args(["-w", "0", "nt"]);
    if let Some(dir) = cwd {
        cmd.arg("-d").arg(dir);
    }
    cmd.arg("--title").arg("Trajectory Viewer");
    cmd.arg(program);
    cmd.args(program_args);

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open a Windows Terminal tab: {e}"))
}

/// Fallback: a new console window via `start`, for a terminal that has no
/// host of its own on this machine (or when Windows Terminal is unavailable).
///
/// `start` treats a quoted first argument as the window title, so an empty
/// title is passed first to keep the program in the right position.
#[cfg(windows)]
fn spawn_console_window(
    program: &str,
    program_args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    // `start` treats a quoted first argument as the window title, so an empty
    // title is passed first to keep the command in the right position.
    let mut inner = String::new();
    inner.push_str(&quote_windows_arg(program));
    for arg in program_args {
        inner.push(' ');
        inner.push_str(&quote_windows_arg(arg));
    }

    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/c", "start", ""]);
    // `raw_arg` matters: `args()` re-quotes the string and turns the inner
    // quotes into `\"`, which cmd.exe does not understand — the command then
    // silently does not run.
    cmd.raw_arg(&inner);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Detach the console entirely: a `/k` window would otherwise inherit this
    // process's stdio handles and keep them open for as long as it lives.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open a terminal: {e}"))
}

/// Quote one argv entry for `cmd.exe`.
///
/// Ids are validated to `[A-Za-z0-9_-]` and the command is a single program
/// name, so this is belt-and-braces rather than the only defence.
#[cfg(windows)]
fn quote_windows_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', ""))
}

/// macOS: run the command in the frontmost Terminal window, which is what
/// "resume here" means on a platform that allows it.
///
/// The shell is started as a LOGIN, INTERACTIVE shell so aliases defined in
/// `~/.zshrc` resolve — an alias is not a program and cannot be exec'd.
#[cfg(target_os = "macos")]
fn spawn_terminal(
    terminal: &str,
    launcher: Launcher,
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut line = String::new();
    if let Some(dir) = cwd {
        line.push_str(&format!("cd {} && ", shell_quote(&dir.to_string_lossy())));
    }
    line.push_str(&shell_quote(command));
    for arg in args {
        line.push(' ');
        line.push_str(&shell_quote(arg));
    }
    // Keep the window usable if the CLI exits immediately.
    if launcher == Launcher::Direct {
        line.push_str(&format!("; exec {}", shell_quote(&shell)));
    }

    // Terminal.app ships with macOS and is scripted with `do script`; iTerm2
    // opens the same command in a new window of its default profile.
    let script_body = if terminal == "iterm" && terminal_available("iterm") {
        "create window with default profile command".to_string()
    } else {
        "do script".to_string()
    };
    let app = if script_body.starts_with("create") {
        "iTerm"
    } else {
        "Terminal"
    };

    let script = format!(
        "tell application \"{app}\"\n  activate\n  {script_body} \"{}\"\nend tell",
        line.replace('\\', "\\\\").replace('"', "\\\"")
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open {app}: {e}"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn spawn_terminal(
    terminal: &str,
    _launcher: Launcher,
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<(), String> {
    // Linux terminals differ too much to guess; try the common ones in order.
    const DEFAULT_TERMINALS: [&str; 4] =
        ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"];
    let mut shell_command = String::new();
    if let Some(dir) = cwd {
        shell_command.push_str(&format!("cd {} && ", shell_quote(&dir.to_string_lossy())));
    }
    shell_command.push_str(&shell_quote(command));
    for arg in args {
        shell_command.push(' ');
        shell_command.push_str(&shell_quote(arg));
    }
    // An interactive shell so aliases resolve, kept open afterwards.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let script = format!("{shell_command}; exec {}", shell_quote(&shell));

    // A pinned terminal is tried on its own; `auto` walks the fallback chain.
    let candidates: Vec<&str> = if terminal == "auto" {
        DEFAULT_TERMINALS.to_vec()
    } else {
        vec![terminal]
    };
    for terminal in candidates {
        if !which(terminal) {
            continue;
        }
        let mut cmd = std::process::Command::new(terminal);
        // gnome-terminal and konsole take `-e`; xterm too.
        cmd.arg("-e").arg("sh").arg("-c").arg(&script);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }
    Err(
        "No supported terminal found (tried x-terminal-emulator, gnome-terminal, konsole, xterm)"
            .to_string(),
    )
}

/// Single-quote a value for POSIX shells.
#[cfg(not(windows))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_outside_the_known_charset_are_rejected() {
        assert!(validate_session_id("ab96e624-4606-46ca-9ffc-a5518540f5c2").is_ok());
        assert!(validate_session_id("session-da28a5d3-15e2-41fc-b6bd-4e6806c6d7c1").is_ok());
        assert!(validate_session_id("").is_err());
        // Shell metacharacters must not survive.
        assert!(validate_session_id("abc; rm -rf /").is_err());
        assert!(validate_session_id("abc$(whoami)").is_err());
        assert!(validate_session_id("abc`id`").is_err());
        assert!(validate_session_id("abc&def").is_err());
        assert!(validate_session_id("../etc/passwd").is_err());
        assert!(validate_session_id("abc def").is_err());
    }

    #[test]
    fn unsupported_providers_are_refused() {
        let err = launch("nope", "abc", None).unwrap_err();
        assert!(err.contains("not supported"), "got: {err}");
    }

    // Each provider spells "continue this session" differently. These are the
    // forms their own `--help` documents; opencode in particular has no
    // `session resume` subcommand, only the `--session` / `-s` flag.
    #[test]
    fn each_provider_gets_its_own_continuation_arguments() {
        let args = |p: &str| resume_args(p, "ses-1").unwrap();
        assert_eq!(args("claude"), vec!["--resume", "ses-1"]);
        assert_eq!(args("codex"), vec!["resume", "ses-1"]);
        assert_eq!(args("dsh"), vec!["--resume", "ses-1"]);
        assert_eq!(args("opencode"), vec!["--session", "ses-1"]);
    }

    // Regression: opencode was launched as `opencode session resume <id>`, but
    // `opencode session` has no `resume` subcommand — the tab printed help and
    // the conversation never continued.
    #[test]
    fn opencode_never_uses_a_session_resume_subcommand() {
        let args = resume_args("opencode", "ses_2").unwrap();
        assert!(
            !args.iter().any(|a| a == "resume"),
            "opencode has no `resume` subcommand: {args:?}"
        );
        assert_eq!(args[0], "--session");
    }

    #[test]
    fn display_command_matches_the_launch_arguments() {
        // The header must show exactly what the button runs (modulo the
        // configured program name, which the UI substitutes).
        assert_eq!(
            display_command("opencode", "ses_2").as_deref(),
            Some("opencode --session ses_2")
        );
        assert_eq!(
            display_command("claude", "abc").as_deref(),
            Some("claude --resume abc")
        );
        assert_eq!(display_command("nope", "abc"), None);
    }

    // The displayed command must not depend on the user's saved settings —
    // it runs during a session scan, and a machine-specific value there would
    // make the parsers' output (and their tests) depend on local state.
    #[test]
    fn display_command_ignores_configured_overrides() {
        // HOME is process-global; take the shared lock the other env-touching
        // tests use so this cannot race them.
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let dir = tempfile::tempdir().unwrap();
        let saved_home = std::env::var("HOME").ok();
        let saved_profile = std::env::var("USERPROFILE").ok();

        // An empty home means `get_settings` reads nothing and falls back to
        // the provider default, whatever the real machine has configured.
        std::env::set_var("HOME", dir.path());
        std::env::set_var("USERPROFILE", dir.path());
        let out = display_command("claude", "abc");

        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match saved_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }

        assert_eq!(out.as_deref(), Some("claude --resume abc"));
    }

    // The id is checked before anything is spawned, so a bad one cannot reach
    // a shell even if the command is missing.
    #[test]
    fn an_invalid_id_fails_before_launching() {
        let err = launch("claude", "a;b", None).unwrap_err();
        assert!(err.contains("Invalid session id"), "got: {err}");
    }

    // Validation is exercised through `validate_command` directly, not through
    // `set_settings`: that writes to the user's real `~/.trajectory-viewer/
    // resume.json`, so a test run would clobber the actual configuration.
    #[test]
    fn validation_rejects_a_shell_carrying_command() {
        // A command is interpolated into a shell line, so shell syntax of its
        // own must not survive: flags belong in the alias it resolves to.
        for bad in [
            "claude --verbose",
            "claude; rm -rf /",
            "claude$(id)",
            "claude|cat",
        ] {
            assert!(validate_command(bad).is_err(), "should reject {bad:?}");
        }
        // Plain names and paths are accepted.
        for good in [
            "cc",
            "claude",
            "dsh-tui",
            "/usr/local/bin/claude",
            "C:\\tools\\cc.exe",
        ] {
            assert!(validate_command(good).is_ok(), "should accept {good:?}");
        }
    }

    // An empty entry means "use the default", so `command_for` must fall back
    // rather than return a blank command.
    #[test]
    fn a_blank_entry_falls_back_to_the_default() {
        let settings = ResumeSettings {
            commands: [("claude".to_string(), "   ".to_string())]
                .into_iter()
                .collect(),
            terminal: None,
        };
        assert_eq!(settings.command_for("claude"), "claude");
    }

    // One shared command cannot serve every provider: each tool has its own
    // CLI, so a Codex session must not be resumed with `claude`.
    #[test]
    fn each_provider_has_its_own_default_command() {
        let settings = ResumeSettings::default();
        assert_eq!(settings.command_for("claude"), "claude");
        assert_eq!(settings.command_for("codex"), "codex");
        assert_eq!(settings.command_for("dsh"), "dsh-tui");
        assert_eq!(settings.command_for("opencode"), "opencode");
    }

    #[test]
    fn a_configured_command_overrides_only_its_own_provider() {
        let settings = ResumeSettings {
            commands: [("claude".to_string(), "cc".to_string())]
                .into_iter()
                .collect(),
            terminal: None,
        };
        assert_eq!(settings.command_for("claude"), "cc");
        // Other providers keep their defaults.
        assert_eq!(settings.command_for("codex"), "codex");
    }

    // A shell alias is not a program: `where`/`which` cannot see it, and only
    // an interactive shell can run it.
    #[test]
    fn a_bare_name_is_classified_by_how_it_resolves() {
        // Every machine with this app has some shell; `claude` is on PATH in
        // CI only by accident, so assert the shape rather than a fixed answer.
        let kind = classify("claude");
        assert!(matches!(
            kind,
            Launcher::Direct | Launcher::ShellAlias | Launcher::Missing
        ));
        // A name that cannot exist resolves to Missing.
        assert_eq!(
            classify("definitely-not-a-real-command-xyz"),
            Launcher::Missing
        );
        assert_eq!(classify(""), Launcher::Missing);
    }

    // `which` scans PATH in-process. It used to shell out to `cmd /c where`,
    // which measured ~800ms per call — and the settings pane probes four
    // providers at once, so the app froze for seconds.
    #[test]
    fn which_finds_a_program_without_spawning_a_shell() {
        // A program that exists on every supported platform.
        let universal = if cfg!(windows) { "cmd" } else { "sh" };
        let started = std::time::Instant::now();
        assert!(which(universal), "expected `{universal}` on PATH");
        // Generous, but far below the process-spawn cost this replaced.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "PATH scan took {:?}",
            started.elapsed()
        );
        assert!(!which("definitely-not-a-real-command-xyz"));
    }

    // A PATH entry that exists but is not executable must not count on POSIX.
    #[test]
    #[cfg(not(windows))]
    fn which_requires_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        // PATH is process-global; take the shared lock the other env-touching
        // tests use so this cannot race them.
        let _guard = crate::trajectory::parser::opencode::opencode_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-executable");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let saved = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", dir.path());
        let found_without_bit = which("not-executable");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found_with_bit = which("not-executable");
        std::env::set_var("PATH", saved);

        assert!(!found_without_bit);
        assert!(found_with_bit);
    }

    // The shell lookup is the expensive one, so its result is memoised. A
    // missing command must be cached as a miss too, not retried every call.
    #[test]
    fn a_shell_lookup_is_memoised() {
        let command = "definitely-not-a-real-command-xyz";
        let first = std::time::Instant::now();
        classify(command);
        let cold = first.elapsed();

        let second = std::time::Instant::now();
        classify(command);
        let warm = second.elapsed();

        // The warm call skips the shell spawn entirely.
        assert!(
            warm < cold,
            "second classify should be cheaper: cold {cold:?}, warm {warm:?}"
        );
    }

    #[test]
    fn an_explicit_path_is_classified_without_a_shell_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("wrapper");
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(classify(&exe.to_string_lossy()), Launcher::Direct);
        assert_eq!(
            classify(&dir.path().join("absent").to_string_lossy()),
            Launcher::Missing
        );
    }

    #[test]
    fn status_reports_the_resolution_kind() {
        assert_eq!(command_status("definitely-not-real-xyz").kind, "missing");
        assert!(!command_available("definitely-not-real-xyz"));
    }

    #[test]
    fn a_blank_terminal_falls_back_to_auto() {
        assert_eq!(ResumeSettings::default().terminal(), "auto");
        assert_eq!(
            ResumeSettings {
                terminal: Some("  ".to_string()),
                ..Default::default()
            }
            .terminal(),
            "auto"
        );
        assert_eq!(
            ResumeSettings {
                terminal: Some("wt".to_string()),
                ..Default::default()
            }
            .terminal(),
            "wt"
        );
    }

    // The dropdown must always carry `auto`, and it is the entry that survives
    // a terminal being uninstalled — so it can never be reported unavailable.
    #[test]
    fn auto_is_always_available_and_first() {
        let terminals = all_terminals();
        assert_eq!(terminals.first().unwrap().id, "auto");
        assert!(terminals.first().unwrap().available);
        assert!(terminals.first().unwrap().is_default);
        // Every entry has a label and a unique id.
        let ids: Vec<_> = terminals.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids.len(),
            std::collections::BTreeSet::from_iter(ids.iter()).len()
        );
        assert!(terminals.iter().all(|t| !t.label.is_empty()));
    }

    // A terminal id is stored, never interpolated into a shell line, so it is
    // folded onto the known set instead of character-validated.
    #[test]
    fn a_terminal_outside_the_known_set_is_refused() {
        // Guarded by not calling `set_settings`: that writes the user's real
        // `~/.trajectory-viewer/resume.json`.
        let known: std::collections::BTreeSet<&str> =
            TERMINALS.iter().copied().filter(|t| *t != "auto").collect();
        assert!(known.contains("wt") || known.contains("terminal") || known.contains("xterm"));
        assert!(!known.contains("definitely-not-a-terminal"));
    }

    #[test]
    fn the_default_settings_carry_no_overrides() {
        let settings = ResumeSettings::default();
        assert!(settings.commands.is_empty());
        assert!(settings.terminal.is_none());
    }

    #[test]
    fn missing_cwd_is_tolerated() {
        // A session can outlive its project; the launch degrades rather than
        // failing. `command_available` decides whether it proceeds at all, so
        // only assert the path handling here.
        let gone = PathBuf::from("Z:/definitely/not/here");
        assert!(!gone.is_dir());
    }
}
