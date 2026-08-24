//! DPAPI-based protection for secrets stored at rest (Windows).
//!
//! `protect` wraps a plaintext value with the current user's DPAPI scope and
//! returns a self-describing `"dpapi1:<hex>"` envelope. `unprotect` reverses
//! envelopes it produced and returns `None` for anything else, so values
//! written before this mechanism existed keep working until the next save
//! transparently upgrades them.
//!
//! Non-Windows platforms intentionally return `None`: callers then store the
//! value as before. This is a no-regression design 鈥?encryption is additive.

use std::ffi::c_void;

const ENVELOPE_PREFIX: &str = "dpapi1:";
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    #[repr(C)]
    pub(super) struct DataBlob {
        pub cb_data: u32,
        pub pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            pdatain: *const DataBlob,
            szdatadescr: *const u16,
            poptionalentropy: *const DataBlob,
            pvreserved: *mut c_void,
            ppromptstruct: *const c_void,
            dwflags: u32,
            pdataout: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            pdatain: *const DataBlob,
            ppszdatadescr: *mut *mut u16,
            poptionalentropy: *const DataBlob,
            pvreserved: *mut c_void,
            ppromptstruct: *const c_void,
            dwflags: u32,
            pdataout: *mut DataBlob,
        ) -> i32;
        fn LocalFree(hmem: isize) -> isize;
    }

    pub(super) fn protect_raw(plain: &[u8]) -> Option<Vec<u8>> {
        let mut input_bytes = plain.to_vec();
        let input = DataBlob {
            cb_data: input_bytes.len() as u32,
            pb_data: input_bytes.as_mut_ptr(),
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 || output.pb_data.is_null() {
            return None;
        }
        let protected =
            unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe { LocalFree(output.pb_data as isize) };
        // Best-effort scrub of the local plaintext copy.
        for byte in input_bytes.iter_mut() {
            *byte = 0;
        }
        Some(protected)
    }

    pub(super) fn unprotect_raw(protected: &[u8]) -> Option<Vec<u8>> {
        let input = DataBlob {
            cb_data: protected.len() as u32,
            pb_data: protected.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 || output.pb_data.is_null() {
            return None;
        }
        let plain =
            unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe { LocalFree(output.pb_data as isize) };
        Some(plain)
    }
}

/// Whether a stored value already carries the protection envelope.
pub fn is_envelope(value: &str) -> bool {
    value.starts_with(ENVELOPE_PREFIX)
}

/// Encrypts `plain` under the current Windows user. Returns `None` when
/// protection fails or on non-Windows platforms; callers must then fall back
/// to storing the value unchanged.
pub fn protect(plain: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let protected = windows::protect_raw(plain.as_bytes())?;
        Some(format!("{}{}", ENVELOPE_PREFIX, hex::encode(protected)))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = plain;
        None
    }
}

/// Decrypts an envelope produced by [`protect`]. Returns `None` for values
/// without the envelope prefix (legacy plaintext) or on decryption failure.
pub fn unprotect(stored: &str) -> Option<String> {
    let encoded = stored.strip_prefix(ENVELOPE_PREFIX)?;
    #[cfg(target_os = "windows")]
    {
        let bytes = hex::decode(encoded).ok()?;
        let plain = windows::unprotect_raw(&bytes)?;
        String::from_utf8(plain).ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = encoded;
        None
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn protect_round_trips_and_marks_the_envelope() {
        let secret = "wJalrXUtnFEMI";
        let stored = protect(secret).expect("DPAPI should be available on Windows");
        assert!(stored.starts_with(ENVELOPE_PREFIX));
        assert!(!stored.contains(secret));
        assert_eq!(unprotect(&stored).as_deref(), Some(secret));
    }

    #[test]
    fn unprotect_rejects_non_envelope_values() {
        assert!(unprotect("plain-secret").is_none());
        assert!(unprotect("dpapi1:not-hex").is_none());
    }
}
