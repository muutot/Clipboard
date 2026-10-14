//! OS locale detection (CoreFoundation) for the `language: "system"` setting.
//!
//! The webview reports the Chromium accepted language, which can differ from
//! the macOS display language; `CFLocaleCopyCurrentLocale` gives the user's
//! actual locale. See `platform/os_locale.rs` for the shared contract.

use std::ffi::c_void;

// ApplicationServices, not CoreFoundation: it re-exports the CoreFoundation
// symbols this module needs, and it is the framework this adapter already
// links successfully for the CoreGraphics declarations in `ffi.rs` /
// `quick_paste.rs`. A `#[link(name = "CoreFoundation")]` block here left
// `_CFLocaleCopyCurrentLocale` unresolved at the final link (observed on the
// macOS arm64 CI runner).
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CFLocaleCopyCurrentLocale() -> *const c_void;
    fn CFLocaleGetIdentifier(locale: *const c_void) -> *const c_void;
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut u8,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(cf: *const c_void);
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// Returns the macOS user locale identifier (`zh-CN`, `en-US`, …), or `None`
/// when CoreFoundation cannot answer.
pub fn locale_name() -> Option<String> {
    unsafe {
        let locale = CFLocaleCopyCurrentLocale();
        if locale.is_null() {
            return None;
        }
        let identifier = CFLocaleGetIdentifier(locale);
        let name = if identifier.is_null() {
            None
        } else {
            let mut buffer = [0u8; 64];
            if CFStringGetCString(
                identifier,
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            ) {
                let length = buffer
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(buffer.len());
                Some(String::from_utf8_lossy(&buffer[..length]).into_owned())
            } else {
                None
            }
        };
        // The copied locale is retained; the identifier belongs to it.
        CFRelease(locale);
        name
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn identifier_is_a_plain_locale_name() {
        // Guarded by the platform gate in CI: the shape of the identifier is
        // pure UTF-8 BCP-47 text, never a JSON envelope.
        let Some(name) = super::locale_name() else {
            return;
        };
        assert!(!name.trim().is_empty());
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "unexpected locale identifier: {name}"
        );
    }
}
