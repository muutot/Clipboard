//! Wayland capability model: what the running compositor supports.
//!
//! Consumed by `platform_info` to populate `RuntimeInfo.wayland_capabilities`;
//! this is live code, but note that `detect()` still returns the conservative
//! `unknown()` profile until compositor probing is implemented.

use serde::Serialize;

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
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let sway_socket = std::env::var_os("SWAYSOCK").is_some();
        let hyprland_signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();

        Self::classify(&session_type, &desktop, sway_socket, hyprland_signature)
    }

    /// Maps session and compositor signals onto a capability profile.
    ///
    /// Split out from `detect()` so the mapping is testable without mutating
    /// the process environment. The socket flags are the tie-breakers for
    /// compositors whose `XDG_CURRENT_DESKTOP` is generic or unset.
    pub fn classify(
        session_type: &str,
        desktop: &str,
        sway_socket: bool,
        hyprland_signature: bool,
    ) -> Self {
        if !session_type.eq_ignore_ascii_case("wayland") {
            return Self::unknown();
        }

        let desktop = desktop.to_ascii_lowercase();

        if hyprland_signature || desktop.contains("hyprland") {
            return Self {
                compositor: "Hyprland".into(),
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: true,
                global_shortcuts: true,
                system_tray: true,
                requires_config: false,
                notes: vec!["Global shortcuts are configured in hyprland.conf.".into()],
            };
        }

        if sway_socket || desktop.contains("sway") {
            return Self::wlroots_based("Sway");
        }

        if desktop.contains("kde") || desktop.contains("plasma") {
            return Self {
                compositor: "KDE Plasma".into(),
                clipboard_read: true,
                clipboard_write: true,
                primary_selection: false,
                global_shortcuts: true,
                system_tray: true,
                requires_config: false,
                notes: vec!["Global shortcuts are registered through KGlobalAccel.".into()],
            };
        }

        if desktop.contains("gnome") {
            return Self {
                compositor: "GNOME Shell".into(),
                clipboard_read: false,
                clipboard_write: false,
                primary_selection: false,
                global_shortcuts: true,
                system_tray: false,
                requires_config: true,
                notes: vec![
                    "Clipboard access is restricted in GNOME Wayland. Install a clipboard \
                     provider extension."
                        .into(),
                    "System tray requires the 'AppIndicator' GNOME Shell extension.".into(),
                ],
            };
        }

        Self::unverified(&desktop)
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

    /// Returns the conservative profile for a compositor outside the matrix.
    ///
    /// Keeps the `unknown()` flags — nothing about this compositor has been
    /// verified — but carries the detected desktop name so `platform_notes` can
    /// name what the session actually reported.
    pub fn unverified(compositor: &str) -> Self {
        if compositor.is_empty() {
            return Self::unknown();
        }
        Self {
            compositor: format!("{compositor} (unverified)"),
            ..Self::unknown()
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
