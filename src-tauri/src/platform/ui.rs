use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Listener, Manager, Runtime,
};

use crate::config::ConfigStore;
use crate::domain::ClipboardKind;
use crate::platform::windows_hotkey::HotkeyManager;
use crate::privacy::PrivacyManager;
use crate::storage::{ClipboardRepository, Database, HistoryFilter};
use crate::CaptureState;
use crate::SelfTriggerState;

pub struct SystemTray;

impl SystemTray {
    const SHOW_MENU_ID: &'static str = "tray-show";
    const FLOAT_MENU_ID: &'static str = "tray-float";
    const SETTINGS_MENU_ID: &'static str = "tray-settings";
    const PAUSE_MENU_ID: &'static str = "tray-pause";
    const RESTART_MENU_ID: &'static str = "tray-restart";
    const QUIT_MENU_ID: &'static str = "tray-quit";
    const RECENT_SUBMENU_ID: &'static str = "tray-recent";
    const RECENT_EMPTY_ID: &'static str = "tray-recent-empty";

    pub fn create<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let menu = Self::build_menu(app, &[])?;
        let mut builder = TrayIconBuilder::with_id("main-tray")
            .menu(&menu)
            .tooltip("Clipboard")
            .show_menu_on_left_click(false)
            .on_menu_event(move |app, event| {
                Self::handle_menu_event(app, event.id().as_ref());
            })
            .on_tray_icon_event(|tray, event| {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    show_main_window(tray.app_handle());
                }
            });

        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }

        // The menu is rebuilt from state on every refresh, so this listener
        // only triggers a rebuild instead of holding a menu-item handle that
        // would go stale after the first refresh.
        let app_for_refresh = app.clone();
        app.listen("privacy-pause-changed", move |_event| {
            refresh_tray_recent_menu(&app_for_refresh);
        });

        builder
            .build(app)
            .map_err(|error| format!("failed to create the system tray icon: {error}"))?;

        Ok(Self)
    }

    /// Builds the full tray menu. The pause checkbox always reflects the live
    /// capture state because callers rebuild (rather than mutate) the menu.
    fn build_menu<R: Runtime>(
        app: &AppHandle<R>,
        recent: &[(String, String)],
    ) -> Result<Menu<R>, String> {
        let show_item =
            MenuItem::with_id(app, Self::SHOW_MENU_ID, "显示主窗口", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray show item: {error}"))?;
        let settings_item =
            MenuItem::with_id(app, Self::SETTINGS_MENU_ID, "打开设置", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray settings item: {error}"))?;
        let float_item =
            MenuItem::with_id(app, Self::FLOAT_MENU_ID, "悬浮窗口", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray float item: {error}"))?;
        let recording_enabled = !app
            .try_state::<CaptureState>()
            .is_some_and(|c| c.is_paused());
        let pause_item = CheckMenuItem::with_id(
            app,
            Self::PAUSE_MENU_ID,
            "剪切板记录",
            true,
            recording_enabled,
            None::<&str>,
        )
        .map_err(|error| format!("failed to create the tray pause item: {error}"))?;
        let restart_item =
            MenuItem::with_id(app, Self::RESTART_MENU_ID, "重启应用", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray restart item: {error}"))?;
        let quit_item = MenuItem::with_id(app, Self::QUIT_MENU_ID, "退出", true, None::<&str>)
            .map_err(|error| format!("failed to create the tray quit item: {error}"))?;
        let recent_submenu = Self::build_recent_submenu(app, recent)?;
        Menu::with_items(
            app,
            &[
                &show_item,
                &float_item,
                &settings_item,
                &pause_item,
                &recent_submenu,
                &restart_item,
                &quit_item,
            ],
        )
        .map_err(|error| format!("failed to create the tray menu: {error}"))
    }

    /// "最近复制" submenu: up to [`TRAY_RECENT_COUNT`] text/link entries, or
    /// a single disabled placeholder when history is empty.
    fn build_recent_submenu<R: Runtime>(
        app: &AppHandle<R>,
        recent: &[(String, String)],
    ) -> Result<Submenu<R>, String> {
        let submenu = Submenu::with_id(app, Self::RECENT_SUBMENU_ID, "最近复制", true)
            .map_err(|error| format!("failed to create the tray recent submenu: {error}"))?;
        if recent.is_empty() {
            let placeholder = MenuItem::with_id(
                app,
                Self::RECENT_EMPTY_ID,
                "暂无可复制的文本记录",
                false,
                None::<&str>,
            )
            .map_err(|error| format!("failed to create the tray placeholder item: {error}"))?;
            submenu
                .append(&placeholder)
                .map_err(|error| format!("failed to append the tray placeholder: {error}"))?;
            return Ok(submenu);
        }
        for (item_id, title) in recent {
            let entry = MenuItem::with_id(app, recent_menu_id(item_id), title, true, None::<&str>)
                .map_err(|error| format!("failed to create a tray recent item: {error}"))?;
            submenu
                .append(&entry)
                .map_err(|error| format!("failed to append a tray recent item: {error}"))?;
        }
        Ok(submenu)
    }

    fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
        if id == Self::SHOW_MENU_ID {
            show_main_window(app);
        } else if id == Self::FLOAT_MENU_ID {
            // Same float-only toggle as every other entry point (the main
            // window is left alone); never depends on the
            // main window's frontend state. Window creation dispatches to
            // the event loop and blocks for the response, while this menu
            // callback itself runs on the event loop thread — building
            // inline would self-deadlock. Offload to a worker thread.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(error) = crate::commands::float::toggle_float_panel(app) {
                    crate::log_event!("[tray] failed to toggle the float panel: {error}");
                }
            });
        } else if id == Self::SETTINGS_MENU_ID {
            show_main_window(app);
            let _ = app.emit("tray-open-settings", ());
        } else if id == Self::PAUSE_MENU_ID {
            let paused = app
                .try_state::<CaptureState>()
                .map(|c| {
                    let new_paused = !c.is_paused();
                    c.set_paused(new_paused);
                    new_paused
                })
                .unwrap_or(false);
            if let Ok(mut config) = app.state::<Mutex<ConfigStore>>().lock() {
                let _ = config.set_privacy_paused(paused);
            }
            if let Ok(mut privacy) = app.state::<Mutex<PrivacyManager>>().lock() {
                privacy.toggle_pause();
            }
            let _ = app.emit("privacy-pause-changed", paused);
            // Rebuild so the checkbox reflects the toggled state; the
            // privacy listener rebuilds again on the event above, which is
            // idempotent.
            refresh_tray_recent_menu(app);
        } else if id == Self::QUIT_MENU_ID {
            app.exit(0);
        } else if id == Self::RESTART_MENU_ID {
            app.restart();
        } else if let Some(item_id) = recent_item_id(id) {
            tray_copy_item(app, item_id);
        }
    }
}

