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
    #[cfg(not(target_os = "windows"))]
    wake_ipc: Option<super::local_wake::LocalWake>,
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
                    #[cfg(not(target_os = "windows"))]
                    let wake_ipc = match super::local_wake::LocalWake::bind(project_dir, pid) {
                        Ok(wake) => wake,
                        Err(error) => {
                            let _ = fs::remove_file(&lock_path);
                            return Err(SingleInstanceError::LockFile(format!(
                                "failed to bind wake channel: {error}"
                            )));
                        }
                    };
                    return Ok(Self {
                        lock_path,
                        pid,
                        #[cfg(target_os = "windows")]
                        wake_event,
                        #[cfg(not(target_os = "windows"))]
                        wake_ipc: Some(wake_ipc),
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
            self.wake_ipc
                .as_mut()
                .ok_or_else(|| SingleInstanceError::LockFile("wake channel closed".into()))?
                .start(callback)
                .map_err(|error| SingleInstanceError::LockFile(error.to_string()))?;
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
            super::local_wake::LocalWake::notify(project_dir, owner_pid)
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        #[cfg(not(target_os = "windows"))]
        drop(self.wake_ipc.take());
        if read_instance_lock_pid(&self.lock_path).ok().flatten() == Some(self.pid) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

/// What a failed `OpenProcess` says about the lock owner's liveness.
///
/// The classification itself is a pure function of the Win32 error code, so it
/// stays platform-independent and testable everywhere; only the `GetLastError`
/// call site is Windows-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
enum OwnerLiveness {
    /// The process exists. We may simply not be allowed to open it.
    Running,
    /// The pid does not exist, so the lock file is genuinely stale.
    Dead,
}

#[cfg(target_os = "windows")]
fn is_process_running(pid: u32) -> bool {
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
        fn GetLastError() -> u32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return classify_open_process_failure(GetLastError()) == OwnerLiveness::Running;
        }
        let mut exit_code = 0u32;
        GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        exit_code == STILL_ACTIVE
    }
}

/// Decides whether a failed `OpenProcess` means the owner is dead.
///
/// Only `ERROR_INVALID_PARAMETER` is documented to mean "no such process".
/// Every other failure must be read as *running*: `ERROR_ACCESS_DENIED` (5) is
/// what Windows returns when the owner is elevated or runs as a different user,
/// and treating that as dead made a second launch delete the live instance's
/// lock and start alongside it — two clipboard monitors, two capture loops, two
/// OCR workers, and two hotkey registrations on one database. The only safe
/// reading of an inconclusive error is the one that refuses to start.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn classify_open_process_failure(error_code: u32) -> OwnerLiveness {
    const ERROR_INVALID_PARAMETER: u32 = 87;
    if error_code == ERROR_INVALID_PARAMETER {
        OwnerLiveness::Dead
    } else {
        OwnerLiveness::Running
    }
}

