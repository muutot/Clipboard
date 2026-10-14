//! OS UI-locale detection backing the `language: "system"` setting.
//!
//! The webview can only report `navigator.language`, which follows the
//! Chromium accepted-language list instead of the OS display language. On
//! Windows the two disagree often enough (Chinese display language with an
//! English preferred-language list) that `system` opened the UI in English on
//! a Chinese machine, so the OS is asked directly here.
//!
//! The raw locale name is returned (`zh-CN`, `en-US`, …); mapping it to a
//! supported UI language stays in the frontend i18n module, next to the
//! `navigator.language` fallback that still covers non-Tauri previews.

/// Returns the OS user locale name (BCP-47-ish), or `None` when the platform
/// call yields nothing usable — the caller then falls back to the webview.
pub fn os_locale_name() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        windows_locale_name()
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::locale::locale_name()
    }
    #[cfg(target_os = "linux")]
    {
        linux_locale_name()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// `GetUserDefaultLocaleName` (the display/format language of the interactive
/// user, not the ANSI code page) is the authoritative Windows UI locale.
#[cfg(target_os = "windows")]
fn windows_locale_name() -> Option<String> {
    // LOCALE_NAME_MAX_LENGTH is 85 including the terminating null.
    const LOCALE_NAME_MAX_LENGTH: usize = 85;

    extern "system" {
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 0 {
        return None;
    }
    // The count includes the terminating null, which utf16_lossy must not see.
    let name = String::from_utf16_lossy(&buffer[..written as usize - 1]);
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// POSIX environment variables in shell precedence order; the codeset and
/// modifier are dropped so `zh_CN.UTF-8` and `zh_CN@euro` both surface as
/// `zh_CN`. `C`/`POSIX` mean "no locale set" and are treated as absent.
#[cfg(target_os = "linux")]
fn linux_locale_name() -> Option<String> {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let name = value
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if !name.is_empty() && name != "C" && name != "POSIX" {
            return Some(name);
        }
    }
    None
}

/// Strips the codeset/modifier from a POSIX locale value. Pure so the Linux
/// parsing rule is testable on every host.
#[cfg(test)]
fn posix_locale_name(value: &str) -> Option<&str> {
    let name = value.split(['.', '@']).next().unwrap_or_default().trim();
    if name.is_empty() || name == "C" || name == "POSIX" {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_values_drop_codeset_and_modifier() {
        assert_eq!(posix_locale_name("zh_CN.UTF-8"), Some("zh_CN"));
        assert_eq!(posix_locale_name("en_US.UTF-8"), Some("en_US"));
        assert_eq!(posix_locale_name("zh_CN@euro"), Some("zh_CN"));
        assert_eq!(posix_locale_name("  zh_TW.Big5  "), Some("zh_TW"));
    }

    #[test]
    fn posix_empty_and_default_values_are_absent() {
        assert_eq!(posix_locale_name(""), None);
        assert_eq!(posix_locale_name("C"), None);
        assert_eq!(posix_locale_name("C.UTF-8"), None);
        assert_eq!(posix_locale_name("POSIX"), None);
        assert_eq!(posix_locale_name("   "), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_reports_the_user_locale_name() {
        let name = windows_locale_name().expect("Windows always names a user locale");
        assert!(!name.trim().is_empty());
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_honors_shell_precedence() {
        // LC_ALL wins over LC_MESSAGES and LANG, and an unset or empty
        // LC_ALL must fall through rather than short-circuit to None.
        let previous: Vec<Option<String>> = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .map(|key| std::env::var(key).ok())
            .collect();

        std::env::set_var("LC_ALL", "zh_CN.UTF-8");
        std::env::set_var("LC_MESSAGES", "en_US.UTF-8");
        std::env::set_var("LANG", "en_US.UTF-8");
        assert_eq!(linux_locale_name(), Some("zh_CN".to_owned()));

        std::env::set_var("LC_ALL", "");
        assert_eq!(linux_locale_name(), Some("en_US".to_owned()));

        std::env::remove_var("LC_ALL");
        std::env::remove_var("LC_MESSAGES");
        std::env::set_var("LANG", "zh_TW.UTF-8");
        assert_eq!(linux_locale_name(), Some("zh_TW".to_owned()));

        for (index, key) in ["LC_ALL", "LC_MESSAGES", "LANG"].iter().enumerate() {
            match &previous[index] {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
