//! Persistent diagnostics channel.
//!
//! Release builds run as a GUI-subsystem process: stderr would disappear
//! into the hidden console created by `attach_hidden_console` and be lost on
//! exit, leaving worker failures with zero trace. [`init`] redirects the
//! process stderr handle to a size-capped log file so every existing
//! `eprintln!` diagnostic (OCR failures, thumbnail errors, recovery notes,
//! ...) survives in production without touching its call site.
//!
//! On non-Windows platforms the app runs attached to a terminal, so stderr
//! is left alone and only a session marker is written.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const LOG_DIRECTORY_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "clipboard.log";
const OLD_LOG_FILE_NAME: &str = "clipboard.log.old";
const MAX_LOG_BYTES: u64 = 512 * 1024;
/// Long-running sessions must not grow the log without bound, but checking
/// the file size on every line would dominate the cost of logging. Sample at
/// most this often.
const SIZE_CHECK_INTERVAL_MS: u64 = 30_000;

static ACTIVE_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LAST_SIZE_CHECK_MS: AtomicU64 = AtomicU64::new(0);

/// Redirects process stderr into `<project>/logs/clipboard.log`, rotating the
/// previous log aside when it exceeds [`MAX_LOG_BYTES`]. Returns the active
/// log path when redirection is active.
pub fn init(project_directory: &Path) -> Option<PathBuf> {
    let logs_directory = project_directory.join(LOG_DIRECTORY_NAME);
    fs::create_dir_all(&logs_directory).ok()?;
    let log_path = logs_directory.join(LOG_FILE_NAME);

    rotate_if_oversized(&log_path);

    let file = open_log_file(&log_path)?;

    #[cfg(target_os = "windows")]
    {
        if !redirect_stderr(&file) {
            return None;
        }
    }

    // Keep the handle alive for the lifetime of the process; the OS closes
    // it on exit and dropping it here would invalidate the redirected handle.
    std::mem::forget(file);
    let _ = ACTIVE_LOG_PATH.set(log_path.clone());

    eprintln!("[logging] session started {}", timestamp_now());
    Some(log_path)
}

/// Writes one timestamped diagnostic line to stderr (and therefore to the
/// redirected log file when active). Prefer this over bare `eprintln!` in
/// worker modules so production logs are attributable in time.
pub fn log_line(message: &str) {
    maybe_truncate_oversized_log();
    eprintln!("[{}] {message}", timestamp_now());
}

/// `log_event!("...{x}", x = 1)` — [`format!`]-style timestamped diagnostics.
#[macro_export]
macro_rules! log_event {
    ($($arg:tt)*) => {
        $crate::logging::log_line(&format!($($arg)*))
    };
}

fn rotate_if_oversized(log_path: &Path) {
    let oversized = fs::metadata(log_path)
        .map(|meta| meta.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if !oversized {
        return;
    }
    let old_path = log_path.with_file_name(OLD_LOG_FILE_NAME);
    if old_path.exists() {
        let _ = fs::remove_file(&old_path);
    }
    let _ = fs::rename(log_path, &old_path);
}

/// Runtime counterpart of [`rotate_if_oversized`]. The redirected stderr
/// handle stays valid, so instead of renaming the file away (which would
/// detach subsequent writes from the path) the oversized log is truncated in
/// place: the append handle always writes at end-of-file, so after truncation
/// logging simply restarts at offset 0. Throttled to one size sample per
/// [`SIZE_CHECK_INTERVAL_MS`].
fn maybe_truncate_oversized_log() {
    const UNSET: u64 = 0;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0);
    if now == 0 {
        return;
    }
    let last = LAST_SIZE_CHECK_MS.load(Ordering::Relaxed);
    if last != UNSET && now.saturating_sub(last) < SIZE_CHECK_INTERVAL_MS {
        return;
    }
    if LAST_SIZE_CHECK_MS
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let Some(log_path) = ACTIVE_LOG_PATH.get() else {
        return;
    };
    truncate_log_if_oversized(log_path);
}

fn truncate_log_if_oversized(log_path: &Path) {
    let oversized = fs::metadata(log_path)
        .map(|meta| meta.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if !oversized {
        return;
    }
    if OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(log_path)
        .is_ok()
    {
        eprintln!(
            "[{}] [logging] log truncated after exceeding size cap",
            timestamp_now()
        );
    }
}

fn open_log_file(log_path: &Path) -> Option<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .ok()
}

fn timestamp_now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

#[cfg(target_os = "windows")]
fn redirect_stderr(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;

    const STD_ERROR_HANDLE: u32 = 12;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetStdHandle(nstdhandle: u32, hhandle: isize) -> i32;
    }

    let handle = file.as_raw_handle() as isize;
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_oversized_log_into_backup() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("clipboard-logging-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let log_path = directory.join(LOG_FILE_NAME);
        fs::write(&log_path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();

        rotate_if_oversized(&log_path);

        assert!(!log_path.exists(), "oversized log was not rotated aside");
        let backup = fs::read(directory.join(OLD_LOG_FILE_NAME)).unwrap();
        assert_eq!(backup.len(), MAX_LOG_BYTES as usize + 1);

        // A fresh, small log must be left alone.
        fs::write(&log_path, b"session").unwrap();
        rotate_if_oversized(&log_path);
        assert!(log_path.exists());
        assert_eq!(fs::read(&log_path).unwrap(), b"session");

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn runtime_truncation_resets_oversized_log_in_place() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("clipboard-logging-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let log_path = directory.join(LOG_FILE_NAME);
        fs::write(&log_path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();

        // Simulate the redirected append handle staying open while the file
        // is truncated underneath it.
        let append_handle = OpenOptions::new().append(true).open(&log_path).unwrap();
        truncate_log_if_oversized(&log_path);

        use std::io::Write;
        writeln!(&append_handle, "after truncate").unwrap();
        let content = fs::read_to_string(&log_path).unwrap();
        assert_eq!(content, "after truncate\n");

        drop(append_handle);
        let _ = fs::remove_dir_all(&directory);
    }
}
