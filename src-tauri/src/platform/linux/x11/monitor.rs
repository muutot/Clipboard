//! XFixes-backed event-driven clipboard monitor (Linux only).

#[cfg(target_os = "linux")]
use super::*;

// ---------------------------------------------------------------------------
// XFixes event-driven clipboard monitor (Linux only)
// ---------------------------------------------------------------------------

/// `XFixesSetSelectionOwnerNotifyMask` — subscribe to selection owner changes.
#[cfg(target_os = "linux")]
const XFIXES_SET_SELECTION_OWNER_NOTIFY_MASK: u64 = 1 << 0;

/// Xlib calls the IO error handler when the display connection dies (X server
/// restart, session logout). The default handler terminates the whole process,
/// which must not take the desktop app down with the session. The handler runs
/// with the dead display's lock held and Xlib does not survive a plain return,
/// so end only the monitor thread via `pthread_exit`. The dead connection and
/// the two pipe fds stay unreclaimed until process exit, which is the price of
/// keeping the app alive; nothing else in-process touches this display.
#[cfg(target_os = "linux")]
extern "C" fn x11_io_error_handler(_display: *mut x11_ffi::Display) -> i32 {
    crate::log_error!("[clipboard-monitor] X11 connection lost; stopping the monitor thread only");
    unsafe { libc::pthread_exit(std::ptr::null_mut()) }
}