/// Maximum recent entries shown in the tray submenu.
const TRAY_RECENT_COUNT: usize = 10;
/// Oversampled on read so kind/text filtering still leaves a full submenu.
const TRAY_RECENT_FETCH: u32 = 20;
/// Menu-item id prefix for recent entries; the history id follows it.
const TRAY_RECENT_PREFIX: &str = "tray-recent:";
/// Title width in characters; longer text is cut at a char boundary.
const TRAY_TITLE_CHARS: usize = 40;

pub fn recent_menu_id(item_id: &str) -> String {
    format!("{TRAY_RECENT_PREFIX}{item_id}")
}

pub fn recent_item_id(menu_id: &str) -> Option<&str> {
    menu_id.strip_prefix(TRAY_RECENT_PREFIX)
}

/// Collapses a clipboard text to a single-line menu title, cut at a
/// character boundary so multi-byte text is never split mid-character.
pub fn tray_recent_title(text: &str) -> String {
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= TRAY_TITLE_CHARS {
        return single;
    }
    let mut title: String = single.chars().take(TRAY_TITLE_CHARS - 1).collect();
    title.push('…');
    title
}

/// Rebuilds the tray menu from the current capture state and recent history.
/// Best-effort: any failure keeps the previous menu and logs the cause.
/// Called after tray creation, after each captured save, and on pause flips.
pub fn refresh_tray_recent_menu<R: Runtime>(app: &AppHandle<R>) {
    let entries = load_recent_entries(app);
    let app_for_task = app.clone();
    // Build and install the menu on the main thread without waiting. Calling
    // `tray.set_menu` directly from a background thread blocks until the main
    // thread runs the queued menu task, but the main thread joins the capture
    // worker during shutdown, so the two would deadlock. `run_on_main_thread`
    // only posts the task and returns.
    if let Err(error) =
        app.run_on_main_thread(
            move || match SystemTray::build_menu(&app_for_task, &entries) {
                Ok(menu) => {
                    if let Some(tray) = app_for_task.tray_by_id("main-tray") {
                        if let Err(error) = tray.set_menu(Some(menu)) {
                            crate::log_event!("[tray] failed to refresh recent menu: {error}");
                        }
                    }
                }
                Err(error) => crate::log_event!("[tray] failed to rebuild menu: {error}"),
            },
        )
    {
        crate::log_event!("[tray] failed to schedule menu refresh: {error}");
    }
}

