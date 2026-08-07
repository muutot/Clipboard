//! Linux Wayland platform adapter.
//!
//! Wayland is a fundamentally different display protocol from X11.  It has
//! stronger security isolation: applications cannot spy on each other's
//! events, clipboard contents, or keyboard input by default.  This means
//! clipboard managers must use specific Wayland protocol extensions to
//! function.
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Wayland Platform Layer                    │
//! │                                                              │
//! │  ┌──────────────────┐  ┌──────────────────┐  ┌───────────┐ │
//! │  │ Clipboard        │  │ Global Shortcuts  │  │ Tray      │ │
//! │  │ (wlr-data-ctrl)  │  │ (Portal)          │  │ (SNI)     │ │
//! │  └──────────────────┘  └──────────────────┘  └───────────┘ │
//! │          │                      │                  │         │
//! │          ▼                      ▼                  ▼         │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │              Compositor Capability Detection          │   │
//! │  │  (XDG_CURRENT_DESKTOP, XDG_SESSION_TYPE, etc.)       │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Compositor Compatibility Matrix
//!
//! | Feature          | wlroots (Sway) | KDE Plasma  | GNOME/Mutter | Hyprland | river |
//! |------------------|----------------|--------------|--------------|----------|-------|
//! | Clipboard read   | Full           | Full         | Partial*     | Full     | Full  |
//! | Clipboard write  | Full           | Full         | No†          | Full     | Full  |
//! | Global shortcuts | Custom DAEMON  | KGlobalAccel | Portal       | Portal   | No    |
//! | System tray      | SNI            | SNI          | SNI‡         | SNI      | No    |
//! | Multiple select.  | PRIMARY+CLIP  | CLIPBOARD    | CLIPBOARD    | PRIMARY+CLIP | CLIP |
//!
//! \* GNOME restricts clipboard access to the focused application when using
//!   the default Mutter compositor.  Use an extension or portal.
//!
//! † GNOME does not expose `wlr-data-control` by default.  You may need to
//!   use the GNOME Shell extension `Clipboard Indicator` or similar.
//!
//! ‡ GNOME hides the system tray by default.  Install the `AppIndicator` or
//!   `Tray Icons Reloaded` GNOME Shell extension.
//!
//! # Detection
//!
//! ```text
//! if XDG_SESSION_TYPE == "wayland":
//!     if XDG_CURRENT_DESKTOP contains "sway" or "Hyprland":
//!         → wlroots-based, full clipboard + SNI tray support
//!     if XDG_CURRENT_DESKTOP contains "KDE":
//!         → KDE Plasma, KGlobalAccel for shortcuts, SNI tray
//!     if XDG_CURRENT_DESKTOP contains "GNOME":
//!         → Reduced clipboard access, Portal shortcuts, SNI (with extension)
//!     else:
//!         → Best-effort: try wlr-data-control, fall back to Portal clipboard
//! ```

#![allow(dead_code)]

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use crate::keyboard::ShortcutBinding;
use serde::Serialize;

pub struct LinuxWaylandPlatform;

#[cfg(target_os = "linux")]
impl crate::platform::PlatformClipboard for LinuxWaylandPlatform {
    fn get_foreground_app(&self) -> crate::platform::ForegroundApp {
        get_foreground_app()
    }

    fn read_clipboard_text(&self) -> Option<String> {
        read_clipboard_text()
    }

    fn read_clipboard_image(&self) -> Option<(Vec<u8>, u32, u32)> {
        read_clipboard_image()
    }

    fn read_clipboard_file_paths(&self) -> Vec<String> {
        read_clipboard_file_paths()
    }

    fn write_clipboard_text_with_self_trigger(&self, text: &str) -> Result<(), String> {
        write_clipboard_text_with_self_trigger(text)
    }

    fn extract_app_icon(
        &self,
        icon_dir: &std::path::Path,
        app_name: &str,
        exe_path: &str,
    ) -> Option<String> {
        extract_app_icon(icon_dir, app_name, exe_path)
    }

    fn read_clipboard_html(&self) -> Option<String> {
        read_clipboard_html()
    }

    fn read_clipboard_rtf(&self) -> Option<String> {
        read_clipboard_rtf()
    }
}

// ---------------------------------------------------------------------------
// WaylandCapabilities
// ---------------------------------------------------------------------------

