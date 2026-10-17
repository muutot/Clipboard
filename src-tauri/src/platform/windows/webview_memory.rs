//! Suspends the main WebView2 while its window is hidden.
//!
//! WebView2 keeps the renderer and browser processes alive when a window is
//! merely hidden. `TrySuspend` releases the active page resources without
//! destroying DOM or JavaScript state. The callback is asynchronous, so a
//! show request can arrive while suspension is still finishing; in that case
//! the completion callback immediately resumes the webview.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
use webview2_com::TrySuspendCompletedHandler;
use windows_core::{Interface, Result as ComResult, BOOL};

static MAIN_WINDOW_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Marks the hide request before the native window visibility change is
/// dispatched so a concurrent show cannot be followed by a stale suspension.
pub fn begin_hide() {
    MAIN_WINDOW_HIDDEN.store(true, Ordering::Release);
}

/// Restores the marker when the native hide itself fails.
pub fn cancel_hide() {
    MAIN_WINDOW_HIDDEN.store(false, Ordering::Release);
}

/// Hides are followed by a best-effort asynchronous suspension. Failure leaves
/// the webview running normally and must never block hiding the window.
pub fn suspend_main_webview<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(error) = window.with_webview(|platform| {
        let result = (|| -> ComResult<()> {
            if !MAIN_WINDOW_HIDDEN.load(Ordering::Acquire) {
                return Ok(());
            }
            let controller = platform.controller();
            unsafe { controller.SetIsVisible(false)? };
            let webview = unsafe { controller.CoreWebView2() }?.cast::<ICoreWebView2_3>()?;
            let resume_webview = webview.clone();
            let handler = TrySuspendCompletedHandler::create(Box::new(
                move |result, suspended| -> ComResult<()> {
                    if let Err(error) = result {
                        crate::log_warn!(
                            "[webview] failed to suspend the hidden main window: {error}"
                        );
                        return Ok(());
                    }
                    if !suspended {
                        crate::log_debug!(
                            "[webview] WebView2 declined to suspend the hidden window"
                        );
                        return Ok(());
                    }
                    if !MAIN_WINDOW_HIDDEN.load(Ordering::Acquire) {
                        match unsafe { resume_if_suspended(&resume_webview) } {
                            Ok(true) => {
                                crate::log_debug!(
                                    "[webview] resumed after a show request raced suspension"
                                );
                            }
                            Ok(false) => {}
                            Err(error) => crate::log_warn!(
                                "[webview] failed to resume after a show/suspend race: {error}"
                            ),
                        }
                    }
                    Ok(())
                },
            ));
            unsafe { webview.TrySuspend(&handler) }
        })();

        if let Err(error) = result {
            crate::log_warn!("[webview] unable to suspend the hidden main window: {error}");
        }
    }) {
        crate::log_warn!("[webview] failed to schedule main-window suspension: {error}");
    }
}

/// Marks the main webview visible and resumes it before the window is shown.
pub fn resume_main_webview<R: Runtime>(window: &WebviewWindow<R>) {
    MAIN_WINDOW_HIDDEN.store(false, Ordering::Release);
    if let Err(error) = window.with_webview(|platform| {
        let result = (|| -> ComResult<()> {
            let controller = platform.controller();
            let webview = unsafe { controller.CoreWebView2() }?.cast::<ICoreWebView2_3>()?;
            unsafe { resume_if_suspended(&webview) }?;
            unsafe { controller.SetIsVisible(true)? };
            Ok(())
        })();

        if let Err(error) = result {
            crate::log_warn!("[webview] unable to resume the main window: {error}");
        }
    }) {
        crate::log_warn!("[webview] failed to schedule main-window resume: {error}");
    }
}

unsafe fn resume_if_suspended(webview: &ICoreWebView2_3) -> ComResult<bool> {
    let mut suspended = BOOL::default();
    webview.IsSuspended(&mut suspended)?;
    if !suspended.as_bool() {
        return Ok(false);
    }
    webview.Resume()?;
    Ok(true)
}