/// Latest text/link records with copyable text, newest first.
fn load_recent_entries<R: Runtime>(app: &AppHandle<R>) -> Vec<(String, String)> {
    let Some(database) = app.try_state::<Database>() else {
        return Vec::new();
    };
    let filter = HistoryFilter {
        kind: None,
        favorite_only: false,
        tag: None,
        source_app: None,
        date_from_ms: None,
        date_to_ms: None,
    };
    // Page through history until the submenu is full or records run out:
    // a fixed oversample window showed "no recent text" whenever the newest
    // records happened to be all images/files.
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut offset: u32 = 0;
    while entries.len() < TRAY_RECENT_COUNT {
        let Ok(items) = database.list_recent(TRAY_RECENT_FETCH, offset, &filter) else {
            break;
        };
        let page_len = items.len();
        if page_len == 0 {
            break;
        }
        for item in items {
            if entries.len() >= TRAY_RECENT_COUNT {
                break;
            }
            if !matches!(item.kind, ClipboardKind::Text | ClipboardKind::Link) {
                continue;
            }
            let text = item.text_content.as_deref().unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            entries.push((item.id.clone(), tray_recent_title(text)));
        }
        if page_len < TRAY_RECENT_FETCH as usize {
            break;
        }
        offset += page_len as u32;
    }
    entries
}

/// Copies a history entry back to the system clipboard through the same
/// self-trigger-marked path as the main UI, then records the reuse for
/// `last_used_at_ms` ordering. Text/link only; images and files stay in the
/// main window where materialization is available.
fn tray_copy_item<R: Runtime>(app: &AppHandle<R>, item_id: &str) {
    let result: Result<(), String> = (|| {
        let database = app.try_state::<Database>().ok_or("database unavailable")?;
        let item = database
            .get_item(item_id)
            .map_err(|error| error.to_string())?
            .ok_or("history item no longer exists")?;
        if !matches!(item.kind, ClipboardKind::Text | ClipboardKind::Link) {
            return Err("only text records can be copied from the tray".to_owned());
        }
        let text = item.text_content.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            return Err("history item has no copyable text".to_owned());
        }
        if let Some(guard) = app.try_state::<SelfTriggerState>() {
            if let Ok(mut guard) = guard.0.lock() {
                guard.mark_clipboard_write(text);
            }
        }
        crate::platform::platform()
            .write_clipboard_text_with_self_trigger(text)
            .map_err(|error| format!("failed to write clipboard: {error}"))?;
        let _ = database.set_last_used(item_id);
        Ok(())
    })();
    if let Err(error) = result {
        crate::log_event!("[tray] failed to copy history item: {error}");
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        crate::log_event!("[tray] main window is unavailable");
        return;
    };

    let is_visible = window.is_visible().unwrap_or(false);
    if !is_visible {
        // Remember which window was active before the clipboard is brought up,
        // so "paste to previous application" works when opened from the tray
        // (the toggle hotkey records the target inside its own loop).
        if let Some(hm) = app.try_state::<Mutex<HotkeyManager>>() {
            if let Ok(hm) = hm.lock() {
                hm.remember_foreground();
            }
        }
    }

    if let Err(error) = window.show() {
        crate::log_event!("[tray] failed to show the main window: {error}");
    }
    if let Err(error) = window.unminimize() {
        crate::log_event!("[tray] failed to restore the main window: {error}");
    }
    if let Err(error) = window.set_focus() {
        crate::log_event!("[tray] failed to focus the main window: {error}");
    }
}

pub struct WindowManager;

impl WindowManager {
    pub fn save_position(
        config: &mut ConfigStore,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        config
            .set_window_position(x, y, width, height)
            .map_err(|e| e.to_string())
    }

    pub fn restore_position(config: &ConfigStore) -> Option<(i32, i32, u32, u32)> {
        config.window_position()
    }
}

