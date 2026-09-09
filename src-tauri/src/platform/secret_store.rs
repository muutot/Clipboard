//! Platform secret storage for sync credentials (`s3_secret_key`,
//! `sync_password`).
//!
//! This facade owns the at-rest scheme so `config::ConfigStore` never talks
//! to a platform backend directly:
//!
//! | Platform | Backend | At-rest form in `conf.json` |
//! | -------- | ------- | --------------------------- |
//! | Windows | DPAPI user scope (`dpapi`) | `dpapi1:<hex>` envelope |
//! | macOS | login Keychain (`apple-native-keyring-store`) | `oskey1:<account>` marker |
//! | Linux | Secret Service (`dbus-secret-service-keyring-store`) | `oskey1:<account>` marker |
//!
//! Markers never contain secret material; the value lives in the OS store
//! under service `clipboard-desktop`. When the OS store is unreachable
//! (headless Linux, locked Keychain), `protect_secret` returns `None` and the
//! caller keeps the previous plaintext behavior and logs the fallback —
//! never a lockout. Legacy plaintext values keep loading, and the next save
//! transparently upgrades them.

/// Slot for each sync secret persisted through this facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretAccount {
    S3SecretKey,
    SyncPassword,
}

impl SecretAccount {
    /// OS-store account name. Only live where the OS-marker path exists
    /// (non-Windows production) or in tests.
    #[cfg(any(test, not(target_os = "windows")))]
    fn as_str(self) -> &'static str {
        match self {
            Self::S3SecretKey => "sync.s3SecretKey",
            Self::SyncPassword => "sync.syncPassword",
        }
    }

    fn from_marker(value: &str) -> Option<Self> {
        match value.strip_prefix(OS_MARKER_PREFIX)? {
            "sync.s3SecretKey" => Some(Self::S3SecretKey),
            "sync.syncPassword" => Some(Self::SyncPassword),
            _ => None,
        }
    }
}

/// Marker prefix for OS-store-backed secrets. The account name follows the
/// prefix; no secret material is stored in `conf.json`.
const OS_MARKER_PREFIX: &str = "oskey1:";

/// Service name under which secrets are kept in the OS credential store.
/// Only live where the OS-marker path exists (non-Windows production) or in
/// tests.
#[cfg(any(test, not(target_os = "windows")))]
const SERVICE_NAME: &str = "clipboard-desktop";

/// Whether a stored value already carries protection (DPAPI envelope on
/// Windows, OS-store marker elsewhere).
pub fn is_protected(value: &str) -> bool {
    crate::platform::dpapi::is_envelope(value) || SecretAccount::from_marker(value).is_some()
}

/// Seals `plain` for `account` under the current platform user. Returns `None`
/// when no backend can protect on this platform/machine; callers must then
/// fall back to storing the value unchanged (and log the fallback).
pub fn protect_secret(account: SecretAccount, plain: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let _ = account;
        crate::platform::dpapi::protect(plain)
    }
    #[cfg(not(target_os = "windows"))]
    {
        os_backend::store(account, plain).then(|| format!("{OS_MARKER_PREFIX}{}", account.as_str()))
    }
}

/// Opens a value produced by [`protect_secret`]. Returns `None` for legacy
/// plaintext (handled by the caller as a passthrough) and for markers or
/// envelopes that cannot be resolved on this machine/user.
pub fn unprotect_secret(stored: &str) -> Option<String> {
    if crate::platform::dpapi::is_envelope(stored) {
        return crate::platform::dpapi::unprotect(stored);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = stored;
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        let account = SecretAccount::from_marker(stored)?;
        os_backend::load(account)
    }
}

/// OS credential-store I/O shared by macOS and Linux. Compiled out of Windows
/// production builds (which use DPAPI); tests keep it so the mock-store
/// round-trip runs on every platform.
#[cfg(any(test, not(target_os = "windows")))]
mod os_backend {
    use super::{SecretAccount, SERVICE_NAME};

    /// Registers the platform credential store once per process. No-op under
    /// `cfg(test)`: tests install the mock store explicitly.
    #[cfg(not(test))]
    fn ensure_platform_store() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            INIT.get_or_init(|| {
                #[cfg(target_os = "macos")]
                let store = apple_native_keyring_store::keychain::Store::new();
                #[cfg(target_os = "linux")]
                let store = dbus_secret_service_keyring_store::Store::new();
                if let Ok(store) = store {
                    keyring_core::set_default_store(store);
                }
            });
        }
    }

    #[cfg(test)]
    fn ensure_platform_store() {}

    pub(super) fn store(account: SecretAccount, plain: &str) -> bool {
        ensure_platform_store();
        keyring_core::Entry::new(SERVICE_NAME, account.as_str())
            .and_then(|entry| entry.set_password(plain))
            .is_ok()
    }

    pub(super) fn load(account: SecretAccount) -> Option<String> {
        ensure_platform_store();
        keyring_core::Entry::new(SERVICE_NAME, account.as_str())
            .and_then(|entry| entry.get_password())
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_cover_both_accounts_and_reject_unknown_values() {
        for account in [SecretAccount::S3SecretKey, SecretAccount::SyncPassword] {
            let marker = format!("{OS_MARKER_PREFIX}{}", account.as_str());
            assert!(is_protected(&marker));
            assert_eq!(SecretAccount::from_marker(&marker), Some(account));
        }
        assert!(!is_protected("plaintext-secret"));
        assert_eq!(SecretAccount::from_marker("oskey1:unknown"), None);
    }

    #[test]
    fn os_backend_round_trips_through_the_mock_store() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        assert!(os_backend::store(SecretAccount::S3SecretKey, "s3-secret"));
        assert_eq!(
            os_backend::load(SecretAccount::S3SecretKey).as_deref(),
            Some("s3-secret")
        );
        assert!(os_backend::load(SecretAccount::SyncPassword).is_none());
    }

    /// Full facade round-trip through the mock store. The `protect_secret` /
    /// `unprotect_secret` dispatch is target-gated (DPAPI on Windows), so this
    /// only runs where the OS-marker path is live.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn facade_round_trips_markers_through_the_mock_store() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let marker =
            protect_secret(SecretAccount::SyncPassword, "pw").expect("mock store always protects");
        assert!(is_protected(&marker));
        assert_eq!(unprotect_secret(&marker).as_deref(), Some("pw"));
    }
}
