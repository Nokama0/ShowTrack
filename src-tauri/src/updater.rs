//! GitHub-releases updater. Checks the repo's latest release, compares its
//! tag against the running version, and (on desktop) downloads and launches
//! the installer.

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, Result};

const LATEST_URL: &str = "https://api.github.com/repos/Nokama0/ShowTrack/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/Nokama0/ShowTrack/releases";

/// The live URL, unless a test points the check at a mock server.
fn latest_url() -> String {
    std::env::var("SHOWTRACK_UPDATE_URL").unwrap_or_else(|_| LATEST_URL.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub installer_url: Option<String>,
    pub page_url: String,
    /// "setup" updates via the installer; "portable" replaces the running exe
    pub kind: String,
}

/// An NSIS install leaves uninstall.exe beside the app. A bare exe run from
/// anywhere else (including a dev target dir) updates by replacing itself.
pub fn is_portable() -> bool {
    #[cfg(target_os = "android")]
    {
        false
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| !d.join("uninstall.exe").exists()))
            .unwrap_or(false)
    }
}

/// "v1.2.3" or "1.2.3" into a comparable triple; anything else is None.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Decide from a release JSON whether it is newer than `current`, picking the
/// asset that matches how this copy runs.
pub fn parse_release(release: &Value, current: (u64, u64, u64), portable: bool) -> Option<UpdateInfo> {
    let tag = release["tag_name"].as_str()?;
    let latest = parse_version(tag)?;
    if latest <= current {
        return None;
    }
    let suffix = if portable { "-portable.exe" } else { "-setup.exe" };
    let installer_url = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|a| {
                a["name"].as_str().is_some_and(|n| n.ends_with(suffix))
            })
        })
        .and_then(|a| a["browser_download_url"].as_str())
        .map(str::to_string);
    Some(UpdateInfo {
        version: tag.trim_start_matches('v').to_string(),
        notes: release["body"].as_str().unwrap_or("").chars().take(600).collect(),
        installer_url,
        page_url: release["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
        kind: (if portable { "portable" } else { "setup" }).to_string(),
    })
}

pub fn current_version() -> (u64, u64, u64) {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

pub async fn check(http: &reqwest::Client) -> Result<Option<UpdateInfo>> {
    let resp = http
        .get(latest_url())
        .header("User-Agent", "ShowTrack")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| AppError::msg(format!("GitHub unreachable: {e}")))?;
    if resp.status().as_u16() == 404 {
        return Err(AppError::msg(
            "no releases found on GitHub (the repo may be private or have no releases yet)",
        ));
    }
    let release: Value = resp
        .error_for_status()
        .map_err(|e| AppError::msg(e.to_string()))?
        .json()
        .await
        .map_err(|e| AppError::msg(e.to_string()))?;
    Ok(parse_release(&release, current_version(), is_portable()))
}

/// Replace the running exe with `new_exe` and return the path to relaunch.
/// A mapped image cannot be overwritten but CAN be renamed, so the running
/// file steps aside as "<exe>.old" (cleaned up on a later launch).
pub fn swap_running_exe(new_exe: &std::path::Path) -> Result<std::path::PathBuf> {
    let cur = std::env::current_exe()
        .map_err(|e| AppError::msg(format!("cannot locate the running exe: {e}")))?;
    let old = std::path::PathBuf::from(format!("{}.old", cur.display()));
    let _ = std::fs::remove_file(&old); // stale leftover from an earlier update
    std::fs::rename(&cur, &old)
        .map_err(|e| AppError::msg(format!("could not step the running exe aside: {e}")))?;
    if let Err(e) = std::fs::copy(new_exe, &cur) {
        let _ = std::fs::rename(&old, &cur); // roll back, app stays usable
        return Err(AppError::msg(format!("could not place the new exe: {e}")));
    }
    Ok(cur)
}

/// Remove the "<exe>.old" a portable update leaves behind. The previous
/// process may still be exiting on the first launch; that failure is fine,
/// the next launch gets it.
pub fn cleanup_old_exe() {
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(std::path::PathBuf::from(format!("{}.old", cur.display())));
    }
}

/// Download the update binary to the temp dir and return its path.
pub async fn download_installer(http: &reqwest::Client, url: &str) -> Result<std::path::PathBuf> {
    let bytes = http
        .get(url)
        .header("User-Agent", "ShowTrack")
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| AppError::msg(format!("download failed: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::msg(e.to_string()))?
        .bytes()
        .await
        .map_err(|e| AppError::msg(e.to_string()))?;
    let path = std::env::temp_dir().join("ShowTrack-update.exe");
    tokio::fs::write(&path, &bytes).await?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_parsing_and_ordering() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("nightly"), None);
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
    }

    #[test]
    fn release_newer_than_current_yields_update() {
        let rel = json!({
            "tag_name": "v0.3.0",
            "body": "Fixes and features",
            "html_url": "https://github.com/Nokama0/ShowTrack/releases/tag/v0.3.0",
            "assets": [
                {"name": "ShowTrack_0.3.0_x64-setup.exe",
                 "browser_download_url": "https://example.com/ShowTrack_0.3.0_x64-setup.exe"},
                {"name": "app-universal-release.apk",
                 "browser_download_url": "https://example.com/app.apk"}
            ]
        });
        let up = parse_release(&rel, (0, 2, 0), false).unwrap();
        assert_eq!(up.version, "0.3.0");
        assert_eq!(up.kind, "setup");
        assert_eq!(up.installer_url.as_deref(),
                   Some("https://example.com/ShowTrack_0.3.0_x64-setup.exe"));
        // same or older release is not an update
        assert!(parse_release(&rel, (0, 3, 0), false).is_none());
        assert!(parse_release(&rel, (1, 0, 0), false).is_none());
    }

    #[test]
    fn portable_copy_gets_the_portable_asset() {
        let rel = json!({
            "tag_name": "v0.3.0",
            "assets": [
                {"name": "ShowTrack_0.3.0_x64-setup.exe",
                 "browser_download_url": "https://example.com/setup.exe"},
                {"name": "ShowTrack_0.3.0_x64-portable.exe",
                 "browser_download_url": "https://example.com/portable.exe"}
            ]
        });
        let up = parse_release(&rel, (0, 2, 0), true).unwrap();
        assert_eq!(up.kind, "portable");
        assert_eq!(up.installer_url.as_deref(), Some("https://example.com/portable.exe"));
        // a release published without a portable asset falls back to the page link
        let setup_only = json!({
            "tag_name": "v0.3.0",
            "assets": [{"name": "ShowTrack_0.3.0_x64-setup.exe",
                        "browser_download_url": "https://example.com/setup.exe"}]
        });
        assert!(parse_release(&setup_only, (0, 2, 0), true).unwrap().installer_url.is_none());
    }

    #[test]
    fn release_without_installer_asset_still_reports() {
        let rel = json!({"tag_name": "v9.9.9", "assets": []});
        let up = parse_release(&rel, (0, 1, 0), false).unwrap();
        assert!(up.installer_url.is_none());
        assert_eq!(up.page_url, RELEASES_PAGE);
    }
}
