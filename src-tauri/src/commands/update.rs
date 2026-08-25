// update.rs — update checks against the project's release sources. The user
// picks the source (GitHub upstream or the GitCode mirror) from the settings
// About panel via the persisted `updateSource` general setting.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::config::{ConfigStore, UpdateSource};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    latest_version: String,
    update_available: bool,
    release_url: String,
    release_title: Option<String>,
    release_notes: Option<String>,
    published_at: Option<String>,
}

/// Common fields shared by the GitHub and GitCode latest-release payloads.
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

impl UpdateSource {
    fn latest_api_url(self) -> String {
        match self {
            Self::Github => {
                "https://api.github.com/repos/muutot/Clipboard/releases/latest".to_owned()
            }
            Self::Gitcode => {
                "https://api.gitcode.com/api/v5/repos/m2u/Clipboard/releases/latest".to_owned()
            }
        }
    }

    fn tag_api_url(self, tag: &str) -> String {
        let encoded = urlencoding::encode(tag);
        match self {
            Self::Github => {
                format!("https://api.github.com/repos/muutot/Clipboard/releases/tags/{encoded}")
            }
            Self::Gitcode => {
                format!("https://api.gitcode.com/api/v5/repos/m2u/Clipboard/releases/tags/{encoded}")
            }
        }
    }

    fn release_page(self) -> String {
        match self {
            Self::Github => "https://github.com/muutot/Clipboard/releases".to_owned(),
            Self::Gitcode => "https://gitcode.com/m2u/Clipboard/releases".to_owned(),
        }
    }
}

async fn fetch_release(url: String) -> Result<Release, String> {
    let client = reqwest::Client::builder()
        .user_agent("clipboard-desktop")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("create client: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request release: {e}"))?
        .error_for_status()
        .map_err(|e| format!("fetch release failed: {e}"))?;
    let body = response
        .text()
        .await
        .map_err(|e| format!("read response: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("parse release: {e}"))
}

#[tauri::command]
pub async fn check_for_update(
    config: tauri::State<'_, Mutex<ConfigStore>>,
) -> Result<UpdateInfo, String> {
    let (source, local_only) = {
        let guard = config.lock().map_err(|e| format!("config lock: {e}"))?;
        (guard.update_source(), guard.privacy_local_only())
    };
    if local_only {
        return Err("update check is disabled: local-only mode is on".to_owned());
    }
    let release = fetch_release(source.latest_api_url()).await?;

    let current_version = env!("CARGO_PKG_VERSION").to_owned();
    let latest_version = release.tag_name.trim_start_matches('v').to_owned();
    let update_available = is_newer(&latest_version, &current_version);

    Ok(UpdateInfo {
        current_version,
        latest_version,
        update_available,
        release_url: release.html_url.unwrap_or_else(|| source.release_page()),
        release_title: release.name,
        release_notes: release.body,
        published_at: release.published_at.or(release.created_at),
    })
}

#[tauri::command]
pub async fn get_release(
    config: tauri::State<'_, Mutex<ConfigStore>>,
    version: String,
) -> Result<UpdateInfo, String> {
    let (source, local_only) = {
        let guard = config.lock().map_err(|e| format!("config lock: {e}"))?;
        (guard.update_source(), guard.privacy_local_only())
    };
    if local_only {
        return Err("release notes are unavailable: local-only mode is on".to_owned());
    }
    let tag = if version.starts_with('v') {
        version
    } else {
        format!("v{version}")
    };
    let release = fetch_release(source.tag_api_url(&tag)).await?;

    let current_version = env!("CARGO_PKG_VERSION").to_owned();
    Ok(UpdateInfo {
        current_version,
        latest_version: release.tag_name.trim_start_matches('v').to_owned(),
        update_available: false,
        release_url: release.html_url.unwrap_or_else(|| source.release_page()),
        release_title: release.name,
        release_notes: release.body,
        published_at: release.published_at.or(release.created_at),
    })
}

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let cleaned = version.trim().trim_start_matches('v');
    let mut parts = cleaned.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let patch = parts.next().unwrap_or("0").parse().unwrap_or(0);
    Some((major, minor, patch))
}

fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_prefix_and_missing_parts() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("  v1.0.1  "), Some((1, 0, 1)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("v"), None);
    }

    #[test]
    fn is_newer_compares_numeric_versions() {
        assert!(is_newer("1.2.0", "1.1.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("1.1.0", "1.1.0"));
        assert!(!is_newer("1.0.1", "1.1.0"));
        assert!(!is_newer("not-a-version", "1.1.0"));
        assert!(!is_newer("1.2.0", "not-a-version"));
    }

    #[test]
    fn tag_leading_v_is_stripped() {
        assert_eq!(parse_version("v2.0.0-rc.1"), Some((2, 0, 0)));
    }
}
