use crate::platform::windows_clipboard::{ClipboardChange, WindowsClipboardMonitor};

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
