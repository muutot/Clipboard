//! Platform secret storage for sync credentials (`s3_secret_key`,
//! `sync_password`).
//!
//! This facade owns the at-rest scheme so `config::ConfigStore` never talks
//! to a platform backend directly:
//!
//! | Platform | Backend | At-rest form in `conf.json` |
//! | -------- | ------- | --------------------------- |
//! | Windows | DPAPI user scope (`dpapi`) | `dpapi1:<hex>` envelope |
//! | macOS | login Keychain (`apple-native-keyring-store`) | `oskey1:<account>:<uuid>` marker |
//! | Linux | Secret Service (`dbus-secret-service-keyring-store`) | `oskey1:<account>:<uuid>` marker |
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
        let account = value.strip_prefix(OS_MARKER_PREFIX)?;
        let name = if let Some((name, generation)) = account.split_once(':') {
            let id = uuid::Uuid::parse_str(generation).ok()?;
            if id.to_string() != generation {
                return None;
            }
            name
        } else {
            account
        };
        match name {
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
    crate::platform::windows::dpapi::is_envelope(value)
        || SecretAccount::from_marker(value).is_some()
}

/// Seals `plain` for `account` under the current platform user. Returns `None`
/// when no backend can protect on this platform/machine; callers must then
/// fall back to storing the value unchanged (and log the fallback).
pub fn protect_secret(account: SecretAccount, plain: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let _ = account;
        crate::platform::windows::dpapi::protect(plain)
    }
    #[cfg(not(target_os = "windows"))]
    {
        os_backend::protect(account, plain)
    }
}

/// Opens a value produced by [`protect_secret`]. Returns `None` for legacy
/// plaintext (handled by the caller as a passthrough) and for markers or
/// envelopes that cannot be resolved on this machine/user.
pub fn unprotect_secret(stored: &str) -> Option<String> {
    if crate::platform::windows::dpapi::is_envelope(stored) {
        return crate::platform::windows::dpapi::unprotect(stored);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = stored;
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        os_backend::unprotect(stored)
    }
}

/// OS credential-store I/O shared by macOS and Linux. Compiled out of Windows
/// production builds (which use DPAPI); tests keep it so the mock-store
/// round-trip runs on every platform.
#[cfg(any(test, not(target_os = "windows")))]
mod os_backend {
    use super::{SecretAccount, OS_MARKER_PREFIX, SERVICE_NAME};

    pub(super) fn protect(account: SecretAccount, plain: &str) -> Option<String> {
        ensure_platform_store();
        // Immutable generations preserve old config/backup markers even when
        // the following config save fails or another profile changes a secret.
        let name = format!("{}:{}", account.as_str(), uuid::Uuid::new_v4());
        let entry = keyring_core::Entry::new(SERVICE_NAME, &name).ok()?;
        entry.set_password(plain).ok()?;
        Some(format!("{OS_MARKER_PREFIX}{name}"))
    }

    pub(super) fn unprotect(stored: &str) -> Option<String> {
        SecretAccount::from_marker(stored)?;
        ensure_platform_store();
        keyring_core::Entry::new(SERVICE_NAME, stored.strip_prefix(OS_MARKER_PREFIX)?)
            .and_then(|entry| entry.get_password())
            .ok()
    }

    pub(super) fn discard(stored: &str) {
        if SecretAccount::from_marker(stored).is_none() {
            return;
        }
        let name = stored.strip_prefix(OS_MARKER_PREFIX).unwrap();
        // Never remove legacy shared slots: another profile may still use one.
        if !name.contains(':') {
            return;
        }
        ensure_platform_store();
        if keyring_core::Entry::new(SERVICE_NAME, name)
            .and_then(|entry| entry.delete_credential())
            .is_err()
        {
            crate::log_warn!("[config] could not discard an uncommitted credential generation");
        }
    }

    /// Registers the platform credential store once per process. Tests share
    /// a mock store and never open the user's native credential store.
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
    fn ensure_platform_store() {
        super::initialize_mock_store();
    }
}

/// Removes only a newly prepared OS-store generation after config persistence fails.
/// DPAPI envelopes have no external object to roll back.
pub fn discard_secret(stored: &str) {
    #[cfg(not(target_os = "windows"))]
    os_backend::discard(stored);
    #[cfg(target_os = "windows")]
    let _ = stored;
}

#[cfg(test)]
pub(crate) fn initialize_mock_store() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_secret_preserves_the_previous_configuration_marker() {
        initialize_mock_store();
        let old = os_backend::protect(SecretAccount::S3SecretKey, "previous fixture").unwrap();
        let next = os_backend::protect(SecretAccount::S3SecretKey, "new fixture").unwrap();
        // Preparing a save must not change the value the old config still names.
        assert_eq!(
            os_backend::unprotect(&old).as_deref(),
            Some("previous fixture")
        );
        assert_eq!(os_backend::unprotect(&next).as_deref(), Some("new fixture"));
        assert_ne!(old, next);
        os_backend::discard(&next);
        assert!(os_backend::unprotect(&next).is_none());
        assert_eq!(
            os_backend::unprotect(&old).as_deref(),
            Some("previous fixture")
        );
    }

    #[test]
    fn markers_cover_both_accounts_and_reject_unknown_values() {
        for account in [SecretAccount::S3SecretKey, SecretAccount::SyncPassword] {
            let marker = format!("{OS_MARKER_PREFIX}{}", account.as_str());
            assert!(is_protected(&marker));
            assert_eq!(SecretAccount::from_marker(&marker), Some(account));
        }
        assert!(!is_protected("plaintext-secret"));
        assert_eq!(SecretAccount::from_marker("oskey1:unknown"), None);
        assert_eq!(
            SecretAccount::from_marker("oskey1:sync.syncPassword:invalid"),
            None
        );
    }

    #[test]
    fn os_backend_round_trips_through_the_mock_store() {
        initialize_mock_store();
        let entry =
            keyring_core::Entry::new(SERVICE_NAME, SecretAccount::S3SecretKey.as_str()).unwrap();
        entry.set_password("s3-secret").unwrap();
        let marker = "oskey1:sync.s3SecretKey";
        assert_eq!(os_backend::unprotect(marker).as_deref(), Some("s3-secret"));
        os_backend::discard(marker);
        assert_eq!(os_backend::unprotect(marker).as_deref(), Some("s3-secret"));
    }

    /// Full facade round-trip through the mock store. The `protect_secret` /
    /// `unprotect_secret` dispatch is target-gated (DPAPI on Windows), so this
    /// only runs where the OS-marker path is live.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn facade_round_trips_markers_through_the_mock_store() {
        initialize_mock_store();
        let marker =
            protect_secret(SecretAccount::SyncPassword, "pw").expect("mock store always protects");
        assert!(is_protected(&marker));
        assert_eq!(unprotect_secret(&marker).as_deref(), Some("pw"));
    }
}
