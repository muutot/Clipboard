//! Standalone float-panel window, owned by the backend so it never depends
//! on the main window's frontend state. The tray menu, the global shortcut,
//! and the in-app shortcut all funnel through [`toggle_float_panel`].

use std::sync::Mutex;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::config::ConfigStore;

pub const FLOAT_WINDOW_LABEL: &str = "float";
const FLOAT_WIDTH: f64 = 320.0;
const FLOAT_HEIGHT: f64 = 480.0;

/// Shows or hides the float panel, mirroring the main-window toggle: a
/// visible and focused panel is hidden, otherwise it is shown (built on
/// first use) and focused. Only the float panel's own visibility changes;
/// the main window is never touched. Every entry point (tray, global
/// hotkey, in-app shortcut) funnels through this single toggle so
/// behavior is uniform.
#[tauri::command]
pub fn toggle_float_panel<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    crate::log_event!("[float] toggle requested");
    if let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);
        crate::log_event!("[float] existing visible: {is_visible}, focused: {is_focused}");
        if is_visible && is_focused {
            window
                .hide()
                .map_err(|error| format!("failed to hide the float panel: {error}"))?;
            crate::log_event!("[float] toggle done");
            return Ok(());
        }
    }
    let result = open_float_panel(app);
    crate::log_event!("[float] toggle done");
    result
}

/// Builds the float panel at the configured initial position on first
/// open. Closed windows are destroyed by the runtime, so a missing handle
/// simply means "build it again".
///
/// The window is built hidden: the float page shows and focuses itself
/// once its JS is mounted (see `revealFloatPanel` in
/// `src/routes/float/+page.svelte`). Showing or focusing from here races
/// WebView2 init and used to wedge the first paint as a stuck white window
/// whenever the main window was in front (the tray path only worked because
/// the main window happened to be hidden). Windows that already exist are
/// shown and focused immediately.
///
/// Must not run on the event-loop thread (e.g. directly inside a tray menu
/// callback): window creation dispatches to the event loop and blocks for
/// the response, which self-deadlocks there. Offload to a worker thread
/// from such call sites.
#[tauri::command]
pub fn open_float_panel<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) {
        window
            .show()
            .map_err(|error| format!("failed to show the float panel: {error}"))?;
        window
            .set_focus()
            .map_err(|error| format!("failed to focus the float panel: {error}"))?;
        return Ok(());
    }
    let builder =
        WebviewWindowBuilder::new(&app, FLOAT_WINDOW_LABEL, WebviewUrl::App("/float".into()))
            .title("Float")
            .inner_size(FLOAT_WIDTH, FLOAT_HEIGHT)
            .min_inner_size(260.0, 320.0)
            .resizable(true)
            .decorations(false)
            .visible(false)
            .always_on_top(true);
    // `WebviewWindowBuilder::transparent` is unavailable on macOS without the
    // `macos-private-api` feature, which we deliberately do not enable. The
    // float page paints its own themed background, so the panel simply stays
    // opaque there.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    crate::log_event!("[float] building window");
    let window = builder
        .build()
        .map_err(|error| format!("failed to open the float panel: {error}"))?;
    // Position after creation instead of with the builder: an undecorated
    // window carries an invisible shadow inset (see tao's
    // `calculate_insets_for_dpi`), so the requested 320x480 inner size is
    // smaller than the real outer size. The builder's `position` is the outer
    // top-left, so centering on the inner size overflowed the work area and
    // pushed the panel under the taskbar / off-screen. The real outer size
    // keeps the whole window inside the work area.
    position_float_panel(&app, &window);
    crate::log_event!("[float] window built");
    Ok(())
}

