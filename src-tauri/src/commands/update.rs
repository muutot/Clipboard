// update.rs — update checks against the project's release sources. The user
// picks the source (GitHub upstream or the GitCode mirror) from the settings
// About panel via the persisted `updateSource` general setting.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::config::{ConfigStore, UpdateSource};

// Release notes plus provider asset metadata fit comfortably within this
// budget. The response timeout alone cannot bound retained body memory.
const MAX_RELEASE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

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
                format!(
                    "https://api.gitcode.com/api/v5/repos/m2u/Clipboard/releases/tags/{encoded}"
                )
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

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request release: {e}"))?
        .error_for_status()
        .map_err(|e| format!("fetch release failed: {e}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_RESPONSE_BYTES as u64)
    {
        return Err(format!(
            "release response exceeds the {MAX_RELEASE_RESPONSE_BYTES} byte limit"
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("read response: {e}"))?
    {
        if chunk.len() > MAX_RELEASE_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(format!(
                "release response exceeds the {MAX_RELEASE_RESPONSE_BYTES} byte limit"
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    // Matches reqwest's text() decoding with the configured charset feature
    // disabled, retaining its UTF-8 replacement behavior within the budget.
    let body = String::from_utf8_lossy(&bytes);
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

fn parse_version(version: &str) -> Option<semver::Version> {
    let trimmed = version.trim();
    let cleaned = trimmed.strip_prefix('v').unwrap_or(trimmed);
    // Preserve legacy abbreviated numeric cores without discarding suffixes.
    let suffix_start = cleaned.find(['-', '+']).unwrap_or(cleaned.len());
    let (core, suffix) = cleaned.split_at(suffix_start);
    let normalized = match core.split('.').count() {
        1 => format!("{core}.0.0{suffix}"),
        2 => format!("{core}.0{suffix}"),
        3 => cleaned.to_owned(),
        _ => return None,
    };
    semver::Version::parse(&normalized).ok()
}

fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(remote), Some(current)) => remote.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        time::Duration,
    };

    struct ReleaseServer {
        url: String,
        stop: Arc<AtomicBool>,
        release: mpsc::Sender<()>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for ReleaseServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = self.release.send(());
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    fn release_server(framing: String, body: Vec<u8>, hold_eof: bool) -> ReleaseServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/release", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (release, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("release fixture accept failed: {error}"),
                };
                // Windows can inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                let chunked = framing.contains("chunked");
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{framing}Connection: close\r\n\r\n");
                if chunked {
                    let _ = write!(stream, "{:x}\r\n", body.len());
                }
                let _ = stream.write_all(&body);
                if chunked {
                    let _ = stream.write_all(b"\r\n");
                }
                let _ = stream.flush();
                if hold_eof {
                    let _ = receiver.recv_timeout(Duration::from_secs(5));
                }
                if chunked {
                    let _ = stream.write_all(b"0\r\n\r\n");
                }
                return;
            }
        });
        ReleaseServer {
            url,
            stop,
            release,
            thread: Some(thread),
        }
    }

    fn oversized_release_scenario(framing: String, body: Vec<u8>) {
        let server = release_server(framing, body, true);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), fetch_release(server.url.clone())).await
        });
        drop(server);
        assert!(
            result
                .as_ref()
                .is_ok_and(|result| result.as_ref().is_err_and(|error| error.contains("limit"))),
            "release response was not bounded before EOF: {result:?}"
        );
    }

    #[test]
    fn oversized_declared_release_response_is_rejected_before_eof() {
        oversized_release_scenario(
            format!("Content-Length: {}\r\n", MAX_RELEASE_RESPONSE_BYTES + 1),
            b"{".to_vec(),
        );
    }

    #[test]
    fn oversized_chunked_release_response_is_rejected_before_eof() {
        oversized_release_scenario(
            "Transfer-Encoding: chunked\r\n".to_owned(),
            vec![b' '; MAX_RELEASE_RESPONSE_BYTES + 1],
        );
    }

    #[test]
    fn oversized_close_delimited_release_response_is_rejected_before_eof() {
        oversized_release_scenario(String::new(), vec![b' '; MAX_RELEASE_RESPONSE_BYTES + 1]);
    }

    #[test]
    fn valid_release_responses_at_the_budget_preserve_provider_fields() {
        for (framing, timestamp) in [
            (
                format!("Content-Length: {MAX_RELEASE_RESPONSE_BYTES}\r\n"),
                "published_at",
            ),
            ("Transfer-Encoding: chunked\r\n".to_owned(), "created_at"),
            (String::new(), "published_at"),
        ] {
            let notes = "中文发布说明\n- preserve Unicode and newlines";
            let mut bytes = serde_json::to_vec(&serde_json::json!({
                "tag_name": "v1.8.2",
                "name": "Clipboard 中文",
                "body": notes,
                "html_url": "https://example.invalid/releases/v1.8.2",
                (timestamp): "2026-11-02T00:00:00Z",
                "assets": [{"name": "fixture.exe", "size": 1024}]
            }))
            .unwrap();
            bytes.resize(MAX_RELEASE_RESPONSE_BYTES, b' ');
            let server = release_server(framing, bytes, false);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(fetch_release(server.url.clone()));
            drop(server);
            let release = result.expect("valid response at the budget must parse");
            assert_eq!(release.tag_name, "v1.8.2");
            assert_eq!(release.name.as_deref(), Some("Clipboard 中文"));
            assert_eq!(release.body.as_deref(), Some(notes));
            assert_eq!(
                release.html_url.as_deref(),
                Some("https://example.invalid/releases/v1.8.2")
            );
            assert_eq!(
                release.published_at.or(release.created_at).as_deref(),
                Some("2026-11-02T00:00:00Z")
            );
        }
    }

    #[test]
    fn parse_version_handles_prefix_and_missing_parts() {
        assert_eq!(parse_version("1.2.3"), Some(semver::Version::new(1, 2, 3)));
        assert_eq!(parse_version("v1.2.3"), Some(semver::Version::new(1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some(semver::Version::new(1, 2, 0)));
        assert_eq!(parse_version("2"), Some(semver::Version::new(2, 0, 0)));
        assert_eq!(
            parse_version("  v1.0.1  "),
            Some(semver::Version::new(1, 0, 1))
        );
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
    fn semver_precedence_preserves_prereleases_and_ignores_build_metadata() {
        for (remote, current) in [
            ("v1.8.4-beta.2", "1.8.4-beta.1"),
            ("1.8.4-beta.10", "1.8.4-beta.2"),
            ("1.8.4", "1.8.4-beta.2"),
            ("1.8.5-alpha", "1.8.4"),
        ] {
            assert!(is_newer(remote, current), "{remote} > {current}");
            assert!(!is_newer(current, remote));
        }
        assert!(!is_newer("1.8.4+new", "1.8.4+old"));
        assert!(!is_newer("1.8.4-beta.2", "1.8.4"));
        for invalid in [
            "1.2.3.4",
            "01.2.3",
            "1.2.3-beta.01",
            "1.2.3-",
            "1.2.3+",
            "vv1.2.3",
            "1..2",
        ] {
            assert!(parse_version(invalid).is_none(), "{invalid}");
            assert!(!is_newer(invalid, "1.0.0"));
        }
    }

    #[test]
    fn parse_version_rejects_unparsable_components() {
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_version("1.x.0"), None);
        assert_eq!(
            parse_version("1.2.3-beta"),
            Some(semver::Version::parse("1.2.3-beta").unwrap())
        );
        assert_eq!(
            parse_version("1.2.3+build"),
            Some(semver::Version::parse("1.2.3+build").unwrap())
        );
    }

    #[test]
    fn tag_leading_v_is_stripped() {
        assert_eq!(
            parse_version("v2.0.0-rc.1"),
            Some(semver::Version::parse("2.0.0-rc.1").unwrap())
        );
    }
}