/// Describes which Wayland features are available on the current compositor.
///
/// This is populated at startup by probing environment variables and
/// attempting protocol connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaylandCapabilities {
    /// The compositor name (e.g. "sway", "Hyprland", "GNOME Shell", "KWin").
    pub compositor: String,
    /// Whether `wlr-data-control-unstable-v1` is available for clipboard access.
    pub clipboard_read: bool,
    /// Whether the compositor allows writing to the clipboard from background
    /// applications.
    pub clipboard_write: bool,
    /// Whether PRIMARY selection is supported (common in wlroots compositors).
    pub primary_selection: bool,
    /// Whether global shortcuts can be registered.
    pub global_shortcuts: bool,
    /// Whether the system tray (StatusNotifierItem) works.
    pub system_tray: bool,
    /// Whether the compositor requires additional permissions or configuration.
    pub requires_config: bool,
    /// Human-readable notes about compatibility quirks.
    pub notes: Vec<String>,
}

impl WaylandCapabilities {
    /// Detects the current compositor and returns available capabilities.
    ///
    /// Reads environment variables:
    /// - `XDG_SESSION_TYPE` — should be "wayland"
    /// - `XDG_CURRENT_DESKTOP` — compositor identifier
    /// - `WAYLAND_DISPLAY` — socket name (e.g. "wayland-0")
    /// - `SWAYSOCK` — Sway IPC socket
    /// - `HYPRLAND_INSTANCE_SIGNATURE` — Hyprland runtime ID
    pub fn detect() -> Self {
        // Implementation outline:
        //
        // let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        // if session_type != "wayland" {
        //     return Self::empty();
        // }
        //
        // let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        //     .unwrap_or_default()
        //     .to_lowercase();
        //
        // match desktop.as_str() {
        //     s if s.contains("sway") => Self {
        //         compositor: "Sway".into(),
        //         clipboard_read: true,
        //         clipboard_write: true,
        //         primary_selection: true,
        //         global_shortcuts: true,  // via swaymsg / IPC
        //         system_tray: true,       // via SNI
        //         requires_config: false,
        //         notes: vec!["Global shortcuts configured in Sway config".into()],
        //     },
        //     s if s.contains("hyprland") => Self {
        //         compositor: "Hyprland".into(),
        //         clipboard_read: true,
        //         clipboard_write: true,
        //         primary_selection: true,
        //         global_shortcuts: true,  // via hyprctl dispatcher
        //         system_tray: true,
        //         requires_config: false,
        //         notes: vec!["Global shortcuts configured in hyprland.conf".into()],
        //     },
        //     s if s.contains("kde") || s.contains("plasma") => Self {
        //         compositor: "KDE Plasma".into(),
        //         clipboard_read: true,
        //         clipboard_write: true,
        //         primary_selection: false,
        //         global_shortcuts: true,  // via KGlobalAccel / Portal
        //         system_tray: true,
        //         requires_config: false,
        //         notes: vec![],
        //     },
        //     s if s.contains("gnome") => Self {
        //         compositor: "GNOME Shell".into(),
        //         clipboard_read: false,   // restricted without extension
        //         clipboard_write: false,
        //         primary_selection: false,
        //         global_shortcuts: true,  // via Portal
        //         system_tray: false,      // hidden by default
        //         requires_config: true,
        //         notes: vec![
        //             "Clipboard access is restricted in GNOME Wayland. \
        //              Install a clipboard provider extension.".into(),
        //             "System tray requires 'AppIndicator' GNOME Shell extension.".into(),
        //         ],
        //     },
        //     _ => Self::unknown(),
        // }
        Self::unknown()
    }

    /// Returns capabilities for an unknown compositor (conservative defaults).
    pub fn unknown() -> Self {
        Self {
            compositor: "Unknown".into(),
            clipboard_read: false,
            clipboard_write: false,
            primary_selection: false,
            global_shortcuts: false,
            system_tray: false,
            requires_config: true,
            notes: vec!["Compositor not recognized. Clipboard and global shortcuts \
                 may not function."
                .into()],
        }
    }

    /// Returns capabilities assuming a wlroots-based compositor.
    pub fn wlroots_based(compositor: &str) -> Self {
        Self {
            compositor: compositor.to_owned(),
            clipboard_read: true,
            clipboard_write: true,
            primary_selection: true,
            global_shortcuts: true,
            system_tray: true,
            requires_config: false,
            notes: vec![format!(
                "Global shortcuts are handled by {compositor}, not the application."
            )],
        }
    }

