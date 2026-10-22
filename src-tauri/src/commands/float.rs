//! Standalone float-panel window, owned by the backend so it never depends
//! on the main window's frontend state. The tray menu, the global shortcut,
//! and the in-app shortcut all funnel through [`toggle_float_panel`].

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::config::ConfigStore;

pub const FLOAT_WINDOW_LABEL: &str = "float";
const FLOAT_WIDTH: f64 = 320.0;
const FLOAT_HEIGHT: f64 = 480.0;
/// How long a hidden float panel may stay idle before its WebView is
/// destroyed, releasing the renderer process and its JS heap. Every show
/// path rebuilds on demand, so destruction is transparent apart from a
/// slightly slower first paint.
const FLOAT_IDLE_DESTROY_AFTER: Duration = Duration::from_secs(10 * 60);
/// Reaper polling interval; destruction lands within one tick after the
/// idle threshold.
const FLOAT_IDLE_POLL: Duration = Duration::from_secs(60);

/// Shows or hides the float panel, mirroring the main-window toggle: a
/// visible and focused panel is hidden, otherwise it is shown (built on
/// first use) and focused. Only the float panel's own visibility changes;
/// the main window is never touched. Every entry point (tray, global
/// hotkey, in-app shortcut) funnels through this single toggle so
/// behavior is uniform.
#[tauri::command]
pub fn toggle_float_panel<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    crate::log_debug!("[float] toggle requested");
    if let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);
        crate::log_debug!("[float] existing visible: {is_visible}, focused: {is_focused}");
        if is_visible && is_focused {
            window
                .hide()
                .map_err(|error| format!("failed to hide the float panel: {error}"))?;
            crate::log_debug!("[float] toggle done");
            return Ok(());
        }
    }
    let result = open_float_panel(app);
    crate::log_debug!("[float] toggle done");
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
    // The float page paints its own themed background, so this panel remains
    // opaque on macOS even though main-window effects enable macos-private-api.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    crate::log_debug!("[float] building window");
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
    crate::log_debug!("[float] window built");
    Ok(())
}

/// Places the freshly built, still-hidden float panel at the configured
/// work-area corner. Coordinates are computed from the window's actual outer
/// size against the primary monitor's physical work area, so the native
/// undecorated shadow inset stays inside the work area. Falls back to
/// centering when the position cannot be resolved.
///
/// When the idle reaper has destroyed a previous panel, the captured outer
/// geometry is restored instead of the configured corner, so an idle rebuild
/// does not jump the panel back across the screen. The restored position is
/// still clamped into the work area in case the monitor setup changed.
fn position_float_panel<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
) {
    let outer = match window.outer_size() {
        Ok(size) => size,
        Err(error) => {
            crate::log_error!("[float] failed to read panel size ({error}); centering");
            let _ = window.center();
            return;
        }
    };
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        crate::log_warn!("[float] primary monitor unavailable; centering");
        let _ = window.center();
        return;
    };
    let area = monitor.work_area();
    crate::log_debug!(
        "[float] monitor scale={} work_area=({},{} {}x{})",
        monitor.scale_factor(),
        area.position.x,
        area.position.y,
        area.size.width,
        area.size.height
    );
    if let Some(geometry) = *float_geometry_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
    {
        restore_geometry(window, geometry, area, outer);
        return;
    }
    let place = {
        let Some(config) = app.try_state::<Mutex<ConfigStore>>() else {
            let _ = window.center();
            return;
        };
        let place = match config.lock() {
            Ok(guard) => guard.general_settings().float_panel_position.clone(),
            Err(error) => {
                crate::log_error!(
                    "[float] configuration lock poisoned while reading panel position ({error}); centering"
                );
                let _ = window.center();
                return;
            }
        };
        place
    };
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
        Ok(()) => crate::log_debug!("[float] place={place} at ({x},{y})"),
        Err(error) => {
            crate::log_error!("[float] failed to place panel at ({x},{y}) for {place}: {error}");
            let _ = window.center();
        }
    }
}

/// Restores a previously captured panel placement: sizes the window so its
/// outer frame matches the capture (the builder's fresh window carries the
/// same shadow inset, which is subtracted from the outer size), then places
/// the outer top-left, clamped into the work area.
fn restore_geometry<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    geometry: FloatGeometry,
    area: &tauri::PhysicalRect<i32, u32>,
    fresh_outer: tauri::PhysicalSize<u32>,
) {
    // The fresh window carries the same undecorated shadow inset as the
    // captured one; subtract it so `set_size` (inner size) reproduces the
    // captured outer size. A missing read degrades to no inset correction.
    let (inset_w, inset_h) = match window.inner_size() {
        Ok(inner) => (
            fresh_outer.width.saturating_sub(inner.width),
            fresh_outer.height.saturating_sub(inner.height),
        ),
        Err(_) => (0, 0),
    };
    let inner = PhysicalSize::new(
        geometry.width.saturating_sub(inset_w),
        geometry.height.saturating_sub(inset_h),
    );
    if let Err(error) = window.set_size(inner) {
        crate::log_warn!("[float] failed to restore panel size: {error}");
    }
    let (x, y) = {
        let work = WorkArea::from_rect(area);
        work.clamp_point(
            geometry.x,
            geometry.y,
            geometry.width as i32,
            geometry.height as i32,
        )
    };
    if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
        crate::log_warn!("[float] failed to restore panel position ({x},{y}): {error}");
    } else {
        crate::log_debug!("[float] restored idle-captured placement at ({x},{y})");
    }
}

