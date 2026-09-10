//! Standalone float-panel window, owned by the backend so it never depends
//! on the main window's frontend state. The tray menu, the global shortcut,
//! and the main toolbar all funnel through [`toggle_float_panel`].

use std::sync::Mutex;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::config::ConfigStore;

pub const FLOAT_WINDOW_LABEL: &str = "float";
const FLOAT_WIDTH: f64 = 320.0;
const FLOAT_HEIGHT: f64 = 480.0;

/// Toggles between the main window and the float panel: when the float
/// panel is visible it is hidden and the main window is shown; otherwise
/// the float panel is opened (focused when already open) and the main
/// window is hidden. Every entry point (toolbar, tray, global hotkey,
/// in-app shortcut) funnels through this single swap so behavior is uniform.
#[tauri::command]
pub fn toggle_float_panel<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    crate::log_event!("[float] toggle requested");
    let float_visible = app
        .get_webview_window(FLOAT_WINDOW_LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    crate::log_event!("[float] existing visible: {float_visible}");
    if float_visible {
        if let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) {
            window
                .hide()
                .map_err(|error| format!("failed to hide the float panel: {error}"))?;
        }
        if let Some(window) = app.get_webview_window("main") {
            window
                .show()
                .map_err(|error| format!("failed to show the main window: {error}"))?;
            window
                .set_focus()
                .map_err(|error| format!("failed to focus the main window: {error}"))?;
        }
        return Ok(());
    }
    // Hide the main window BEFORE creating the float window so creation
    // happens under the same conditions as the proven tray path (no
    // visibility/focus overlap while the new webview paints). On failure the
    // main window is restored so the user is never left with nothing.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    if let Err(error) = open_float_panel(app.clone()) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
        }
        return Err(error);
    }
    if let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) {
        let _ = window.set_focus();
    }
    crate::log_event!("[float] toggle done");
    Ok(())
}

/// Focuses the float panel, creating it at the configured initial position
/// on first open. Closed windows are destroyed by the runtime, so a missing
/// handle simply means "build it again".
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
    let mut builder =
        WebviewWindowBuilder::new(&app, FLOAT_WINDOW_LABEL, WebviewUrl::App("/float".into()))
            .title("Float")
            .inner_size(FLOAT_WIDTH, FLOAT_HEIGHT)
            .min_inner_size(260.0, 320.0)
            .resizable(true)
            .decorations(false)
            .always_on_top(true);
    match float_initial_position(&app) {
        Some((x, y)) => {
            builder = builder.position(x, y);
        }
        None => {
            builder = builder.center();
        }
    }
    crate::log_event!("[float] building window");
    builder
        .build()
        .map_err(|error| format!("failed to open the float panel: {error}"))?;
    crate::log_event!("[float] window built");
    Ok(())
}

/// Initial position from `general.floatPanelPosition`, resolved against the
/// primary monitor's work area. Builder coordinates are logical pixels while
/// monitor geometry is physical, hence the scale conversion. `None` falls
/// back to centered.
fn float_initial_position<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<(f64, f64)> {
    let place = {
        let config = app.try_state::<Mutex<ConfigStore>>()?;
        let config = config.lock().ok()?;
        config.general_settings().float_panel_position.clone()
    };
    let monitor = app.primary_monitor().ok()??;
    let scale = monitor.scale_factor();
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let area = monitor.work_area();
    let origin_x = area.position.x as f64 / scale;
    let origin_y = area.position.y as f64 / scale;
    let area_w = area.size.width as f64 / scale;
    let area_h = area.size.height as f64 / scale;
    Some(corner_position(
        place.as_str(),
        origin_x,
        origin_y,
        area_w,
        area_h,
    ))
}

/// Maps a configured position name onto a work-area origin. Unknown names
/// fall back to bottom-right (the default), never to an error.
fn corner_position(
    place: &str,
    origin_x: f64,
    origin_y: f64,
    area_w: f64,
    area_h: f64,
) -> (f64, f64) {
    match place {
        "topLeft" => (origin_x, origin_y),
        "topRight" => (origin_x + area_w - FLOAT_WIDTH, origin_y),
        "bottomLeft" => (origin_x, origin_y + area_h - FLOAT_HEIGHT),
        "center" => (
            origin_x + (area_w - FLOAT_WIDTH) / 2.0,
            origin_y + (area_h - FLOAT_HEIGHT) / 2.0,
        ),
        // "bottomRight" default; unknown values fall through here too.
        _ => (
            origin_x + area_w - FLOAT_WIDTH,
            origin_y + area_h - FLOAT_HEIGHT,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_resolve_inside_the_work_area() {
        // 1920x1080 work area at origin, 320x480 panel.
        assert_eq!(
            corner_position("topLeft", 0.0, 0.0, 1920.0, 1080.0),
            (0.0, 0.0)
        );
        assert_eq!(
            corner_position("topRight", 0.0, 0.0, 1920.0, 1080.0),
            (1600.0, 0.0)
        );
        assert_eq!(
            corner_position("bottomLeft", 0.0, 0.0, 1920.0, 1080.0),
            (0.0, 600.0)
        );
        assert_eq!(
            corner_position("bottomRight", 0.0, 0.0, 1920.0, 1080.0),
            (1600.0, 600.0)
        );
        assert_eq!(
            corner_position("center", 0.0, 0.0, 1920.0, 1080.0),
            (800.0, 300.0)
        );
    }

    #[test]
    fn unknown_positions_fall_back_to_bottom_right() {
        for unknown in ["", "corner", "BOTTOMRIGHT"] {
            assert_eq!(
                corner_position(unknown, 0.0, 0.0, 1920.0, 1080.0),
                (1600.0, 600.0)
            );
        }
    }
}