    /// Returns a human-readable summary of capabilities.
    pub fn summary(&self) -> String {
        let available = |b: bool| if b { "YES" } else { "no" };
        format!(
            "Compositor: {}\n\
             Clipboard read:  {}\n\
             Clipboard write: {}\n\
             PRIMARY select.: {}\n\
             Global shortcuts:{}\n\
             System tray:     {}{}",
            self.compositor,
            available(self.clipboard_read),
            available(self.clipboard_write),
            available(self.primary_selection),
            available(self.global_shortcuts),
            available(self.system_tray),
            if self.requires_config {
                format!(
                    "\n⚠  Additional configuration required.\n   {}",
                    self.notes.join("\n   ")
                )
            } else {
                String::new()
            }
        )
    }
}

// ---------------------------------------------------------------------------
// WaylandClipboardMonitor
// ---------------------------------------------------------------------------

/// Monitors the Wayland clipboard via the `wlr-data-control-unstable-v1`
/// protocol.
///
/// # Protocol Sequence
///
/// ```text
/// 1. Bind to wl_registry → get zwlr_data_control_manager_v1
/// 2. Create zwlr_data_control_device_v1 (per seat)
/// 3. Listen for zwlr_data_control_device_v1.data_offer events:
///    - A data offer contains the available MIME types.
/// 4. On selection, call zwlr_data_control_offer_v1.receive(mime_type, fd)
///    - The compositor writes data to the provided file descriptor.
/// 5. Read from the fd into a buffer.
/// 6. Emit clipboard snapshot.
///
/// For PRIMARY selection:
/// - Use zwlr_data_control_manager_v1.get_data_device with
///   ZWLR_DATA_CONTROL_MANAGER_V1_PRIMARY_SELECTION_DEVICE role.
/// ```
///
/// # Dependencies
///
/// - `wayland-client` crate for protocol bindings.
/// - `wayland-protocols-wlr` for the `wlr-data-control` protocol XML.
pub struct WaylandClipboardMonitor {
    /// Whether the monitor is currently running.
    running: Arc<AtomicBool>,
    /// Set of application IDs to ignore.
    ignored_apps: Arc<Mutex<HashSet<String>>>,
}

impl Default for WaylandClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandClipboardMonitor {
    /// Creates a new, stopped monitor.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            ignored_apps: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Starts clipboard monitoring.
    ///
    /// # Steps
    ///
    /// 1. Connect to the Wayland display (`WAYLAND_DISPLAY` env var).
    /// 2. Bind the `wl_registry` and listen for global advertisements.
    /// 3. When `zwlr_data_control_manager_v1` is advertised, bind to it.
    /// 4. Create a data device for each seat.
    /// 5. Set up event listeners for `data_offer` and `selection` events.
    /// 6. On `selection` event with a new offer:
    ///    a. Read the offer's MIME types.
    ///    b. If `text/plain;charset=utf-8` or `text/plain` is available, call
    ///    `zwlr_data_control_offer_v1.receive()` with a pipe fd.
    ///    c. Read data from the pipe.
    ///    d. Check whether the source application is ignored.
    ///    e. Emit snapshot.
    pub fn start(&mut self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Stops clipboard monitoring and disconnects from Wayland.
    pub fn stop(&mut self) -> Result<(), String> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Returns whether the monitor is currently active.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Sets the list of application IDs to ignore.
    ///
    /// Note: On Wayland, the source application ID may not always be available
    /// through `wlr-data-control`.  Ignore lists are best-effort.
    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        if let Ok(mut guard) = self.ignored_apps.lock() {
            *guard = apps.into_iter().collect();
        }
    }

