//! Main-window show/restore, saved window position, and window transparency.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime};

use crate::config::ConfigStore;
use crate::platform::windows::hotkey::HotkeyManager;

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        crate::log_warn!("[tray] main window is unavailable");
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
        crate::log_error!("[tray] failed to show the main window: {error}");
    }
    if let Err(error) = window.unminimize() {
        crate::log_error!("[tray] failed to restore the main window: {error}");
    }
    if let Err(error) = window.set_focus() {
        crate::log_error!("[tray] failed to focus the main window: {error}");
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

/// Turning whole-window opacity off must restore the native surface to 100%;
/// CSS then owns background-only translucency without dimming text twice.
pub fn native_opacity_percentage(percent: u8, affects_text: bool) -> u8 {
    if affects_text {
        percent.clamp(60, 100)
    } else {
        100
    }
}

/// Caller dispatches to the UI thread before touching native window objects.
pub fn apply_webview_transparency<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    percent: u8,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        apply_window_transparency(
            window.hwnd().map_err(|e| e.to_string())?.0 as isize,
            percent,
        )
    }
    #[cfg(target_os = "macos")]
    {
        use super::macos::objc;
        let native = window.ns_window().map_err(|e| e.to_string())?;
        if native.is_null() {
            return Err("native window unavailable".into());
        }
        unsafe {
            objc::msgSend_void_f64(
                native.cast(),
                objc::sel_registerName(c"setAlphaValue:".as_ptr()),
                f64::from(percent.clamp(60, 100)) / 100.0,
            );
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::WidgetExt;
        window
            .gtk_window()
            .map_err(|e| e.to_string())?
            .set_opacity(f64::from(percent.clamp(60, 100)) / 100.0);
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (window, percent);
        Err("window opacity unavailable".into())
    }
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
/// clears the active effect. Other platforms use their own adapters below.
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

#[cfg(target_os = "macos")]
pub fn apply_window_effect<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    effect: &str,
) -> Result<(), String> {
    use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};
    let material = match effect {
        "acrylic" => NSVisualEffectMaterial::Sidebar,
        "mica" => NSVisualEffectMaterial::UnderWindowBackground,
        _ => {
            return window_vibrancy::clear_vibrancy(window)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    };
    window_vibrancy::apply_vibrancy(window, material, Some(NSVisualEffectState::Active), None)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
pub fn apply_window_effect<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    effect: &str,
) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let enabled = matches!(effect, "acrylic" | "mica");
    if super::Platform::detect().is_wayland() {
        return if enabled {
            Err("Wayland backdrop blur is controlled by the compositor; window opacity remains available".into())
        } else {
            Ok(())
        };
    }
    let id = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).map_err(|e| e.to_string())?,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => return Err("X11 window handle unavailable".into()),
    };
    crate::platform::linux::x11::effect::set_blur(id, enabled)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn apply_window_effect<R: Runtime>(
    _: &tauri::WebviewWindow<R>,
    effect: &str,
) -> Result<(), String> {
    if effect == "off" {
        Ok(())
    } else {
        Err("window effects unavailable".into())
    }
}

#[cfg(test)]
mod opacity_tests {
    #[test]
    fn switching_back_to_background_only_restores_native_alpha() {
        assert_eq!(super::native_opacity_percentage(75, true), 75);
        assert_eq!(super::native_opacity_percentage(75, false), 100);
        assert_eq!(super::native_opacity_percentage(0, true), 60);
        assert_eq!(super::native_opacity_percentage(255, true), 100);
    }
}
