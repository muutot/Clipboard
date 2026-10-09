//! Bounded multi-format reads. Native revisions protect Windows/macOS; on
//! displays without one, two equal reads provide a best-effort stable sample.
use super::{ClipboardImageData, PlatformClipboard};

#[derive(Debug, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub text: Option<String>,
    pub html: Option<String>,
    pub rtf: Option<String>,
    pub image: Option<(ClipboardImageData, u32, u32)>,
    pub files: Vec<String>,
}

impl ClipboardSnapshot {
    fn read(platform: &dyn PlatformClipboard, limit: usize) -> Self {
        Self {
            text: platform.read_clipboard_text(),
            html: platform
                .read_clipboard_html()
                .filter(|s| !s.trim().is_empty() && s.len() <= limit),
            rtf: platform
                .read_clipboard_rtf()
                .filter(|s| !s.trim().is_empty() && s.len() <= limit),
            image: platform.read_clipboard_image(),
            files: platform.read_clipboard_file_paths(),
        }
    }
}

/// Retry immediately: a polling monitor may already have consumed the change
/// that invalidated the first read and need not emit another notification.
/// Equal repeated reads are not an atomic snapshot (an ABA replacement is
/// unobservable without a native revision), but never rely on a lagging local
/// notification counter as if it were the clipboard server's revision.
pub fn read_consistent_snapshot(
    platform: &dyn PlatformClipboard,
    limit: usize,
) -> Option<ClipboardSnapshot> {
    for _ in 0..3 {
        let before = platform.read_clipboard_sequence();
        let snapshot = ClipboardSnapshot::read(platform, limit);
        let after = platform.read_clipboard_sequence();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if super::bounded_command::capture_aborted() {
            return None;
        }
        match (before, after) {
            (Some(a), Some(b)) if a == b => return Some(snapshot),
            (None, None) => {
                let repeated = ClipboardSnapshot::read(platform, limit);
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                if super::bounded_command::capture_aborted() {
                    return None;
                }
                if snapshot == repeated && platform.read_clipboard_sequence().is_none() {
                    return Some(repeated);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ForegroundApp;
    use std::{cell::Cell, path::Path};

    struct Clipboard {
        reads: Cell<u32>,
        native: bool,
        settle_after: u32,
    }

    impl PlatformClipboard for Clipboard {
        fn read_clipboard_sequence(&self) -> Option<u32> {
            self.native.then(|| self.reads.get().min(self.settle_after))
        }
        fn read_clipboard_text(&self) -> Option<String> {
            let n = self.reads.get();
            self.reads.set(n + 1);
            Some(n.min(self.settle_after).to_string())
        }
        fn read_clipboard_html(&self) -> Option<String> {
            Some(self.reads.get().min(self.settle_after).to_string())
        }
        fn read_clipboard_image(&self) -> Option<(ClipboardImageData, u32, u32)> {
            None
        }
        fn read_clipboard_file_paths(&self) -> Vec<String> {
            vec![]
        }
        fn get_foreground_app(&self) -> ForegroundApp {
            ForegroundApp::empty()
        }
        fn write_clipboard_text_with_self_trigger(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn extract_app_icon(&self, _: &Path, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn retries_replacement_instead_of_saving_mixed_formats() {
        for native in [false, true] {
            let clipboard = Clipboard {
                reads: Cell::new(0),
                native,
                settle_after: 1,
            };
            let snapshot = read_consistent_snapshot(&clipboard, 100).unwrap();
            assert_eq!(snapshot.text.as_deref(), Some("1"));
            assert_eq!(snapshot.html.as_deref(), Some("1"));
        }
    }

    #[test]
    fn continuous_changes_have_a_bounded_retry_budget() {
        for native in [false, true] {
            let clipboard = Clipboard {
                reads: Cell::new(0),
                native,
                settle_after: 100,
            };
            assert!(read_consistent_snapshot(&clipboard, 100).is_none());
            assert_eq!(clipboard.reads.get(), if native { 3 } else { 6 });
        }
    }
}