/// Physical work-area rectangle used to keep windows on screen.
#[derive(Clone, Copy)]
struct WorkArea {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl WorkArea {
    fn from_rect(rect: &tauri::PhysicalRect<i32, u32>) -> Self {
        Self {
            x: rect.position.x,
            y: rect.position.y,
            width: rect.size.width as i32,
            height: rect.size.height as i32,
        }
    }

    /// Clamps a window's outer top-left so the whole window stays inside the
    /// work area. A window larger than the area is pinned to the area origin.
    fn clamp_point(&self, x: i32, y: i32, win_w: i32, win_h: i32) -> (i32, i32) {
        let max_x = (self.x + self.width - win_w).max(self.x);
        let max_y = (self.y + self.height - win_h).max(self.y);
        (x.clamp(self.x, max_x), y.clamp(self.y, max_y))
    }
}

/// Outer position and size of the float panel at destroy time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FloatGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn float_geometry_slot() -> &'static Mutex<Option<FloatGeometry>> {
    static SLOT: OnceLock<Mutex<Option<FloatGeometry>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Tracks how long the float panel has been continuously hidden. Visibility
/// resets the clock; each hidden observation returns whether the idle
/// threshold has been reached.
struct FloatIdleTracker {
    hidden_since: Option<Instant>,
}

impl FloatIdleTracker {
    fn new() -> Self {
        Self { hidden_since: None }
    }

    fn observe(&mut self, visible: bool, now: Instant) -> bool {
        if visible {
            self.hidden_since = None;
            return false;
        }
        let since = *self.hidden_since.get_or_insert(now);
        now.duration_since(since) >= FLOAT_IDLE_DESTROY_AFTER
    }
}

/// Spawns the float-panel idle reaper: once per minute it observes the panel
/// and destroys it after it has been hidden for [`FLOAT_IDLE_DESTROY_AFTER`],
/// releasing the second WebView's renderer process. The outer geometry is
/// captured first so the next on-demand rebuild restores the placement
/// instead of the configured corner. All hide paths leave the window in
/// place untouched — this loop never runs on the event-loop thread.
pub fn start_float_idle_reaper<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let _ = std::thread::Builder::new()
        .name("float-idle-reaper".into())
        .spawn(move || {
            let mut tracker = FloatIdleTracker::new();
            loop {
                std::thread::sleep(FLOAT_IDLE_POLL);
                let Some(window) = app.get_webview_window(FLOAT_WINDOW_LABEL) else {
                    tracker = FloatIdleTracker::new();
                    continue;
                };
                if !tracker.observe(window.is_visible().unwrap_or(false), Instant::now()) {
                    continue;
                }
                let geometry = match (window.outer_position(), window.outer_size()) {
                    (Ok(position), Ok(size)) => Some(FloatGeometry {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    }),
                    (error, size_error) => {
                        crate::log_warn!(
                            "[float] failed to capture idle geometry ({error:?} {size_error:?})"
                        );
                        None
                    }
                };
                if let Some(geometry) = geometry {
                    if let Ok(mut slot) = float_geometry_slot().lock() {
                        *slot = Some(geometry);
                    }
                }
                crate::log_debug!("[float] idle threshold reached; destroying panel");
                if window.close().is_err() {
                    crate::log_warn!("[float] idle destruction failed; retrying next tick");
                    continue;
                }
                tracker = FloatIdleTracker::new();
            }
        });
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
    fn clamp_keeps_window_inside_the_work_area() {
        let inside = |x: i32, y: i32, area: WorkArea, w: i32, h: i32| area.clamp_point(x, y, w, h);
        // Already inside: untouched.
        assert_eq!(
            inside(
                100,
                100,
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                320,
                480
            ),
            (100, 100)
        );
        // Right/bottom overflow pulls back to the area edge.
        assert_eq!(
            inside(
                1900,
                900,
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                320,
                480
            ),
            (1600, 600)
        );
        // Negative coordinates pin to the origin.
        assert_eq!(
            inside(
                -50,
                -50,
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                320,
                480
            ),
            (0, 0)
        );
        // A shifted work area (taskbar left, monitor above) keeps the window inside.
        let shifted = WorkArea {
            x: 48,
            y: 100,
            width: 1872,
            height: 980,
        };
        assert_eq!(inside(40, -100, shifted, 336, 496), (48, 100));
        assert_eq!(inside(1600, 600, shifted, 336, 496), (1584, 584));
        // A window larger than the area is pinned to the area origin.
        assert_eq!(
            inside(
                10,
                10,
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100
                },
                320,
                480
            ),
            (0, 0)
        );
    }

    #[test]
    fn idle_tracker_destroys_only_after_continuous_hidden_time() {
        let start = Instant::now();
        let mut tracker = FloatIdleTracker::new();
        // Hidden observations accumulate toward the threshold.
        assert!(!tracker.observe(false, start));
        assert!(!tracker.observe(
            false,
            start + FLOAT_IDLE_DESTROY_AFTER - Duration::from_secs(1)
        ));
        assert!(tracker.observe(false, start + FLOAT_IDLE_DESTROY_AFTER));
        // Becoming visible resets the clock; the next hidden spell must run a
        // full threshold from its own first hidden observation.
        let resume = start + FLOAT_IDLE_DESTROY_AFTER;
        assert!(!tracker.observe(true, resume));
        assert!(!tracker.observe(false, resume));
        assert!(!tracker.observe(
            false,
            resume + FLOAT_IDLE_DESTROY_AFTER - Duration::from_secs(1)
        ));
        assert!(tracker.observe(false, resume + FLOAT_IDLE_DESTROY_AFTER));
        // A fresh tracker has nothing to reap yet.
        let mut fresh = FloatIdleTracker::new();
        assert!(!fresh.observe(false, start));
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