#[cfg(not(target_os = "windows"))]
fn is_process_running(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    let result = unsafe { libc::kill(pid, 0) };
    // EPERM and inconclusive failures still mean a potentially live owner.
    result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "clipboard-single-instance-test-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&directory).expect("create temp dir");
        directory
    }

    #[test]
    fn create_instance_lock_writes_pid_exclusively() {
        let directory = temp_dir();
        let lock_path = directory.join("instance.lock");

        create_instance_lock(&lock_path, 4242).expect("first lock must succeed");
        assert_eq!(read_instance_lock_pid(&lock_path).unwrap(), Some(4242));

        // The second acquire on the same path fails as already-exists, which
        // is what drives the AlreadyRunning branch in `acquire`.
        let error = create_instance_lock(&lock_path, 5).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        // Original content is untouched by the failed attempt.
        assert_eq!(read_instance_lock_pid(&lock_path).unwrap(), Some(4242));

        fs::remove_file(&lock_path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn read_instance_lock_pid_rejects_invalid_and_zero_pids() {
        let directory = temp_dir();

        for (content, expected) in [
            ("123\n", Some(123)),
            ("  77 \n", Some(77)),
            ("", None),
            ("not-a-pid", None),
            ("-1", None),
            ("99999999999999", None),
            ("0", None),
        ] {
            let lock_path = directory.join("instance.lock");
            fs::write(&lock_path, content).unwrap();
            assert_eq!(
                read_instance_lock_pid(&lock_path).unwrap(),
                expected,
                "unexpected parse for {content:?}"
            );
            fs::remove_file(&lock_path).unwrap();
        }

        // A missing file reports NotFound rather than a usable pid.
        let missing = directory.join("missing.lock");
        assert_eq!(
            read_instance_lock_pid(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn current_process_detects_itself_as_running() {
        assert!(is_process_running(std::process::id()));
    }

    #[test]
    fn guard_acquire_creates_and_drop_removes_the_lock() {
        let directory = temp_dir();
        let lock_path = directory.join("instance.lock");

        let guard = SingleInstanceGuard::acquire(&directory)
            .expect("acquire must succeed in an empty directory");
        assert!(read_instance_lock_pid(&lock_path).unwrap().is_some());
        drop(guard);
        assert!(!lock_path.exists(), "drop must remove the owned lock");

        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn guard_acquire_reports_the_running_owner_after_retries() {
        let directory = temp_dir();
        let lock_path = directory.join("instance.lock");
        create_instance_lock(&lock_path, std::process::id()).unwrap();

        let acquired = SingleInstanceGuard::acquire(&directory);
        match acquired {
            Err(SingleInstanceError::AlreadyRunning(owner)) => {
                assert_eq!(owner, std::process::id());
            }
            Ok(_) => panic!("expected AlreadyRunning for an owned lock"),
            Err(error) => panic!("expected AlreadyRunning, got {error}"),
        }

        fs::remove_file(&lock_path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn guard_acquire_takes_over_a_stale_lock_with_a_dead_owner() {
        let directory = temp_dir();
        let lock_path = directory.join("instance.lock");

        // PID 0 never parses as a live owner; write a raw zero file so
        // `read_instance_lock_pid` returns None and the stale lock is removed.
        fs::write(&lock_path, b"0").unwrap();
        let guard = SingleInstanceGuard::acquire(&directory)
            .expect("a stale lock with no owner pid must be taken over");
        assert_eq!(
            read_instance_lock_pid(&lock_path).unwrap(),
            Some(std::process::id())
        );

        drop(guard);
        assert!(!lock_path.exists());
        fs::remove_dir(&directory).unwrap();
    }

    /// A failed `OpenProcess` is not evidence that the owner is dead. Only
    /// `ERROR_INVALID_PARAMETER` says "no such process"; `ERROR_ACCESS_DENIED`
    /// (5) is what Windows returns for an elevated or cross-session owner, and
    /// reading that as dead made a second launch delete the live instance's lock
    /// and start alongside it — duplicate capture, OCR workers, and hotkeys on
    /// one database.
    #[test]
    fn an_access_denied_owner_is_never_reported_as_dead() {
        const ERROR_ACCESS_DENIED: u32 = 5;
        const ERROR_INVALID_PARAMETER: u32 = 87;
        const STILL_ACTIVE: u32 = 259;

        // The bug's exact trigger.
        assert_eq!(
            classify_open_process_failure(ERROR_ACCESS_DENIED),
            OwnerLiveness::Running
        );
        // The only failure that genuinely means the pid is gone.
        assert_eq!(
            classify_open_process_failure(ERROR_INVALID_PARAMETER),
            OwnerLiveness::Dead
        );
        // Anything unrecognised is inconclusive, and the safe reading of
        // inconclusive is "do not start a second instance".
        for error_code in [0u32, 1, 6, 50, 87 + 1, 999, STILL_ACTIVE, u32::MAX] {
            if error_code == ERROR_INVALID_PARAMETER {
                continue;
            }
            assert_eq!(
                classify_open_process_failure(error_code),
                OwnerLiveness::Running,
                "error {error_code} must not be read as a dead owner"
            );
        }
    }

    #[test]
    fn notify_existing_instance_returns_false_for_a_missing_event() {
        let directory = temp_dir();
        // No instance was acquired here, so the named wake event cannot exist.
        assert!(!SingleInstanceGuard::notify_existing_instance(
            &directory,
            std::process::id()
        ));
        fs::remove_dir(&directory).unwrap();
    }
}
