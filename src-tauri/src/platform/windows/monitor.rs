//! Change monitoring: the event-driven Windows listener and the
//! polling/stub fallbacks for other platforms.

use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use super::*;
#[cfg(target_os = "windows")]
pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    last_sequence: u32,
    sender: Option<mpsc::Sender<ClipboardChange>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
}

#[cfg(target_os = "windows")]
impl Default for WindowsClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl WindowsClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            ignored_apps: vec![],
            last_sequence: 0,
            sender: None,
            stop_sender: None,
            handle: None,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Receiver<ClipboardChange>, String> {
        if self.running {
            // The monitor thread exits silently when the capture worker's
            // receiver is dropped (worker panic or spawn failure) — nothing
            // resets this flag. Detect the dead thread so a restart is not
            // permanently blocked by "already running".
            let thread_dead = self
                .handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished());
            if !thread_dead {
                return Err("clipboard monitor is already running".to_string());
            }
            self.running = false;
            self.handle = None;
        }

        let (sender, receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let sender_for_thread = sender.clone();

        let handle = thread::Builder::new()
            .name("clipboard-monitor".to_owned())
            .spawn(move || {
                let mut sequence = 0u32;
                loop {
                    match stop_receiver.recv_timeout(Duration::from_millis(300)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    let current_sequence = match read_clipboard_sequence() {
                        Some(seq) => seq,
                        None => continue,
                    };

                    if current_sequence == sequence {
                        continue;
                    }
                    sequence = current_sequence;

                    if has_self_trigger_format() {
                        if let (Some(marker), Some(text)) =
                            (read_self_trigger_marker(), read_clipboard_text())
                        {
                            if clipboard_change_is_self_write(&marker, &text) {
                                continue;
                            }
                        }
                    }

                    if sender_for_thread
                        .send(ClipboardChange { sequence })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| format!("failed to spawn clipboard monitor: {error}"))?;
        self.sender = Some(sender);
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        self.running = true;

        Ok(receiver)
    }

    pub fn stop(&mut self) {
        self.running = false;
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                // A panicked monitor explains why captures silently stopped;
                // swallowing the payload hid that from every log.
                if let Err(panic) = handle.join() {
                    crate::log_error!(
                        "[clipboard-monitor] monitor thread terminated with a panic: {panic:?}"
                    );
                }
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_apps = apps;
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "windows")]
pub(super) fn read_clipboard_sequence() -> Option<u32> {
    extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }

    unsafe {
        let seq = GetClipboardSequenceNumber();
        if seq == 0 {
            None
        } else {
            Some(seq)
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn read_clipboard_sequence() -> Option<u32> {
    None
}

// ---------------------------------------------------------------------------
//  Non-Windows stubs for WindowsClipboardMonitor and ClipboardChange
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub struct WindowsClipboardMonitor {
    running: bool,
    ignored_apps: Vec<String>,
    sender: Option<mpsc::Sender<ClipboardChange>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    /// Write end of the self-pipe that wakes an event-driven monitor
    /// (XFixes / Wayland data-control) on stop. The poll fallback uses the
    /// mpsc stop channel instead.
    #[cfg(target_os = "linux")]
    stop_writer: Option<crate::platform::linux::stop_pipe::StopPipeWriter>,
}

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub sequence: u32,
}

#[cfg(not(target_os = "windows"))]
impl Default for WindowsClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "windows"))]
impl WindowsClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: false,
            ignored_apps: vec![],
            sender: None,
            stop_sender: None,
            handle: None,
            #[cfg(target_os = "linux")]
            stop_writer: None,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Receiver<ClipboardChange>, String> {
        if self.running {
            // The monitor thread exits silently when the capture worker's
            // receiver is dropped (worker panic or spawn failure) — nothing
            // resets this flag. Detect the dead thread so a restart is not
            // permanently blocked by "already running".
            let thread_dead = self
                .handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished());
            if !thread_dead {
                return Err("clipboard monitor is already running".to_string());
            }
            self.running = false;
            self.handle = None;
        }

        let (sender, receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let sender_for_thread = sender.clone();

        // Event-driven monitors push a change notification as soon as the
        // selection owner changes. Linux tries XFixes (X11) and the
        // data-control protocols (Wayland); any failure returns None and the
        // generic 500 ms poll loop below takes over. macOS has no clipboard
        // change push API at all (`NSPasteboard` exposes only a change count),
        // so polling is the platform standard there — see Maccy/Paste.
        #[cfg(target_os = "linux")]
        let event_handle: Option<(
            JoinHandle<()>,
            crate::platform::linux::stop_pipe::StopPipeWriter,
        )> = match crate::platform::Platform::detect() {
            crate::platform::Platform::LinuxX11 => {
                crate::platform::linux::x11::try_spawn_xfixes_monitor(sender_for_thread.clone())
            }
            crate::platform::Platform::LinuxWayland => {
                crate::platform::linux::wayland::try_spawn_data_control_monitor(
                    sender_for_thread.clone(),
                )
            }
            _ => None,
        };

        #[cfg(target_os = "linux")]
        if let Some((event_thread, stop_writer)) = event_handle {
            self.sender = Some(sender);
            self.stop_sender = Some(stop_sender);
            self.handle = Some(event_thread);
            self.stop_writer = Some(stop_writer);
            self.running = true;
            return Ok(receiver);
        }

        let handle = Self::spawn_poll_monitor(sender_for_thread, stop_receiver)?;

        self.sender = Some(sender);
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        self.running = true;

        Ok(receiver)
    }

    pub fn stop(&mut self) {
        self.running = false;
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        #[cfg(target_os = "linux")]
        if let Some(writer) = self.stop_writer.take() {
            // Wakes the event-driven monitor thread out of its blocking poll.
            writer.trigger();
        }
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                // A panicked monitor explains why captures silently stopped;
                // swallowing the payload hid that from every log.
                if let Err(panic) = handle.join() {
                    crate::log_error!(
                        "[clipboard-monitor] monitor thread terminated with a panic: {panic:?}"
                    );
                }
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.ignored_apps = apps;
    }

    /// The generic 500 ms poll loop. This is the fallback everywhere: on Linux
    /// when XFixes / data-control are unavailable, and permanently on macOS
    /// (no clipboard change push API — `NSPasteboard` only exposes a change
    /// count, so polling is what every clipboard manager does there).
    #[cfg(not(target_os = "windows"))]
    fn spawn_poll_monitor(
        sender_for_thread: mpsc::Sender<ClipboardChange>,
        stop_receiver: mpsc::Receiver<()>,
    ) -> Result<JoinHandle<()>, String> {
        thread::Builder::new()
            .name("clipboard-monitor".to_owned())
            .spawn(move || {
                let mut poll_state = crate::platform::ClipboardPollState::new();
                let mut sequence = 0u32;

                loop {
                    match stop_receiver.recv_timeout(Duration::from_millis(500)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    let platform = crate::platform::platform();
                    let current_text = platform.read_clipboard_text();
                    // Text is the cheap signal. File and image reads only run
                    // when there is no text, so image decoding does not happen
                    // on every tick while a text selection sits on the
                    // clipboard, while image/file copies are still detected.
                    let (current_files, current_image) = if current_text.is_none() {
                        let files = platform.read_clipboard_file_paths();
                        let image = if files.is_empty() {
                            platform.read_clipboard_image()
                        } else {
                            None
                        };
                        (files, image)
                    } else {
                        (Vec::new(), None)
                    };

                    if poll_state.observe(current_text, current_files, current_image) {
                        sequence = sequence.wrapping_add(1);
                        let _ = sender_for_thread.send(ClipboardChange { sequence });
                    }
                }
            })
            .map_err(|error| format!("failed to spawn clipboard monitor: {error}"))
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for WindowsClipboardMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
//  App-facing monitor facade: wraps the concrete monitor for the app layer
//  ---------------------------------------------------------------------------

pub struct ClipboardMonitor {
    monitor: WindowsClipboardMonitor,
    pub running: bool,
    pub last_check_at: i64,
    pub ignored_applications: Vec<String>,
    receiver: Option<std::sync::mpsc::Receiver<ClipboardChange>>,
}

impl Default for ClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardMonitor {
    pub fn new() -> Self {
        Self {
            monitor: WindowsClipboardMonitor::new(),
            running: false,
            last_check_at: 0,
            ignored_applications: Vec::new(),
            receiver: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        let receiver = self.monitor.start()?;
        self.receiver = Some(receiver);
        self.running = true;
        self.last_check_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.monitor.stop();
        self.running = false;
        self.receiver = None;
        Ok(())
    }

    pub fn take_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<ClipboardChange>> {
        self.receiver.take()
    }

    pub fn set_ignored_apps(&mut self, apps: Vec<String>) {
        self.monitor.set_ignored_apps(apps.clone());
        self.ignored_applications = apps;
    }
}
