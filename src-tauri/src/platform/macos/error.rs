//! macOS adapter error type shared by the accessibility, hotkey and tray code.

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that may occur during macOS platform operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacOSError {
    /// Clipboard access was denied (sandbox / TCC).
    ClipboardAccessDenied,
    /// The pasteboard returned an unexpected change count.
    PasteboardReadFailed(String),
    /// Accessibility permission has not been granted.
    AccessibilityPermissionDenied,
    /// Registering a global hotkey failed (key already taken).
    HotkeyRegistrationFailed(String),
    /// A Carbon / CoreGraphics API call returned an error.
    ApiError(i32, String),
    /// The menu bar item could not be created.
    TrayCreationFailed(String),
}

impl std::fmt::Display for MacOSError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClipboardAccessDenied => f.write_str("clipboard access denied"),
            Self::PasteboardReadFailed(msg) => write!(f, "pasteboard read failed: {msg}"),
            Self::AccessibilityPermissionDenied => {
                f.write_str("accessibility permission not granted")
            }
            Self::HotkeyRegistrationFailed(msg) => {
                write!(f, "hotkey registration failed: {msg}")
            }
            Self::ApiError(code, msg) => write!(f, "API error ({code}): {msg}"),
            Self::TrayCreationFailed(msg) => write!(f, "tray creation failed: {msg}"),
        }
    }
}

impl std::error::Error for MacOSError {}

/// Convenience alias for results from macOS platform operations.
pub type MacOSResult<T> = Result<T, MacOSError>;
