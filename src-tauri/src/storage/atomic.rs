//! Durable atomic file replacement shared by the JSON config stores.

use std::path::Path;

use super::StorageError;

/// Atomically replaces `target` with the already-written `temporary` file and
/// makes the replacement as durable as the platform allows.
///
/// On Unix the parent directory is `fsync`'d after the rename so the directory
/// entry survives a crash. On Windows the rename uses
/// `MoveFileExW(.., MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`, which
/// asks the filesystem to flush the move before returning. On other platforms
/// it falls back to a plain rename.
pub fn replace_file(temporary: &Path, target: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        std::fs::rename(temporary, target)?;
        if let Some(parent) = target.parent() {
            if let Ok(directory) = std::fs::File::open(parent) {
                directory.sync_all()?;
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

        extern "system" {
            fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }

        fn wide(path: &Path) -> Vec<u16> {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        let existing = wide(temporary);
        let new = wide(target);
        let result = unsafe {
            MoveFileExW(
                existing.as_ptr(),
                new.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            return Err(StorageError::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        std::fs::rename(temporary, target)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_target_with_the_temporary_file() {
        let directory =
            std::env::temp_dir().join(format!("clipboard-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("conf.json");
        let temporary = directory.join(".conf.json.tmp");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&temporary, b"new").unwrap();

        replace_file(&temporary, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!temporary.exists());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