/// Places the freshly built, still-hidden float panel at the configured
/// work-area corner. Coordinates are computed from the window's actual outer
/// size against the primary monitor's physical work area, so the native
/// undecorated shadow inset stays inside the work area. Falls back to
/// centering when the position cannot be resolved.
fn position_float_panel<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
) {
    let outer = match window.outer_size() {
        Ok(size) => size,
        Err(error) => {
            crate::log_event!("[float] failed to read panel size ({error}); centering");
            let _ = window.center();
            return;
        }
    };
    let place = {
        let Some(config) = app.try_state::<Mutex<ConfigStore>>() else {
            let _ = window.center();
            return;
        };
        let place = match config.lock() {
            Ok(guard) => guard.general_settings().float_panel_position.clone(),
            Err(error) => {
                crate::log_event!(
                    "[float] configuration lock poisoned while reading panel position ({error}); centering"
                );
                let _ = window.center();
                return;
            }
        };
        place
    };
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        crate::log_event!("[float] primary monitor unavailable; centering");
        let _ = window.center();
        return;
    };
    let area = monitor.work_area();
    crate::log_event!(
        "[float] monitor scale={} work_area=({},{} {}x{})",
        monitor.scale_factor(),
        area.position.x,
        area.position.y,
        area.size.width,
        area.size.height
    );
    let (x, y) = corner_position(
        place.as_str(),
        area.position.x,
        area.position.y,
        area.size.width as i32,
        area.size.height as i32,
        outer.width as i32,
        outer.height as i32,
    );
    match window.set_position(tauri::PhysicalPosition::new(x, y)) {
        Ok(()) => crate::log_event!("[float] place={place} at ({x},{y})"),
        Err(error) => {
            crate::log_event!("[float] failed to place panel at ({x},{y}) for {place}: {error}");
            let _ = window.center();
        }
    }
}

/// Maps a configured position name onto a work-area origin, keeping the whole
/// `win_w` x `win_h` window inside it. Coordinates are physical pixels.
/// Unknown names fall back to bottom-right (the default), never to an error.
fn corner_position(
    place: &str,
    origin_x: i32,
    origin_y: i32,
    area_w: i32,
    area_h: i32,
    win_w: i32,
    win_h: i32,
) -> (i32, i32) {
    match place {
        "topLeft" => (origin_x, origin_y),
        "topRight" => (origin_x + area_w - win_w, origin_y),
        "bottomLeft" => (origin_x, origin_y + area_h - win_h),
        "center" => (
            origin_x + (area_w - win_w) / 2,
            origin_y + (area_h - win_h) / 2,
        ),
        // "bottomRight" default; unknown values fall through here too.
        _ => (origin_x + area_w - win_w, origin_y + area_h - win_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_resolve_inside_the_work_area() {
        // 1920x1080 work area at origin, 320x480 window.
        assert_eq!(
            corner_position("topLeft", 0, 0, 1920, 1080, 320, 480),
            (0, 0)
        );
        assert_eq!(
            corner_position("topRight", 0, 0, 1920, 1080, 320, 480),
            (1600, 0)
        );
        assert_eq!(
            corner_position("bottomLeft", 0, 0, 1920, 1080, 320, 480),
            (0, 600)
        );
        assert_eq!(
            corner_position("bottomRight", 0, 0, 1920, 1080, 320, 480),
            (1600, 600)
        );
        assert_eq!(
            corner_position("center", 0, 0, 1920, 1080, 320, 480),
            (800, 300)
        );
    }

    #[test]
    fn unknown_positions_fall_back_to_bottom_right() {
        for unknown in ["", "corner", "BOTTOMRIGHT"] {
            assert_eq!(
                corner_position(unknown, 0, 0, 1920, 1080, 320, 480),
                (1600, 600)
            );
        }
    }

    #[test]
    fn outer_size_and_shifted_work_area_stay_inside() {
        // A taskbar on the left shifts the origin and shrinks the area; the
        // outer size (including the undecorated shadow inset) must still fit.
        assert_eq!(
            corner_position("bottomRight", 48, 0, 1872, 1040, 336, 496),
            (48 + 1872 - 336, 1040 - 496)
        );
        // A monitor left of the primary reports a negative origin.
        assert_eq!(
            corner_position("topLeft", -1920, 0, 1920, 1080, 320, 480),
            (-1920, 0)
        );
    }
}