/// Maps a transparency percentage (60-100) to the Win32 layered-window alpha
/// byte. Values outside the settings range are clamped first.
pub fn window_transparency_alpha(percent: u8) -> u8 {
    let percent = percent.clamp(60, 100);
    ((u32::from(percent) * 255) / 100) as u8
}

/// Applies the configured window transparency to a native window using the
/// Win32 layered-window alpha channel. A value of 100 maps to alpha byte 255
/// while the layered style is retained so per-pixel translucent pixels keep
/// compositing over the desktop (the application window is `transparent`).
#[cfg(target_os = "windows")]
pub fn apply_window_transparency(window_handle: isize, percent: u8) -> Result<(), String> {
    extern "system" {
        fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
        fn SetWindowLongW(hwnd: isize, index: i32, new_long: i32) -> i32;
        fn SetLayeredWindowAttributes(hwnd: isize, color_key: u32, alpha: u8, flags: u32) -> i32;
    }

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_LAYERED: i32 = 0x0008_0000;
    const LWA_ALPHA: u32 = 0x0000_0002;

    if window_handle == 0 {
        return Err("window handle is unavailable".to_owned());
    }

    let percent = percent.clamp(60, 100);
    unsafe {
        let ex_style = GetWindowLongW(window_handle, GWL_EXSTYLE);
        SetWindowLongW(window_handle, GWL_EXSTYLE, ex_style | WS_EX_LAYERED);
        if SetLayeredWindowAttributes(
            window_handle,
            0,
            window_transparency_alpha(percent),
            LWA_ALPHA,
        ) == 0
        {
            return Err("SetLayeredWindowAttributes failed".to_owned());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn apply_window_transparency(_window_handle: isize, _percent: u8) -> Result<(), String> {
    Err("window transparency is not supported on this platform".to_owned())
}

/// Applies a frosted glass effect to the window. Supported effects are
/// `"acrylic"` (Windows 10+) and `"mica"` (Windows 11); any other value
/// clears the active effect. Other platforms are a no-op.
#[cfg(target_os = "windows")]
pub fn apply_window_effect<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    effect: &str,
) -> Result<(), String> {
    match effect {
        "acrylic" => window_vibrancy::apply_acrylic(window, Some((18, 18, 18, 125)))
            .map_err(|error| error.to_string()),
        "mica" => {
            window_vibrancy::apply_mica(window, Some(true)).map_err(|error| error.to_string())
        }
        _ => {
            let _ = window_vibrancy::clear_acrylic(window);
            let _ = window_vibrancy::clear_mica(window);
            Ok(())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn apply_window_effect<R: Runtime>(
    _window: &tauri::WebviewWindow<R>,
    _effect: &str,
) -> Result<(), String> {
    Ok(())
}

/// Capacity information for the volume that stores a given directory.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Queries the total and caller-available capacity of the volume holding
/// `path`. Returns `None` when the platform query is unavailable or fails.
#[cfg(target_os = "windows")]
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory_name: *const u16,
            free_bytes_available: *mut u64,
            total_number_of_bytes: *mut u64,
            total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    let succeeded =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if succeeded == 0 {
        return None;
    }
    Some(DiskSpace {
        total_bytes: total,
        available_bytes: available,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn disk_space(_path: &Path) -> Option<DiskSpace> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_menu_ids_round_trip_item_ids() {
        let menu_id = recent_menu_id("abc-123");
        assert!(menu_id.starts_with(TRAY_RECENT_PREFIX));
        assert_eq!(recent_item_id(&menu_id), Some("abc-123"));
        assert_eq!(recent_item_id("tray-show"), None);
        assert_eq!(recent_item_id("tray-recent:"), Some(""));
    }

    #[test]
    fn recent_titles_collapse_whitespace_and_respect_char_boundaries() {
        assert_eq!(tray_recent_title("hello world"), "hello world");
        assert_eq!(tray_recent_title("  a\nb\tc  "), "a b c");
        let long = "x".repeat(100);
        let titled = tray_recent_title(&long);
        assert_eq!(titled.chars().count(), TRAY_TITLE_CHARS);
        assert!(titled.ends_with('…'));
        let cjk = "汉".repeat(100);
        let titled_cjk = tray_recent_title(&cjk);
        assert_eq!(titled_cjk.chars().count(), TRAY_TITLE_CHARS);
    }
}
