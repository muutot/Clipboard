//! Accessibility permission state reported to `platform_info`.
//!
//! `is_trusted()` currently returns `false` unconditionally: the
//! `AXIsProcessTrusted` / `AXMakeProcessTrusted` calls are still commented out
//! in `ffi.rs`, so treat this as an honest "not yet wired" signal rather than a
//! working permission probe.

#[cfg(target_os = "macos")]
use super::error::MacOSError;
use super::error::MacOSResult;

// ---------------------------------------------------------------------------
// MacOSAccessibilityHelper
// ---------------------------------------------------------------------------

/// Checks and requests macOS accessibility permissions.
///
/// On macOS many clipboard-manager features (paste simulation, window focus
/// tracking) require the application to be granted Accessibility access in
/// System Settings → Privacy & Security → Accessibility.
///
/// # API
///
/// - `AXIsProcessTrusted()` → returns `true` if the process is trusted.
/// - `AXMakeProcessTrusted()` → opens a system dialog requesting permission.
///   The process must be restarted after the dialog is approved.
pub struct MacOSAccessibilityHelper;

impl MacOSAccessibilityHelper {
    /// Returns `true` if the current process has been granted accessibility
    /// permissions.
    ///
    /// Wraps `AXIsProcessTrusted()`.
    #[cfg(target_os = "macos")]
    pub fn is_trusted() -> bool {
        // unsafe { AXIsProcessTrusted() }
        false
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_trusted() -> bool {
        false
    }

    /// Requests accessibility permission from the user.
    ///
    /// On macOS this calls `AXMakeProcessTrusted()` which presents a system
    /// dialog.  The process typically requires a restart after approval.
    ///
    /// Returns `Ok(())` if the request was submitted, or an error if
    /// the API is unavailable.
    #[cfg(target_os = "macos")]
    pub fn request_permission() -> MacOSResult<()> {
        // let result = unsafe { AXMakeProcessTrusted() };
        // if result != 0 {
        //     return Err(MacOSError::AccessibilityPermissionDenied);
        // }
        // Ok(())
        Err(MacOSError::AccessibilityPermissionDenied)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn request_permission() -> MacOSResult<()> {
        Ok(())
    }

    /// Returns a human-readable status string describing the current
    /// accessibility permission state.
    pub fn status_description() -> &'static str {
        if Self::is_trusted() {
            "Accessibility permission granted — all features available."
        } else {
            "Accessibility permission required. Open System Settings → \
             Privacy & Security → Accessibility and enable this app."
        }
    }

    /// Returns whether a restart is required after granting permission.
    ///
    /// On macOS, `AXMakeProcessTrusted()` does not take effect until the
    /// process is relaunched.
    pub fn requires_restart_after_grant() -> bool {
        true
    }
}
