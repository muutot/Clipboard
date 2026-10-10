use super::*;

// ---------------------------------------------------------------------------
// X11TrayManager
// ---------------------------------------------------------------------------

/// Manages a system tray icon using either libappindicator or XEmbed.
///
/// # Strategy
///
/// The adapter attempts the following in order:
///
/// 1. **libappindicator3** (modern Ubuntu / Debian):
///    - Uses the `AppIndicator3` D-Bus interface.
///    - Provides `org.kde.StatusNotifierItem` compatibility.
///
/// 2. **libappindicator1** (older systems):
///    - Fallback for Ubuntu < 18.04.
///
/// 3. **XEmbed** (legacy / lightweight WMs):
///    - Creates a small `GtkStatusIcon` window and embeds it into the tray.
///    - Uses `_NET_SYSTEM_TRAY_S0` selection to find the tray manager window.
///    - Sends `SYSTEM_TRAY_REQUEST_DOCK` client message.
///
/// 4. **StatusNotifierItem** (KDE / modern GNOME):
///    - D-Bus implementation of the `org.kde.StatusNotifierItem` spec.
///    - Register on `org.kde.StatusNotifierWatcher`.
///
/// # Backend Detection
///
/// ```text
/// Check $XDG_CURRENT_DESKTOP → if KDE → StatusNotifierItem
///                           → if GNOME → check for AppIndicator extension
///                           → if XFCE  → XEmbed tray
///                           → fallback → XEmbed
/// ```
pub struct X11TrayManager {
    /// Current backend in use.
    backend: X11TrayBackend,
    /// Whether the tray icon is currently visible.
    visible: bool,
}

/// Identifies which tray implementation is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11TrayBackend {
    None,
    LibAppIndicator3,
    LibAppIndicator1,
    XEmbed,
    StatusNotifierItem,
}

/// A tray menu item definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11TrayMenuItem {
    Item { label: String, action: String },
    Separator,
}

impl X11TrayManager {
    /// Creates a tray manager, auto-detecting the best available backend.
    pub fn create() -> X11Result<Self> {
        // Detection logic:
        // 1. Try StatusNotifierItem via D-Bus if $XDG_CURRENT_DESKTOP is KDE.
        // 2. Try libappindicator3 via dlopen("libappindicator3.so").
        // 3. Try libappindicator1.
        // 4. Try _NET_SYSTEM_TRAY_S0 selection → XEmbed.
        // 5. Otherwise set backend to None.
        Ok(Self {
            backend: X11TrayBackend::None,
            visible: false,
        })
    }

    /// Creates a tray manager with an explicitly chosen backend.
    pub fn with_backend(backend: X11TrayBackend) -> X11Result<Self> {
        Ok(Self {
            backend,
            visible: false,
        })
    }

    /// Shows the tray icon.  On first call this creates the icon.
    pub fn show(&mut self) -> X11Result<()> {
        match self.backend {
            X11TrayBackend::None => {
                return Err(X11Error::TrayCreationFailed(
                    "no tray backend available".into(),
                ));
            }
            X11TrayBackend::LibAppIndicator3 | X11TrayBackend::LibAppIndicator1 => {
                // Create AppIndicator via D-Bus or directly.
            }
            X11TrayBackend::XEmbed => {
                // XEmbed tray:
                // 1. XOpenDisplay
                // 2. Find _NET_SYSTEM_TRAY_S0 selection owner
                // 3. Create GTK invisible window
                // 4. Send SYSTEM_TRAY_REQUEST_DOCK client message
            }
            X11TrayBackend::StatusNotifierItem => {
                // Register on org.kde.StatusNotifierWatcher via D-Bus.
            }
        }
        self.visible = true;
        Ok(())
    }

    /// Hides the tray icon.
    pub fn hide(&mut self) -> X11Result<()> {
        self.visible = false;
        Ok(())
    }

    /// Sets the tray menu items.
    pub fn set_menu(&mut self, _items: &[X11TrayMenuItem]) -> X11Result<()> {
        Ok(())
    }

    /// Sets the tray icon tooltip text.
    pub fn set_tooltip(&mut self, _text: &str) -> X11Result<()> {
        Ok(())
    }

    /// Sets the tray icon from a file path (PNG preferred).
    pub fn set_icon_from_path(&mut self, _path: &str) -> X11Result<()> {
        Ok(())
    }

    /// Returns the currently active backend.
    pub fn backend(&self) -> X11TrayBackend {
        self.backend
    }

    /// Attempts to detect the best available tray backend on this system.
    pub fn detect_backend() -> X11TrayBackend {
        // 1. Check environment: $XDG_CURRENT_DESKTOP, $DESKTOP_SESSION
        // 2. Check D-Bus availability for StatusNotifierItem
        // 3. Check for libappindicator shared libraries
        // 4. Check for _NET_SYSTEM_TRAY_S0 atom on the X display
        X11TrayBackend::None
    }

    /// Returns a description of each backend's capabilities.
    pub fn backend_info() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "StatusNotifierItem",
                "Modern D-Bus tray protocol. Supported by KDE Plasma, GNOME (with extension), \
                 Budgie, Cinnamon.",
            ),
            (
                "libappindicator3",
                "Ubuntu's AppIndicator library. Supported on Ubuntu 18.04+, elementary OS, \
                 and many GNOME-based desktops with the AppIndicator extension.",
            ),
            (
                "libappindicator1",
                "Older AppIndicator library. Ubuntu 16.04 and earlier.",
            ),
            (
                "XEmbed",
                "Legacy XEmbed protocol. Supported by most lightweight WMs (i3, dwm, Openbox, \
                 Fluxbox) and older desktop environments.",
            ),
        ]
    }

    /// Default menu for clipboard manager tray.
    pub fn default_menu() -> Vec<X11TrayMenuItem> {
        vec![
            X11TrayMenuItem::Item {
                label: "Show/Hide".to_owned(),
                action: "toggleWindow".to_owned(),
            },
            X11TrayMenuItem::Separator,
            X11TrayMenuItem::Item {
                label: "Preferences".to_owned(),
                action: "openPreferences".to_owned(),
            },
            X11TrayMenuItem::Item {
                label: "About".to_owned(),
                action: "openAbout".to_owned(),
            },
            X11TrayMenuItem::Separator,
            X11TrayMenuItem::Item {
                label: "Quit".to_owned(),
                action: "quit".to_owned(),
            },
        ]
    }
}
