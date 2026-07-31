// update.rs — update checks against the project's GitHub Releases API.

use serde::{Deserialize, Serialize};

const UPDATE_REPO: &str = "muutot/Clipboard";

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

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
}

#[tauri::command]
pub async fn check_for_update() -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent("clipboard-desktop")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("create client: {e}"))?;

    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/releases/latest");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("request latest release: {e}"))?
        .error_for_status()
        .map_err(|e| format!("check update failed: {e}"))?;
    let body = response
        .text()
        .await
        .map_err(|e| format!("read response: {e}"))?;
    let release: GithubRelease =
        serde_json::from_str(&body).map_err(|e| format!("parse release: {e}"))?;

    let current_version = env!("CARGO_PKG_VERSION").to_owned();
    let latest_version = release.tag_name.trim_start_matches('v').to_owned();
    let update_available = is_newer(&latest_version, &current_version);

    Ok(UpdateInfo {
        current_version,
        latest_version,
        update_available,
        release_url: release.html_url,
        release_title: release.name,
        release_notes: release.body,
        published_at: release.published_at,
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