/// Spawns the event-driven clipboard monitor backed by the XFixes extension.
///
/// The monitor thread subscribes to `XFixesSelectionNotify` for the CLIPBOARD
/// selection and sends a [`ClipboardChange`] as soon as the selection owner
/// changes — no polling. It blocks in `poll(2)` on the X connection fd plus
/// the read end of a stop pipe, so `stop()` wakes it immediately.
///
/// Returns `None` when XFixes is unavailable (no X display, extension missing,
/// thread spawn failure); the caller then falls back to the generic poll loop.
///
/// The returned [`StopPipeWriter`] must be triggered (or dropped) on stop; the
/// monitor thread owns and closes the pipe's read end.
#[cfg(target_os = "linux")]
pub(crate) fn try_spawn_xfixes_monitor(
    sender: std::sync::mpsc::Sender<crate::platform::windows::ClipboardChange>,
) -> Option<(
    thread::JoinHandle<()>,
    crate::platform::linux::stop_pipe::StopPipeWriter,
)> {
    use crate::platform::linux::stop_pipe::StopPipePair;
    use crate::platform::windows::ClipboardChange;

    // Raw Xlib display pointer. The monitor thread takes exclusive
    // ownership of the connection, so wrapping it in a Send marker is
    // sound even though `*mut c_void` itself is not Send.
    #[repr(transparent)]
    struct XDisplayPtr(*mut std::ffi::c_void);
    unsafe impl Send for XDisplayPtr {}
    impl XDisplayPtr {
        fn ptr(self) -> *mut std::ffi::c_void {
            self.0
        }
    }

    let stop = StopPipePair::new().ok()?;

    // SAFETY: Xlib FFI. The display is opened, configured, and consumed
    // entirely on this thread; on any setup failure every created resource is
    // closed before returning None. After a successful spawn, ownership of
    // `display` and `window` moves to the monitor thread.
    let (display, event_base, window) = unsafe {
        let display = x11_ffi::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return None;
        }
        let mut event_base: i32 = 0;
        let mut error_base: i32 = 0;
        if x11_ffi::XFixesQueryExtension(display, &mut event_base, &mut error_base) == 0 {
            x11_ffi::XCloseDisplay(display);
            return None;
        }
        // XFixes requires a version negotiation before requests are used;
        // selection input exists since XFixes 1.0.
        let (mut major, mut minor) = (1, 0);
        x11_ffi::XFixesQueryVersion(display, &mut major, &mut minor);
        let root = x11_ffi::XDefaultRootWindow(display);
        let window = x11_ffi::XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0);
        if window == 0 {
            x11_ffi::XCloseDisplay(display);
            return None;
        }
        // CStr literal for the NUL terminator; the cast stays because the FFI
        // declares `*const i8` while `c_char` is unsigned on aarch64 Linux.
        let clipboard = x11_ffi::XInternAtom(display, c"CLIPBOARD".as_ptr().cast(), 0);
        if clipboard == 0 {
            x11_ffi::XDestroyWindow(display, window);
            x11_ffi::XCloseDisplay(display);
            return None;
        }
        x11_ffi::XFixesSelectSelectionInput(
            display,
            window,
            clipboard,
            XFIXES_SET_SELECTION_OWNER_NOTIFY_MASK,
        );
        // Make sure the subscription is on the server before we start waiting.
        x11_ffi::XSync(display, 0);
        (display, event_base, window)
    };
    let x_fd = unsafe { x11_ffi::XConnectionNumber(display) };
    // Copy of the raw pointer kept on this thread so the spawn-failure path
    // can still clean up after the closure took ownership of the wrapper.
    let raw_display = display;
    let display = XDisplayPtr(display);
    let stop_reader_fd = stop.reader_fd();

    let spawn_result = thread::Builder::new()
        .name("x11-clipboard-monitor".to_owned())
        .spawn(move || {
            // Method call (instead of `display.0`) so the closure captures
            // the whole Send wrapper, not the raw pointer field.
            let display = display.ptr();
            // Install the crash guard before the first Xlib read: without it
            // a dead X connection (server restart, logout) invokes Xlib's
            // default IO handler and terminates the whole application.
            // SAFETY: Xlib call; the handler is a plain function pointer.
            unsafe {
                x11_ffi::XSetIOErrorHandler(Some(x11_io_error_handler));
            }
            let mut fds = [
                libc::pollfd {
                    fd: x_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop_reader_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let mut sequence: u32 = 0;
            loop {
                // SAFETY: both fds are valid for the lifetime of the loop.
                let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if ready < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                if fds[1].revents != 0 {
                    break; // stop requested
                }
                if fds[0].revents & libc::POLLIN == 0 {
                    // POLLERR/POLLHUP/POLLNVAL without POLLIN: the connection
                    // is gone but Xlib has not raised the IO error path.
                    // Break instead of spinning forever on a poll that can
                    // never drain.
                    if fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                        crate::log_error!(
                            "[clipboard-monitor] X11 connection poll error; stopping the monitor thread"
                        );
                        break;
                    }
                    continue;
                }
                // Drain everything already queued on the connection.
                while unsafe { x11_ffi::XPending(display) } > 0 {
                    // SAFETY: `event` is a valid XEvent storage; XNextEvent
                    // fully initialises it before we read the type tag.
                    let mut event: x11_ffi::XEvent = unsafe { std::mem::zeroed() };
                    unsafe { x11_ffi::XNextEvent(display, &mut event) };
                    // XFixesSelectionNotify is event 0 in the XFixes event
                    // space, so the wire type is exactly `event_base`.
                    let event_type = unsafe { event.data.any.type_ };
                    if event_type == event_base {
                        sequence = sequence.wrapping_add(1);
                        if sender.send(ClipboardChange { sequence }).is_err() {
                            // Capture worker is gone; stop the monitor.
                            break;
                        }
                    }
                }
            }
            // SAFETY: this thread owns the display connection and window.
            unsafe {
                x11_ffi::XDestroyWindow(display, window);
                x11_ffi::XCloseDisplay(display);
                libc::close(stop_reader_fd);
            }
        });
    match spawn_result {
        Ok(handle) => Some((handle, stop.into_writer())),
        // The pair's Drop closes both pipe fds; the display/window were never
        // handed to a thread.
        Err(_) => {
            let display = raw_display;
            // SAFETY: this thread still owns the display connection.
            unsafe {
                x11_ffi::XDestroyWindow(display, window);
                x11_ffi::XCloseDisplay(display);
            }
            None
        }
    }
}
