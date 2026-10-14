//! OS locale detection (CoreFoundation) for the `language: "system"` setting.
//!
//! The webview reports the Chromium accepted language, which can differ from
//! the macOS display language; `CFLocaleCopyCurrent` gives the user's actual
//! locale. See `platform/os_locale.rs` for the shared contract.
//!
//! The CoreFoundation declarations live in `ffi.rs`, next to the other native
//! declarations of this adapter.

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// Returns the macOS user locale identifier (`zh-CN`, `en-US`, …), or `None`
/// when CoreFoundation cannot answer.
pub fn locale_name() -> Option<String> {
    use crate::platform::macos::ffi::{
        CFLocaleCopyCurrent, CFLocaleGetIdentifier, CFRelease, CFStringGetCString,
    };
    unsafe {
        let locale = CFLocaleCopyCurrent();
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
        // `CFRelease` is declared once in `platform::macos::ffi`.
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
