use std::{
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

#[cfg(target_os = "windows")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "windows")]
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
};

pub struct SingleInstanceGuard {
    lock_path: PathBuf,
    pid: u32,
    #[cfg(target_os = "windows")]
    wake_event: WindowsWakeEvent,
}

#[derive(Debug)]
pub enum SingleInstanceError {
    AlreadyRunning(u32),
    LockFile(String),
}

impl fmt::Display for SingleInstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning(pid) => {
                write!(
                    formatter,
                    "another instance is already running (PID: {pid})"
                )
            }
            Self::LockFile(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SingleInstanceError {}

#[cfg(target_os = "windows")]
struct WindowsWakeEvent {
    handle: isize,
    stop: Arc<AtomicBool>,
    listener: Mutex<Option<JoinHandle<()>>>,
}

#[cfg(target_os = "windows")]
impl WindowsWakeEvent {
    const EVENT_MODIFY_STATE: u32 = 0x0002;
    const INFINITE: u32 = u32::MAX;
    const WAIT_OBJECT_0: u32 = 0x0000;

    fn name(project_dir: &Path, pid: u32) -> Vec<u16> {
        let mut project_hash = 14695981039346656037u64;
        for byte in project_dir
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_bytes()
        {
            project_hash ^= u64::from(*byte);
            project_hash = project_hash.wrapping_mul(1099511628211);
        }
        format!("Local\\ClipboardDesktopSingleInstance-{project_hash:016x}-{pid}\0")
            .encode_utf16()
            .collect()
    }

    fn create(project_dir: &Path, pid: u32) -> io::Result<Self> {
        extern "system" {
            fn CreateEventW(
                attributes: *const std::ffi::c_void,
                manual_reset: i32,
                initial_state: i32,
                name: *const u16,
            ) -> isize;
        }

        let name = Self::name(project_dir, pid);
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if handle == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            handle,
            stop: Arc::new(AtomicBool::new(false)),
            listener: Mutex::new(None),
        })
    }

    fn start_listener<F>(&self, callback: F) -> io::Result<()>
    where
        F: Fn() + Send + 'static,
    {
        extern "system" {
            fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
        }

        let mut listener = self
            .listener
            .lock()
            .map_err(|_| io::Error::other("wake listener lock poisoned"))?;
        if listener.is_some() {
            return Ok(());
        }

        let handle = self.handle;
        let stop = Arc::clone(&self.stop);
        let thread = thread::Builder::new()
            .name("single-instance-wake".to_owned())
            .spawn(move || loop {
                let result = unsafe { WaitForSingleObject(handle, Self::INFINITE) };
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if result != Self::WAIT_OBJECT_0 {
                    break;
                }
                callback();
            })?;
        *listener = Some(thread);
        Ok(())
    }

    fn notify(project_dir: &Path, pid: u32) -> bool {
        extern "system" {
            fn AllowSetForegroundWindow(process_id: u32) -> i32;
            fn OpenEventW(desired_access: u32, inherit_handle: i32, name: *const u16) -> isize;
            fn SetEvent(handle: isize) -> i32;
            fn CloseHandle(handle: isize) -> i32;
        }

        let name = Self::name(project_dir, pid);
        let handle = unsafe { OpenEventW(Self::EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle == 0 {
            return false;
        }

        unsafe {
            let _ = AllowSetForegroundWindow(pid);
        }
        let signaled = unsafe { SetEvent(handle) != 0 };
        unsafe {
            CloseHandle(handle);
        }
        signaled
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsWakeEvent {
    fn drop(&mut self) {
        extern "system" {
            fn CloseHandle(handle: isize) -> i32;
            fn SetEvent(handle: isize) -> i32;
        }

        let listener = match self.listener.get_mut() {
            Ok(listener) => listener,
            Err(error) => error.into_inner(),
        };
        if let Some(thread) = listener.take() {
            self.stop.store(true, Ordering::SeqCst);
            unsafe {
                let _ = SetEvent(self.handle);
            }
            let _ = thread.join();
        }

        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn create_instance_lock(lock_path: &Path, pid: u32) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path)?;
    if let Err(error) = writeln!(file, "{pid}").and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(lock_path);
        return Err(error);
    }
    Ok(())
}

fn read_instance_lock_pid(lock_path: &Path) -> io::Result<Option<u32>> {
    let content = fs::read_to_string(lock_path)?;
    Ok(content.trim().parse::<u32>().ok().filter(|pid| *pid != 0))
}

impl SingleInstanceGuard {
    pub fn acquire(project_dir: &Path) -> Result<Self, SingleInstanceError> {
        let lock_path = project_dir.join("instance.lock");
        let pid = std::process::id();
        #[cfg(target_os = "windows")]
        let wake_event = WindowsWakeEvent::create(project_dir, pid).map_err(|error| {
            SingleInstanceError::LockFile(format!(
                "failed to create single-instance wake event: {error}"
            ))
        })?;

        for attempt in 0..10 {
            match create_instance_lock(&lock_path, pid) {
                Ok(()) => {
                    return Ok(Self {
                        lock_path,
                        pid,
                        #[cfg(target_os = "windows")]
                        wake_event,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    match read_instance_lock_pid(&lock_path) {
                        Ok(Some(owner_pid)) if is_process_running(owner_pid) => {
                            if attempt < 9 {
                                sleep(Duration::from_millis(300));
                                continue;
                            }
                            return Err(SingleInstanceError::AlreadyRunning(owner_pid));
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(SingleInstanceError::LockFile(format!(
                                "failed to read instance lock {}: {error}",
                                lock_path.display()
                            )));
                        }
                    }

                    match fs::remove_file(&lock_path) {
                        Ok(()) => continue,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(SingleInstanceError::LockFile(format!(
                                "failed to remove stale instance lock {}: {error}",
                                lock_path.display()
                            )));
                        }
                    }
                }
                Err(error) => {
                    return Err(SingleInstanceError::LockFile(format!(
                        "failed to create instance lock {}: {error}",
                        lock_path.display()
                    )));
                }
            }
        }

        Err(SingleInstanceError::LockFile(format!(
            "instance lock {} changed repeatedly during startup",
            lock_path.display()
        )))
    }

    pub fn start_wake_listener<F>(&mut self, callback: F) -> Result<(), SingleInstanceError>
    where
        F: Fn() + Send + 'static,
    {
        #[cfg(target_os = "windows")]
        {
            self.wake_event.start_listener(callback).map_err(|error| {
                SingleInstanceError::LockFile(format!(
                    "failed to start single-instance wake listener: {error}"
                ))
            })?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = callback;
        }
        Ok(())
    }

    pub fn notify_existing_instance(project_dir: &Path, owner_pid: u32) -> bool {
        #[cfg(target_os = "windows")]
        {
            WindowsWakeEvent::notify(project_dir, owner_pid)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = project_dir;
            let _ = owner_pid;
            false
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if read_instance_lock_pid(&self.lock_path).ok().flatten() == Some(self.pid) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

#[cfg(target_os = "windows")]
fn is_process_running(pid: u32) -> bool {
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return false;
        }
        let mut exit_code = 0u32;
        GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        exit_code == STILL_ACTIVE
    }
}

#[cfg(not(target_os = "windows"))]
fn is_process_running(pid: u32) -> bool {
    unsafe {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(pid as i32, 0) == 0
    }
}