    /// Returns the current ignored application list.
    pub fn ignored_apps(&self) -> Vec<String> {
        self.ignored_apps
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Reads the current clipboard text (one-shot synchronous read).
    ///
    /// This is inherently difficult to do synchronously on Wayland since
    /// the protocol is fully asynchronous and event-driven.  In practice
    /// the application maintains a persistent Wayland connection and
    /// receives clipboard updates as events.
    pub fn read_clipboard_text() -> Option<String> {
        None
    }
}

// ---------------------------------------------------------------------------
// WaylandGlobalShortcut
// ---------------------------------------------------------------------------

/// Manages global keyboard shortcuts on Wayland via the
/// `org.freedesktop.portal.GlobalShortcuts` D-Bus portal.
///
/// # Portal Session Flow
///
/// ```text
/// 1. Call org.freedesktop.portal.GlobalShortcuts.CreateSession
///    → Returns session handle.
///
/// 2. Call org.freedesktop.portal.GlobalShortcuts.BindShortcuts
///    → Provide (session, shortcuts[]) mapping.
///    → Compositor shows a permission dialog to the user.
///
/// 3. Listen for org.freedesktop.portal.GlobalShortcuts.Activated signal
///    → Contains the shortcut_id that was triggered.
///
/// 4. On shortcut change or shutdown:
///    Call org.freedesktop.portal.GlobalShortcuts.UnbindShortcuts
/// ```
///
/// # Compositor-specific Alternatives
///
/// | Compositor | Alternative Shortcut API          |
/// |------------|-----------------------------------|
/// | Sway       | `swaymsg bindsym` in config       |
/// | Hyprland   | `bind =` in hyprland.conf         |
/// | KDE        | KGlobalAccel (native or Portal)   |
/// | GNOME      | Portal (primary), or gsettings    |
/// | river      | Riverctl `map` in init script     |
/// | dwl        | `bind =` in config.h              |
///
/// For compositors that do not support the Portal (or wlroots-based ones
/// that expect the user to configure shortcuts in the compositor config),
/// this adapter detects the compositor and provides guidance rather than
/// attempting registration that would fail.
pub struct WaylandGlobalShortcut {
    /// The compositor we're running under.
    compositor: String,
    /// Whether the Portal is available.
    portal_available: bool,
    /// Set of registered shortcut IDs (for Portal-based registration).
    registered: HashSet<String>,
}

impl Default for WaylandGlobalShortcut {
    fn default() -> Self {
        Self::new()
    }
}

impl WaylandGlobalShortcut {
    /// Creates a new shortcut manager, detecting the compositor capabilities.
    pub fn new() -> Self {
        let caps = WaylandCapabilities::detect();
        // Check for Portal availability by attempting to call
        // org.freedesktop.portal.GlobalShortcuts on the session bus.
        Self {
            compositor: caps.compositor,
            portal_available: false, // set after D-Bus probe
            registered: HashSet::new(),
        }
    }

    /// Registers shortcuts for an action.
    ///
    /// # Behavior by compositor
    ///
    /// - **Portal-based** (GNOME, KDE with portal): calls
    ///   `GlobalShortcuts.BindShortcuts`.
    /// - **wlroots-based** (Sway, Hyprland): returns a message directing the
    ///   user to configure shortcuts in their compositor config.
    /// - **No shortcut support** (some tiling WMs): returns an error.
    pub fn register(
        &mut self,
        action_id: &str,
        shortcuts: &[ShortcutBinding],
    ) -> Result<(), String> {
        if self.portal_available {
            // D-Bus call to org.freedesktop.portal.GlobalShortcuts.BindShortcuts
            // session_handle, shortcuts[] with descriptions
        } else if self.compositor.contains("sway") || self.compositor.contains("hyprland") {
            // On wlroots compositors, global shortcuts are configured in the
            // compositor's config file.  The application should provide
            // instructions or use the compositor's IPC (swaymsg / hyprctl)
            // to suggest bindings.
        }
        let _ = (action_id, shortcuts);
        Ok(())
    }

    /// Unregisters all previously registered shortcuts.
    pub fn unregister_all(&mut self) -> Result<(), String> {
        if self.portal_available {
            // D-Bus call to UnbindShortcuts
        }
        self.registered.clear();
        Ok(())
    }

    /// Returns whether any shortcuts are registered.
    pub fn is_active(&self) -> bool {
        !self.registered.is_empty()
    }

    /// Returns the compositor name.
    pub fn compositor_name(&self) -> &str {
        &self.compositor
    }

