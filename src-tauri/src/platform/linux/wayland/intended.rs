//! Documented intended flows for Wayland features that are **not wired into
//! the runtime yet**: clipboard monitoring, Portal global-shortcut
//! registration, the StatusNotifierItem tray, and the per-compositor
//! compatibility matrix.
//!
//! The live paths live elsewhere in this module: `clipboard.rs` (read/write via
//! `wl-paste`/`wl-copy`), `monitor.rs` (data-control change notifications),
//! `hotkeys.rs` (GlobalShortcuts portal sessions) and `paste.rs` (quick paste).
//! Everything here is compiled only so the recorded design intent stays honest
//! and is exercised by `tests.rs` — do not report it as shipped behavior.

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use crate::keyboard::ShortcutBinding;

use super::caps::WaylandCapabilities;

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
