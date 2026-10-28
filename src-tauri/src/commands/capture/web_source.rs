//! Web-source attribution: when the foreground app is a browser, turn the raw
//! window title and the CF_HTML `SourceURL:` header into the `web` metadata
//! blob stored on text/link captures.
//!
//! The logic is pure and cross-platform: the platform-specific inputs
//! (`ForegroundApp.window_title`, the snapshot `source_url`) are empty/None on
//! platforms that do not provide them, and the metadata then degrades to
//! `None`.

use crate::content::RESOURCE_METADATA_SCHEMA_VERSION;

/// Browser executable stems (lowercase, no extension) whose window titles end
/// with a browser display-name suffix.
const BROWSER_EXE_STEMS: &[&str] = &[
    "chrome", "chromium", "msedge", "edge", "firefox", "brave", "vivaldi", "opera",
];

/// Window-title suffixes appended by browsers after the page title. Firefox
/// uses an em dash; the others use " - ".
const BROWSER_TITLE_SUFFIXES: &[&str] = &[
    " - Google Chrome",
    " - Chromium",
    " - Microsoft Edge",
    " - Brave",
    " - Vivaldi",
    " - Opera",
    " - Mozilla Firefox",
    " — Mozilla Firefox",
];

fn is_browser_exe_stem(exe_stem: &str) -> bool {
    let stem = exe_stem
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_owned();
    BROWSER_EXE_STEMS.contains(&stem.as_str())
}

/// Strips the trailing browser display-name suffix from a raw window title to
/// recover the page title. Returns `None` for non-browser apps and empty or
/// suffix-only titles.
pub fn extract_page_title(window_title: &str, exe_stem: &str) -> Option<String> {
    if !is_browser_exe_stem(exe_stem) {
        return None;
    }
    // Microsoft Edge inserts a zero-width space inside "Microsoft Edge".
    let normalized = window_title.replace('\u{200b}', "");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    for suffix in BROWSER_TITLE_SUFFIXES {
        if let Some(page) = trimmed.strip_suffix(suffix) {
            let page = page.trim();
            return (!page.is_empty()).then(|| page.to_owned());
        }
    }
    Some(trimmed.to_owned())
}

/// Extracts the host from an absolute URL for domain-based mapping rules.
/// Strips userinfo, port and a leading `www.`; returns `None` when the URL
/// has no scheme/host.
pub fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim();
    let (_, after_scheme) = url.split_once("://")?;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = host.split(':').next().unwrap_or_default();
    let host = host
        .strip_prefix("www.")
        .unwrap_or(host)
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Builds the `web` source metadata JSON for a text/link capture, or `None`
/// when the capture is not from a known browser or carries no page signal.
///
/// Shape: `{"schemaVersion":2,"web":{"title":…,"url":…,"domain":…}}` — the
/// same versioned envelope the file-capture metadata uses, and an opaque
/// string for sync (new keys are backward compatible).
pub fn web_source_metadata(
    exe_path: &str,
    window_title: &str,
    source_url: Option<&str>,
) -> Option<String> {
    let exe_stem = exe_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(exe_path)
        .trim_end_matches(".exe");
    if !is_browser_exe_stem(exe_stem) {
        return None;
    }

    let title = extract_page_title(window_title, exe_stem).unwrap_or_default();
    let url = source_url.map(str::trim).filter(|url| !url.is_empty());
    if title.is_empty() && url.is_none() {
        return None;
    }
    let domain = url.and_then(extract_domain);

    Some(
        serde_json::json!({
            "schemaVersion": RESOURCE_METADATA_SCHEMA_VERSION,
            "web": {
                "title": title,
                "url": url,
                "domain": domain,
            }
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_title_strips_known_browser_suffixes() {
        assert_eq!(
            extract_page_title("Example Page - Google Chrome", "chrome").as_deref(),
            Some("Example Page")
        );
        assert_eq!(
            extract_page_title("Example Page — Mozilla Firefox", "firefox").as_deref(),
            Some("Example Page")
        );
        // Edge window titles contain a zero-width space inside the app name.
        assert_eq!(
            extract_page_title("Example Page - Microsoft\u{200b} Edge", "msedge").as_deref(),
            Some("Example Page")
        );
        // Page titles containing " - " survive: only the suffix is stripped.
        assert_eq!(
            extract_page_title("Foo - Bar - Google Chrome", "chrome").as_deref(),
            Some("Foo - Bar")
        );
        // The stem check tolerates a ".exe" suffix.
        assert_eq!(
            extract_page_title("Untitled - Google Chrome", "chrome.exe").as_deref(),
            Some("Untitled")
        );
    }

    #[test]
    fn page_title_rejects_non_browsers_and_empty_input() {
        assert_eq!(
            extract_page_title("Document.txt - Notepad", "notepad"),
            None
        );
        assert_eq!(extract_page_title("", "chrome"), None);
        assert_eq!(extract_page_title("   ", "chrome"), None);
        // Without a matching suffix the raw title is returned as-is.
        assert_eq!(
            extract_page_title("Google Chrome", "chrome").as_deref(),
            Some("Google Chrome")
        );
    }

    #[test]
    fn domain_extraction_strips_scheme_www_port_and_userinfo() {
        assert_eq!(
            extract_domain("https://github.com/rust-lang/rust").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            extract_domain("https://www.example.com:8443/path?q=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            extract_domain("https://user:pass@host.example.com/").as_deref(),
            Some("host.example.com")
        );
        assert_eq!(extract_domain("not a url"), None);
    }

    #[test]
    fn web_metadata_is_written_for_browser_captures_only() {
        let metadata = web_source_metadata(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            "Example Page - Google Chrome",
            Some("https://example.com/page"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(value["schemaVersion"], RESOURCE_METADATA_SCHEMA_VERSION);
        assert_eq!(value["web"]["title"], "Example Page");
        assert_eq!(value["web"]["url"], "https://example.com/page");
        assert_eq!(value["web"]["domain"], "example.com");

        // Firefox: title only, no URL/domain.
        let metadata =
            web_source_metadata(r"C:\Apps\firefox.exe", "Docs — Mozilla Firefox", None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(value["web"]["title"], "Docs");
        assert!(value["web"]["url"].is_null());
        assert!(value["web"]["domain"].is_null());

        // Non-browser apps never get web metadata.
        assert_eq!(
            web_source_metadata(r"C:\Windows\notepad.exe", "Untitled - Notepad", None),
            None
        );
        // Browser without any page signal (empty title, no URL) stays empty.
        assert_eq!(web_source_metadata(r"C:\Apps\chrome.exe", "", None), None);
    }
}