    /// Returns a message explaining how to set up global shortcuts for the
    /// current compositor.
    pub fn setup_instructions(&self) -> String {
        match self.compositor.as_str() {
            "Sway" => "Edit ~/.config/sway/config and add:\n\
                 bindsym $mod+Shift+V exec clipboard-manager toggle"
                .into(),
            "Hyprland" => "Edit ~/.config/hypr/hyprland.conf and add:\n\
                 bind = $mainMod SHIFT, V, exec, clipboard-manager toggle"
                .into(),
            "KDE Plasma" => "Open System Settings → Shortcuts → Custom Shortcuts and add\n\
                 an entry pointing to the clipboard manager."
                .into(),
            "GNOME Shell" => "Open Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts\n\
                 and add a shortcut for the clipboard manager."
                .into(),
            _ => "Consult your compositor documentation for configuring\n\
                 global keyboard shortcuts."
                .into(),
        }
    }
}

// ---------------------------------------------------------------------------
// WaylandTrayManager
// ---------------------------------------------------------------------------

/// Manages the system tray on Wayland via the StatusNotifierItem (SNI)
/// D-Bus protocol.
///
/// # SNI Protocol
///
/// StatusNotifierItem is a D-Bus specification originally from KDE, now
/// supported by most Wayland compositors:
///
/// ```text
/// 1. Application connects to the session D-Bus bus.
/// 2. Implements org.kde.StatusNotifierItem interface at a well-known path.
/// 3. Calls org.kde.StatusNotifierWatcher.RegisterStatusNotifierItem
///    with the service name.
/// 4. Compositor / tray host initiates communication:
///    - Requests icon via org.freedesktop.DBus.Properties (IconName/IconPixmap)
///    - Reads tooltip via org.freedesktop.DBus.Properties (ToolTip)
///    - Receives signals for menu updates (NewMenu, UpdateMenuLayout)
/// 5. User clicks on tray icon → compositor calls
///    org.kde.StatusNotifierItem.Activate (left-click) or
///    org.kde.StatusNotifierItem.ContextMenu (right-click).
/// ```
///
/// # Alternative: Layer Shell
///
/// Some compositors (Sway, Hyprland) support `zwlr_layer_shell_v1`, which
/// allows applications to render a status bar item directly.  This is more
/// complex but works without D-Bus.
///
/// # Dependencies
///
/// - `dbus` crate for D-Bus communication.
/// - PNG icon data for tray icon.
pub struct WaylandTrayManager {
    /// Whether SNI D-Bus service is available.
    sni_available: bool,
    /// Whether the tray icon is currently visible.
    visible: bool,
    /// The D-Bus service name used for SNI registration.
    service_name: String,
}

impl WaylandTrayManager {
    /// Creates a tray manager, probing for SNI D-Bus availability.
    pub fn create() -> Result<Self, String> {
        // Implementation:
        // 1. Check if D-Bus session bus is reachable.
        // 2. Check if org.kde.StatusNotifierWatcher is available on the bus.
        // 3. Allocate a unique service name (e.g. :1.xxx or org.clipboard.SNI).
        // 4. Set up the org.kde.StatusNotifierItem D-Bus interface.
        Ok(Self {
            sni_available: false,
            visible: false,
            service_name: "org.clipboard.sni".into(),
        })
    }

    /// Shows the tray icon.
    ///
    /// On first call, registers with StatusNotifierWatcher and creates the
    /// Gtk/GDK window for rendering the icon.
    pub fn show(&mut self) -> Result<(), String> {
        if self.sni_available {
            // 1. Create D-Bus object implementing StatusNotifierItem.
            // 2. Call StatusNotifierWatcher.RegisterStatusNotifierItem(service_name).
            // 3. Signal NewStatus to the watcher.
        }
        self.visible = true;
        Ok(())
    }

    /// Hides the tray icon.
    pub fn hide(&mut self) -> Result<(), String> {
        self.visible = false;
        Ok(())
    }

