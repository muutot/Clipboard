use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Listener, Manager, Runtime,
};

use crate::config::ConfigStore;
use crate::platform::windows_hotkey::HotkeyManager;
use crate::privacy::PrivacyManager;
use crate::CaptureState;

pub struct SystemTray;

impl SystemTray {
    const SHOW_MENU_ID: &'static str = "tray-show";
    const SETTINGS_MENU_ID: &'static str = "tray-settings";
    const PAUSE_MENU_ID: &'static str = "tray-pause";
    const RESTART_MENU_ID: &'static str = "tray-restart";
    const QUIT_MENU_ID: &'static str = "tray-quit";

    pub fn create<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let show_item =
            MenuItem::with_id(app, Self::SHOW_MENU_ID, "显示主窗口", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray show item: {error}"))?;
        let settings_item =
            MenuItem::with_id(app, Self::SETTINGS_MENU_ID, "打开设置", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray settings item: {error}"))?;
        let is_paused = app
            .try_state::<CaptureState>()
            .is_some_and(|c| c.is_paused());
        let recording_enabled = !is_paused;
        let pause_item = CheckMenuItem::with_id(
            app,
            Self::PAUSE_MENU_ID,
            "剪切板记录",
            true,
            recording_enabled,
            None::<&str>,
        )
        .map_err(|error| format!("failed to create the tray pause item: {error}"))?;
        let pause_item = Arc::new(pause_item);
        let pause_item_for_menu = Arc::clone(&pause_item);
        let restart_item =
            MenuItem::with_id(app, Self::RESTART_MENU_ID, "重启应用", true, None::<&str>)
                .map_err(|error| format!("failed to create the tray restart item: {error}"))?;
        let quit_item = MenuItem::with_id(app, Self::QUIT_MENU_ID, "退出", true, None::<&str>)
            .map_err(|error| format!("failed to create the tray quit item: {error}"))?;
        let menu = Menu::with_items(
            app,
            &[
                &show_item,
                &settings_item,
                pause_item.as_ref(),
                &restart_item,
                &quit_item,
            ],
        )
        .map_err(|error| format!("failed to create the tray menu: {error}"))?;

        let mut builder = TrayIconBuilder::with_id("main-tray")
            .menu(&menu)
            .tooltip("Clipboard")
            .show_menu_on_left_click(false)
            .on_menu_event(move |app, event| {
                if event.id() == Self::SHOW_MENU_ID {
                    show_main_window(app);
                } else if event.id() == Self::SETTINGS_MENU_ID {
                    show_main_window(app);
                    let _ = app.emit("tray-open-settings", ());
                } else if event.id() == Self::PAUSE_MENU_ID {
                    let paused = app
                        .try_state::<CaptureState>()
                        .map(|c| {
                            let new_paused = !c.is_paused();
                            c.set_paused(new_paused);
                            new_paused
                        })
                        .unwrap_or(false);
                    let _ = pause_item_for_menu.set_checked(!paused);
                    if let Ok(mut config) = app.state::<Mutex<ConfigStore>>().lock() {
                        let _ = config.set_privacy_paused(paused);
                    }
                    if let Ok(mut privacy) = app.state::<Mutex<PrivacyManager>>().lock() {
                        privacy.toggle_pause();
                    }
                    let _ = app.emit("privacy-pause-changed", paused);
                } else if event.id() == Self::QUIT_MENU_ID {
                    app.exit(0);
                } else if event.id() == Self::RESTART_MENU_ID {
                    app.restart();
                }
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

        let pause_item_for_listener = Arc::clone(&pause_item);
        app.listen("privacy-pause-changed", move |event| {
            let paused = serde_json::from_str::<bool>(event.payload()).unwrap_or(false);
            let _ = pause_item_for_listener.set_checked(!paused);
        });

        builder
            .build(app)
            .map_err(|error| format!("failed to create the system tray icon: {error}"))?;

        Ok(Self)
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("[tray] main window is unavailable");
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
        eprintln!("[tray] failed to show the main window: {error}");
    }
    if let Err(error) = window.unminimize() {
        eprintln!("[tray] failed to restore the main window: {error}");
    }
    if let Err(error) = window.set_focus() {
        eprintln!("[tray] failed to focus the main window: {error}");
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
/// Win32 layered-window alpha channel. Full opacity removes the layered
/// style again so the compositor fast path stays active.
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
        if percent >= 100 {
            SetWindowLongW(window_handle, GWL_EXSTYLE, ex_style & !WS_EX_LAYERED);
            return Ok(());
        }
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
