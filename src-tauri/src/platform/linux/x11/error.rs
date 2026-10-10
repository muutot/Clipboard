// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that may occur during X11 platform operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11Error {
    /// Could not open a connection to the X server (`XOpenDisplay` returned NULL).
    DisplayOpenFailed(String),
    /// An X11 protocol request returned an error.
    XError {
        code: u8,
        request_code: u8,
        minor_code: u16,
        resource_id: u64,
    },
    /// A required X11 atom was missing (TARGETS, UTF8_STRING, etc.).
    MissingAtom(String),
    /// XGetWindowProperty did not return the expected format or type.
    PropertyReadFailed(String),
    /// A global hotkey could not be registered (keycode already grabbed).
    HotkeyGrabFailed(u32, u32),
    /// Failed to create the system tray icon.
    TrayCreationFailed(String),
    /// The XFixes extension is not available (required for clipboard monitoring).
    XFixesNotAvailable,
}

impl std::fmt::Display for X11Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DisplayOpenFailed(msg) => write!(f, "failed to open X display: {msg}"),
            Self::XError {
                code,
                request_code,
                minor_code,
                resource_id,
            } => write!(
                f,
                "X error: code={code} request={request_code} minor={minor_code} resource={resource_id}"
            ),
            Self::MissingAtom(name) => write!(f, "missing X atom: {name}"),
            Self::PropertyReadFailed(msg) => write!(f, "property read failed: {msg}"),
            Self::HotkeyGrabFailed(keycode, modifiers) => {
                write!(f, "failed to grab keycode {keycode} with modifiers {modifiers:#x}")
            }
            Self::TrayCreationFailed(msg) => write!(f, "tray creation failed: {msg}"),
            Self::XFixesNotAvailable => write!(f, "XFixes extension not available"),
        }
    }
}

impl std::error::Error for X11Error {}

/// Convenience alias for X11 operation results.
pub type X11Result<T> = Result<T, X11Error>;