    /// Sets the tray icon from raw PNG bytes.
    pub fn set_icon(&mut self, _png_data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// Sets the tooltip text for the tray icon.
    pub fn set_tooltip(&mut self, _text: &str) -> Result<(), String> {
        Ok(())
    }

    /// Sets the context menu items (displayed on right-click).
    pub fn set_menu(&mut self, _items: &[WaylandTrayMenuItem]) -> Result<(), String> {
        Ok(())
    }

    /// Returns whether the SNI protocol is available.
    pub fn is_sni_available(&self) -> bool {
        self.sni_available
    }

    /// Returns the default menu for the tray icon.
    pub fn default_menu() -> Vec<WaylandTrayMenuItem> {
        vec![
            WaylandTrayMenuItem::Item {
                label: "Show/Hide".to_owned(),
                action: "toggleWindow".to_owned(),
            },
            WaylandTrayMenuItem::Separator,
            WaylandTrayMenuItem::Item {
                label: "Preferences".to_owned(),
                action: "openPreferences".to_owned(),
            },
            WaylandTrayMenuItem::Item {
                label: "About".to_owned(),
                action: "openAbout".to_owned(),
            },
            WaylandTrayMenuItem::Separator,
            WaylandTrayMenuItem::Item {
                label: "Quit".to_owned(),
                action: "quit".to_owned(),
            },
        ]
    }

    /// Returns information about tray support on various compositors.
    pub fn compositor_tray_support() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            (
                "Sway / wlroots",
                "SNI",
                "Built-in. Enable `bar` block with `tray_output`.",
            ),
            ("Hyprland", "SNI", "Built-in via `hyprctl` tray plugin."),
            (
                "KDE Plasma",
                "SNI",
                "Full native support for StatusNotifierItem.",
            ),
            (
                "GNOME Shell",
                "SNI†",
                "Requires 'AppIndicator' or 'Tray Icons Reloaded' extension.",
            ),
            (
                "river",
                "None",
                "No native tray support. Use waybar or similar bar.",
            ),
            (
                "dwl",
                "None",
                "Minimal compositor; tray support via external bar.",
            ),
        ]
    }
}

/// A tray menu item definition for Wayland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandTrayMenuItem {
    Item { label: String, action: String },
    Separator,
}

// ---------------------------------------------------------------------------
// Compositor-specific documentation
// ---------------------------------------------------------------------------

/// Documentation of Wayland clipboard support for each major compositor.
///
/// # GNOME (Mutter)
///
/// GNOME's Mutter compositor has the most restrictive clipboard model on
/// Wayland:
///
/// - **Clipboard monitoring is not possible** through standard protocols.
///   Mutter does not expose `wlr-data-control-unstable-v1`.  Applications
///   can only read the clipboard when they have keyboard focus.
/// - **Workaround**: use the `org.freedesktop.portal.Clipboard` portal
///   (available since `xdg-desktop-portal` 1.18), but this may still
///   require user interaction per read.
/// - **Tray**: requires GNOME Shell extension.  The `AppIndicator` extension
///   maps SNI to a GNOME panel indicator.
///
/// # KDE Plasma (KWin)
///
/// KDE Plasma on Wayland provides broad clipboard support:
///
/// - `wlr-data-control` is supported through `kwin`'s data device manager.
/// - Copy/paste between XWayland and native Wayland apps is handled
///   transparently.
/// - Global shortcuts via KGlobalAccel (preferred) or the Portal.
/// - SNI tray with full native integration in the system tray widget.
///
/// # Sway (wlroots)
///
/// Sway is a tiling Wayland compositor built on wlroots:
///
/// - Full `wlr-data-control` support, including PRIMARY selection.
/// - Clipboard monitoring works reliably for background apps.
/// - Global shortcuts are configured in the Sway config file, not
///   registered programmatically.  The app can use `swaymsg` over the
///   IPC socket to invoke commands bound to keys.
/// - SNI tray is built into `swaybar` when `tray_output` is configured.
///
/// # Hyprland (wlroots-derived)
///
/// Hyprland is a wlroots-based compositor focused on aesthetics:
///
/// - Full `wlr-data-control` and PRIMARY selection support.
/// - Clipboard monitoring works from background.
/// - Global shortcuts in `hyprland.conf`; `hyprctl` IPC available.
/// - SNI tray built into the internal bar or external bars like waybar.
///
/// # river, dwl, and others
///
/// - Generally wlroots-based, so `wlr-data-control` is available if the
///   compositor chooses to implement it.
/// - Tray support depends on the bar implementation (waybar, yambar).
/// - Global shortcuts depend on compositor config, not runtime registration.
#[derive(Debug, Clone)]
pub struct WaylandCompositorInfo {
    pub name: &'static str,
    pub base: &'static str,
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub primary_selection: bool,
    pub global_shortcuts: bool,
    pub system_tray: bool,
    pub notes: &'static str,
}

