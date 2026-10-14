// ── Update Check ─────────────────────────────────────────────────────────
//
// Asks GitHub for this project's newest release and compares it with the
// version this build was compiled as. Deliberately shallow: it reports whether
// a newer release exists and hands the user a link. It does not download,
// verify or install anything — that needs a signed Tauri updater manifest
// published by CI, which this project does not produce yet.
//
// The endpoint is the GitHub REST API (`/releases/latest`, which never returns
// drafts or prereleases) rather than a Tauri `latest.json` manifest, because
// the release workflow publishes no manifest to read.
//
// Used from the Settings → About tab. cc-switch does the same job with
// tauri-plugin-updater + a `latest.json`; this is the smaller first step.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Repository whose releases are offered as updates.
const GITHUB_REPO: &str = "ZH-TX3/trajectory-viewer";

/// How long a check stays fresh enough to skip the next automatic one.
const AUTO_CHECK_INTERVAL_MS: i64 = 6 * 60 * 60 * 1000;

/// Request timeout. Fails fast when the machine is offline.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// The version the running build was compiled as (mirrors tauri.conf.json).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Newest release's own page — the "go download" target.
pub fn latest_release_url() -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/latest")
}

/// Whether `url` may be handed to the system browser.
///
/// Kept narrow on purpose: the open command exists to serve the update card,
/// not to be a general-purpose URL opener for the webview.
pub fn is_allowed_external_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{GITHUB_REPO}/"))
}

/// The subset of GitHub's release payload this feature reads.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
}

/// Update preferences, persisted to `~/.trajectory-viewer/update.json`.
///
/// Stored separately from `settings.json` (backup prefs) because that file is
/// rewritten wholesale by `set_backup_settings`, which would drop these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Check quietly when the app starts.
    pub auto_check: bool,
    /// Unix ms of the last check (any outcome), used to throttle auto-checks.
    pub last_checked_at: Option<i64>,
    /// A version the user asked not to be told about again.
    pub skipped_version: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            auto_check: true,
            last_checked_at: None,
            skipped_version: None,
        }
    }
}

/// Outcome of one check, shaped for the settings UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    /// Newest published version, when one could be read.
    pub latest_version: Option<String>,
    /// True only when `latest_version` is strictly newer *and* not skipped.
    pub has_update: bool,
    /// A newer release exists but the user asked not to be told about it.
    pub skipped: bool,
    pub release_url: String,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    /// Unix ms this check completed.
    pub checked_at: i64,
}

/// Strip a leading `v` and parse as semantic version.
fn parse_version(raw: &str) -> Option<semver::Version> {
    semver::Version::parse(raw.trim().trim_start_matches('v')).ok()
}

/// Whether `remote` is a strictly newer version than `local`.
///
/// `None` when either side is not parseable — the caller reports that rather
/// than guessing (a non-semver tag would otherwise read as "no update").
pub fn is_newer(remote: &str, local: &str) -> Option<bool> {
    Some(parse_version(remote)? > parse_version(local)?)
}

/// Whether an automatic check is due. Manual checks ignore this.
pub fn auto_check_due(settings: &UpdateSettings, now_ms: i64) -> bool {
    if !settings.auto_check {
        return false;
    }
    match settings.last_checked_at {
        None => true,
        Some(last) => now_ms.saturating_sub(last) >= AUTO_CHECK_INTERVAL_MS,
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn settings_path() -> PathBuf {
    crate::backup::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".trajectory-viewer")
        .join("update.json")
}

/// Read persisted update preferences (defaults when absent or unreadable).
pub fn get_settings() -> UpdateSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<UpdateSettings>(&raw).ok())
        .unwrap_or_default()
}

/// Persist update preferences.
pub fn set_settings(settings: &UpdateSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create data dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Cannot serialize update settings: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Cannot save update settings: {e}"))
}

/// Fetch the newest release from GitHub. Blocking — call it off the main thread.
fn fetch_latest_release() -> Result<GithubRelease, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("Cannot build HTTP client: {e}"))?;

    let response = client
        .get(&url)
        .header(
            reqwest::header::USER_AGENT,
            format!("trajectory-viewer/{}", current_version()),
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|e| format!("Cannot reach GitHub: {e}"))?;

    let status = response.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err("GitHub rate-limited this check — try again later.".to_string());
    }
    if status.as_u16() == 404 {
        return Err("This project has no published releases yet.".to_string());
    }
    if !status.is_success() {
        return Err(format!("GitHub returned HTTP {status}"));
    }

    response
        .json::<GithubRelease>()
        .map_err(|e| format!("Unexpected GitHub response: {e}"))
}

/// Run one check against GitHub, unconditionally, and record when it happened.
pub fn check() -> Result<UpdateCheck, String> {
    let release = fetch_latest_release()?;
    let current = current_version().to_string();
    let latest = release.tag_name.trim_start_matches('v').to_string();

    let newer = is_newer(&latest, &current)
        .ok_or_else(|| format!("Cannot compare '{latest}' with '{current}'"))?;

    let mut settings = get_settings();
    let skipped = settings.skipped_version.as_deref() == Some(latest.as_str());
    let checked_at = now_ms();
    settings.last_checked_at = Some(checked_at);
    // A recorded check is best-effort: failing to persist only loses throttling.
    let _ = set_settings(&settings);

    Ok(UpdateCheck {
        current_version: current,
        latest_version: Some(latest),
        has_update: newer && !skipped,
        skipped: newer && skipped,
        release_url: release.html_url.unwrap_or_else(latest_release_url),
        release_notes: release.body,
        published_at: release.published_at,
        checked_at,
    })
}

