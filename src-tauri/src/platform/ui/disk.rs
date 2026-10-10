//! Free-space reporting for the volume that stores a directory.

use std::path::Path;

use serde::Serialize;

/// Capacity information for the volume that stores a given directory.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Queries the total and caller-available capacity of the volume holding
/// `path`. Returns `None` when the platform query is unavailable or fails.
#[cfg(target_os = "windows")]
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory_name: *const u16,
            free_bytes_available: *mut u64,
            total_number_of_bytes: *mut u64,
            total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    let succeeded =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if succeeded == 0 {
        return None;
    }
    Some(DiskSpace {
        total_bytes: total,
        available_bytes: available,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn disk_space(_path: &Path) -> Option<DiskSpace> {
    None
}