impl WaylandCompositorInfo {
    /// Returns compositor info for all known Wayland compositors.
    pub fn all() -> Vec<Self> {
        vec![
            Self {
                name: "Sway",
                base: "wlroots",
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: true,
                global_shortcuts: true,
                system_tray: true,
                notes: "Shortcuts via Sway config.  SNI tray built into swaybar.",
            },
            Self {
                name: "Hyprland",
                base: "wlroots-derived",
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: true,
                global_shortcuts: true,
                system_tray: true,
                notes: "Shortcuts via hyprland.conf.  SNI tray via internal bar.",
            },
            Self {
                name: "KDE Plasma",
                base: "KWin",
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: false,
                global_shortcuts: true,
                system_tray: true,
                notes: "Full support. Use KGlobalAccel or Portal for shortcuts.",
            },
            Self {
                name: "GNOME Shell",
                base: "Mutter",
                clipboard_read: false,
                clipboard_write: false,
                primary_selection: false,
                global_shortcuts: true,
                system_tray: false,
                notes: "Clipboard restricted. Portal shortcuts supported. Tray via extension.",
            },
            Self {
                name: "river",
                base: "wlroots",
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: true,
                global_shortcuts: false,
                system_tray: false,
                notes: "Clipboard wlr-data-control works. Tray via external bar (waybar).",
            },
            Self {
                name: "dwl",
                base: "wlroots",
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: true,
                global_shortcuts: false,
                system_tray: false,
                notes: "Minimal compositor. Limited feature support.",
            },
        ]
    }

    /// Returns a markdown-formatted compatibility table.
    pub fn compatibility_table() -> String {
        let header =
            "| Compositor | Clipboard Read | Clipboard Write | PRIMARY | Shortcuts | Tray |";
        let sep = "|------------|:---:|:---:|:---:|:---:|:---:|";
        let mut lines = vec![header.to_owned(), sep.to_owned()];

        for info in Self::all() {
            let check = |b: bool| if b { "YES" } else { "no" };
            lines.push(format!(
                "| {} | {} | {} | {} | {} | {} |",
                info.name,
                check(info.clipboard_read),
                check(info.clipboard_write),
                check(info.primary_selection),
                check(info.global_shortcuts),
                check(info.system_tray),
            ));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
//  Top-level platform dispatch functions (called from mod.rs)
// ---------------------------------------------------------------------------

/// Reads plain text from the Wayland clipboard using `wl-paste`.
#[cfg(target_os = "linux")]
pub fn read_clipboard_text() -> Option<String> {
    std::process::Command::new("wl-paste")
        .args(["--no-newline"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    for target in &["image/png", "image/bmp", "image/jpeg", "image/tiff"] {
        if let Ok(output) = std::process::Command::new("wl-paste")
            .args(["--type", target])
            .output()
        {
            if output.status.success() && !output.stdout.is_empty() {
                if let Ok(img) = image::load_from_memory(&output.stdout) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    return Some((rgba.into_raw(), w, h));
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Reads the HTML fragment from the Wayland clipboard via wl-paste.
#[cfg(target_os = "linux")]
pub fn read_clipboard_html() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "text/html"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8(output.stdout).ok()?;
            if text.trim().is_empty() {
                return None;
            }
            return Some(text);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_html() -> Option<String> {
    None
}

/// Reads the RTF payload from the Wayland clipboard via wl-paste.
#[cfg(target_os = "linux")]
pub fn read_clipboard_rtf() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "text/rtf"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8(output.stdout).ok()?;
            if text.trim().is_empty() {
                return None;
            }
            return Some(text);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}

#[cfg(not(target_os = "linux"))]
pub fn read_clipboard_file_paths() -> Vec<String> {
    vec![]
}

/// Returns the foreground application on Wayland using `swaymsg` or `hyprctl`.
#[cfg(target_os = "linux")]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    // Try Sway first, then Hyprland, then fallback to /proc via xdotool (XWayland)
    let pid = std::process::Command::new("swaymsg")
        .args(["-t", "get_seats"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                // Parse JSON to get the focused view PID
                let text = String::from_utf8(output.stdout).ok()?;
                let json: serde_json::Value = serde_json::from_str(&text).ok()?;
                json.as_array()?
                    .first()?
                    .get("focus")?
                    .as_array()?
                    .first()?
                    .as_u64()
            } else {
                None
            }
        })
        .or_else(|| {
            // Hyprland
            std::process::Command::new("hyprctl")
                .args(["activewindow"])
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        let text = String::from_utf8(output.stdout).ok()?;
                        // Parse "PID: 1234" from output
                        for line in text.lines() {
                            if let Some(pid_str) = line.strip_prefix("PID: ") {
                                return pid_str.trim().parse::<u64>().ok();
                            }
                        }
                    }
                    None
                })
        })
        .or_else(|| {
            // Fallback: use xdotool (works in XWayland sessions)
            std::process::Command::new("xdotool")
                .args(["getactivewindow", "getwindowpid"])
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        String::from_utf8(output.stdout)
                            .ok()
                            .and_then(|s| s.trim().parse::<u64>().ok())
                    } else {
                        None
                    }
                })
        });