/// Check only when the user enabled it and the throttle window has passed.
///
/// `Ok(None)` means "not due" — distinct from a failed check, so the UI can
/// stay quiet about a skip without swallowing a real error.
pub fn auto_check() -> Result<Option<UpdateCheck>, String> {
    if !auto_check_due(&get_settings(), now_ms()) {
        return Ok(None);
    }
    check().map(Some)
}

/// Remember that a newer release exists but the user does not want to hear
/// about it again. `None` clears the skip.
pub fn set_skipped_version(version: Option<String>) -> Result<(), String> {
    let mut settings = get_settings();
    settings.skipped_version = version.filter(|v| !v.trim().is_empty());
    set_settings(&settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// Shares opencode's env lock: these tests mutate the process-global HOME.
    fn env_lock() -> &'static Mutex<()> {
        crate::trajectory::parser::opencode::opencode_env_lock()
    }

    fn with_home(home: &Path, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap();
        let original_home = std::env::var_os("HOME");
        #[allow(deprecated)]
        std::env::set_var("HOME", home);
        f();
        match original_home {
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

    #[test]
    fn version_comparison_is_semantic() {
        assert_eq!(is_newer("0.6.0", "0.5.5"), Some(true));
        assert_eq!(is_newer("v0.6.0", "0.5.5"), Some(true));
        assert_eq!(is_newer("0.5.5", "0.5.5"), Some(false));
        assert_eq!(is_newer("0.5.4", "0.5.5"), Some(false));
        assert_eq!(is_newer("0.10.0", "0.9.9"), Some(true));
        // Numeric, not lexicographic: 10 > 9 despite '1' < '9'.
        assert_eq!(is_newer("1.0.0", "0.99.99"), Some(true));
    }

    #[test]
    fn prerelease_sorts_below_its_release() {
        assert_eq!(is_newer("0.6.0-beta.1", "0.5.5"), Some(true));
        assert_eq!(is_newer("0.6.0-beta.1", "0.6.0"), Some(false));
        assert_eq!(is_newer("0.6.0", "0.6.0-beta.1"), Some(true));
    }

    #[test]
    fn unparseable_tags_are_not_guessed() {
        assert_eq!(is_newer("nightly", "0.5.5"), None);
        assert_eq!(is_newer("0.6.0", "not-a-version"), None);
    }

    #[test]
    fn auto_check_respects_the_toggle() {
        let off = UpdateSettings {
            auto_check: false,
            ..Default::default()
        };
        assert!(!auto_check_due(&off, 1_000_000_000_000));
    }

    #[test]
    fn auto_check_throttles_for_the_interval() {
        let now = 1_700_000_000_000i64;
        let never = UpdateSettings::default();
        assert!(auto_check_due(&never, now));

        let just_checked = UpdateSettings {
            last_checked_at: Some(now - 60_000),
            ..Default::default()
        };
        assert!(!auto_check_due(&just_checked, now));

        let stale = UpdateSettings {
            last_checked_at: Some(now - AUTO_CHECK_INTERVAL_MS - 1),
            ..Default::default()
        };
        assert!(auto_check_due(&stale, now));
    }

    #[test]
    fn settings_roundtrip_through_disk() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        with_home(&home, || {
            assert_eq!(get_settings(), UpdateSettings::default());

            let saved = UpdateSettings {
                auto_check: false,
                last_checked_at: Some(123),
                skipped_version: Some("0.9.0".to_string()),
            };
            set_settings(&saved).unwrap();
            assert_eq!(get_settings(), saved);
            assert!(home.join(".trajectory-viewer/update.json").exists());

            set_skipped_version(None).unwrap();
            assert_eq!(get_settings().skipped_version, None);
        });
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".trajectory-viewer")).unwrap();
        with_home(&home, || {
            std::fs::write(home.join(".trajectory-viewer/update.json"), "{}").unwrap();
            assert_eq!(get_settings(), UpdateSettings::default());
        });
    }

    #[test]
    fn only_this_repos_pages_may_be_opened() {
        assert!(is_allowed_external_url(&latest_release_url()));
        assert!(is_allowed_external_url(
            "https://github.com/ZH-TX3/trajectory-viewer/releases/tag/v0.5.5"
        ));
        assert!(!is_allowed_external_url("https://example.com/evil"));
        assert!(!is_allowed_external_url(
            "https://github.com/other/repo/releases/latest"
        ));
        assert!(!is_allowed_external_url("javascript:alert(1)"));
    }

    /// Live network smoke test against the real GitHub API, so the request
    /// shape and response fields are exercised against the actual service.
    /// Opt-in: `cargo test --lib -- --ignored`. Note it records a check, which
    /// touches the real `~/.trajectory-viewer/update.json`.
    #[test]
    #[ignore]
    fn real_github_check_reads_the_latest_release() {
        let check = check().expect("GitHub check failed");
        let latest = check
            .latest_version
            .clone()
            .expect("no latest version reported");
        assert!(
            parse_version(&latest).is_some(),
            "unparseable tag: {latest}"
        );
        assert_eq!(check.current_version, current_version());
        assert!(is_allowed_external_url(&check.release_url));
        assert!(check.has_update || latest == current_version());
    }
}
