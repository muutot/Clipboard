//! Platform secret storage for sync credentials (`s3_secret_key`,
//! `sync_password`).
//!
//! This facade owns the envelope scheme so `config::ConfigStore` never talks
//! to a platform backend directly. Backends are additive per platform:
//!
//! | Platform | Backend | At-rest form |
//! | -------- | ------- | ------------ |
//! | Windows | DPAPI user scope (`dpapi`) | `dpapi1:<hex>` envelope in `conf.json` |
//! | macOS / Linux | _not yet wired (SEC-05)_ | plaintext fallback in `conf.json` |
//!
//! Callers must treat `None` from [`protect_secret`] as "keep the previous
//! behavior" (plaintext), exactly like the pre-facade code did: legacy
//! plaintext values keep loading, and the next save transparently upgrades
//! them once a backend exists.

/// Whether a stored value already carries a protection envelope.
pub fn is_protected(value: &str) -> bool {
    crate::platform::dpapi::is_envelope(value)
}

/// Seals `plain` under the current platform user. Returns `None` when no
/// backend can protect on this platform; callers must then fall back to
/// storing the value unchanged.
pub fn protect_secret(plain: &str) -> Option<String> {
    crate::platform::dpapi::protect(plain)
}

/// Opens an envelope produced by [`protect_secret`]. Returns `None` for
/// values without the envelope prefix (legacy plaintext) or on failure.
pub fn unprotect_secret(stored: &str) -> Option<String> {
    crate::platform::dpapi::unprotect(stored)
}