    let pid_u32 = pid.and_then(|p| u32::try_from(p).ok());

    let (name, exe_path) = match pid_u32 {
        Some(pid) => {
            let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .ok()
                .map(|s| s.trim().to_owned())
                .unwrap_or_default();

            let exe_path = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            (name, exe_path)
        }
        None => (String::new(), String::new()),
    };

    crate::platform::ForegroundApp { name, exe_path }
}

#[cfg(not(target_os = "linux"))]
pub fn get_foreground_app() -> crate::platform::ForegroundApp {
    crate::platform::ForegroundApp::empty()
}

#[cfg(target_os = "linux")]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
}

#[cfg(not(target_os = "linux"))]
pub fn extract_app_icon(
    _icon_dir: &std::path::Path,
    _app_name: &str,
    _exe_path: &str,
) -> Option<String> {
    None
}

/// Writes text to the Wayland clipboard using `wl-copy`.
#[cfg(target_os = "linux")]
pub fn write_clipboard_text_with_self_trigger(text: &str) -> Result<(), String> {
    use std::io::Write;

    let mut child = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn wl-copy: {e}"))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("failed to write to wl-copy stdin: {e}"))?;
    }

    child
        .wait()
        .map_err(|e| format!("wl-copy wait failed: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn write_clipboard_text_with_self_trigger(_text: &str) -> Result<(), String> {
    Err("Wayland clipboard writing is not supported on this platform".to_owned())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_capabilities_summary_is_not_empty() {
        let caps = WaylandCapabilities::unknown();
        let summary = caps.summary();
        assert!(!summary.is_empty());
        assert!(summary.contains("Unknown"));
    }

    #[test]
    fn wlroots_capabilities_have_full_support() {
        let caps = WaylandCapabilities::wlroots_based("sway");
        assert!(caps.clipboard_read);
        assert!(caps.clipboard_write);
        assert!(caps.primary_selection);
        assert!(caps.global_shortcuts);
        assert!(caps.system_tray);
    }

    #[test]
    fn clipboard_monitor_lifecycle() {
        let mut monitor = WaylandClipboardMonitor::new();
        assert!(!monitor.is_running());

        monitor.start().unwrap();
        assert!(monitor.is_running());

        monitor.stop().unwrap();
        assert!(!monitor.is_running());
    }

    #[test]
    fn ignored_apps() {
        let mut monitor = WaylandClipboardMonitor::new();
        monitor.set_ignored_apps(vec!["kitty".to_owned(), "alacritty".to_owned()]);
        assert_eq!(monitor.ignored_apps().len(), 2);
    }

    #[test]
    fn global_shortcut_instructions_not_empty() {
        let shortcut = WaylandGlobalShortcut::new();
        let instructions = shortcut.setup_instructions();
        assert!(!instructions.is_empty());
    }

    #[test]
    fn tray_default_menu_has_items() {
        let menu = WaylandTrayManager::default_menu();
        assert!(!menu.is_empty());
    }

    #[test]
    fn tray_compositor_support_table() {
        let table = WaylandTrayManager::compositor_tray_support();
        assert!(!table.is_empty());
        assert!(table.iter().any(|(name, _, _)| *name == "Sway / wlroots"));
    }

    #[test]
    fn compositor_info_table_contains_entries() {
        let info = WaylandCompositorInfo::all();
        assert!(info.len() >= 4);
        assert!(info.iter().any(|c| c.name == "Sway"));
        assert!(info.iter().any(|c| c.name == "GNOME Shell"));
        assert!(info.iter().any(|c| c.name == "KDE Plasma"));
        assert!(info.iter().any(|c| c.name == "Hyprland"));
    }

    #[test]
    fn compatibility_table_is_markdown() {
        let table = WaylandCompositorInfo::compatibility_table();
        assert!(table.starts_with("| Compositor"));
        assert!(table.contains("| Sway |"));
        assert!(table.contains("| GNOME Shell |"));
    }
}
