//! Exclusively create private scratch directories before writing sensitive data.
use std::{io, path::Path};

#[cfg(unix)]
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(target_os = "windows")]
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        InitializeSecurityDescriptor, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl,
    };
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

    if path.as_os_str().encode_wide().any(|word| word == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path contains NUL",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path has no file name",
        )
    })?;
    let name: Vec<u16> = std::fs::canonicalize(parent)?
        .join(name)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    super::windows::security::with_current_user_acl(SUB_CONTAINERS_AND_OBJECTS_INHERIT, |acl| {
        // SAFETY: the descriptor, ACL, attributes and terminated path live
        // through CreateDirectoryW. Existing entries are never reused or changed.
        unsafe {
            let mut descriptor = SECURITY_DESCRIPTOR::default();
            let pointer = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
            if InitializeSecurityDescriptor(pointer, 1) == 0
                || SetSecurityDescriptorDacl(pointer, 1, acl, 0) == 0
                || SetSecurityDescriptorControl(pointer, SE_DACL_PROTECTED, SE_DACL_PROTECTED) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: pointer,
                bInheritHandle: 0,
            };
            if CreateDirectoryW(name.as_ptr(), &attributes) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    })
}

#[cfg(all(not(unix), not(target_os = "windows")))]
pub(crate) fn create_private_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private directories are unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("clipboard-private-dir-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creation_never_reuses_an_existing_entry_or_creates_missing_parents() {
        let root = Root::new();
        let directory = root.0.join("existing");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("keep.txt"), b"keep existing bytes").unwrap();
        let file = root.0.join("file");
        fs::write(&file, b"keep file bytes").unwrap();
        assert!(create_private_directory(&directory).is_err());
        assert!(create_private_directory(&file).is_err());
        assert_eq!(
            fs::read(directory.join("keep.txt")).unwrap(),
            b"keep existing bytes"
        );
        assert_eq!(fs::read(&file).unwrap(), b"keep file bytes");
        assert!(create_private_directory(&root.0.join("missing/child")).is_err());
        assert!(!root.0.join("missing").exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn private_directory_creation_supports_unicode_and_verbatim_long_paths() {
        let root = Root::new();
        let unicode = root.0.join("临时-😀");
        create_private_directory(&unicode).unwrap();
        fs::write(unicode.join("content"), b"unicode data").unwrap();
        let parent = root
            .0
            .join("x".repeat(90))
            .join("y".repeat(90))
            .join("z".repeat(90));
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join("private");
        assert!(path.as_os_str().len() > 260);
        create_private_directory(&path).unwrap();
        fs::write(path.join("content"), b"long path data").unwrap();
        assert!(create_private_directory(Path::new("bad\0directory")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn private_directory_denies_group_and_other_access_before_writing() {
        use std::os::unix::fs::PermissionsExt;
        let root = Root::new();
        let path = root.0.join("private");
        create_private_directory(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        fs::write(path.join("records.jsonl"), b"synthetic records").unwrap();
    }
}
