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
        // The rename has already taken effect, so a directory-fsync failure
        // must not be reported as a write failure: callers roll back their
        // in-memory state on Err, which would fork the runtime state from
        // the (already replaced) file on disk. The fsync is a durability
        // enhancement only; degrade to ignoring the error.
        if let Some(parent) = target.parent()
            && let Ok(directory) = std::fs::File::open(parent)
        {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }

        fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
            if path.as_os_str().encode_wide().any(|word| word == 0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "replacement path contains NUL",
                ));
            }
            let name = path.file_name().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "replacement path has no file name",
                )
            })?;
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            // Canonicalize only the parent to obtain a verbatim long path.
            // Resolving the file itself would rename a symlink's referent.
            Ok(std::fs::canonicalize(parent)?
                .join(name)
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect())
        }

        let existing = wide(temporary)?;
        let new = wide(target)?;
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
    fn replacement_creates_missing_targets_and_preserves_hard_link_aliases() {
        let directory =
            std::env::temp_dir().join(format!("clipboard-atomic-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join("target");
        let temporary = directory.join("staging");
        std::fs::write(&temporary, b"first").unwrap();
        replace_file(&temporary, &target).unwrap();
        let alias = directory.join("alias");
        std::fs::hard_link(&target, &alias).unwrap();
        std::fs::write(&temporary, b"second").unwrap();
        replace_file(&temporary, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        assert_eq!(std::fs::read(&alias).unwrap(), b"first");
        assert!(!temporary.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn replacement_accepts_long_paths_without_a_verbatim_prefix() {
        let directory =
            std::env::temp_dir().join(format!("clipboard-atomic-long-{}", uuid::Uuid::new_v4()));
        let parent = directory.join("a".repeat(100)).join("b".repeat(100));
        std::fs::create_dir_all(&parent).unwrap();
        let temporary = parent.join("staging");
        let target = parent.join("target");
        assert!(temporary.as_os_str().len() > 260);
        std::fs::write(&temporary, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        let result = replace_file(&temporary, &target);
        let bytes = std::fs::read(&target).unwrap();
        let staged = temporary.exists();
        std::fs::remove_dir_all(directory).unwrap();
        assert!(result.is_ok(), "legal long path rejected: {result:?}");
        assert_eq!(bytes, b"new");
        assert!(!staged);
    }

    #[cfg(windows)]
    #[test]
    fn replacement_rejects_nul_in_either_path_without_file_effects() {
        for invalid_source in [true, false] {
            let directory =
                std::env::temp_dir().join(format!("clipboard-atomic-nul-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let temporary = directory.join("staging");
            let target = directory.join("target");
            std::fs::write(&temporary, b"new").unwrap();
            std::fs::write(&target, b"old").unwrap();
            let result = if invalid_source {
                replace_file(&directory.join("staging\0suffix"), &target)
            } else {
                replace_file(&temporary, &directory.join("target\0suffix"))
            };
            let bytes = std::fs::read(&target).unwrap();
            let staged = std::fs::read(&temporary);
            std::fs::remove_dir_all(directory).unwrap();
            assert!(result.is_err(), "NUL must not truncate either native path");
            assert_eq!(bytes, b"old");
            assert_eq!(staged.unwrap(), b"new");
        }
    }

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
