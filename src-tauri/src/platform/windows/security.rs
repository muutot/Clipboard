//! Owned current-process user ACLs for private filesystem creation.
pub(crate) fn with_current_user_acl<T>(
    inheritance: u32,
    apply: impl FnOnce(*mut windows_sys::Win32::Security::ACL) -> std::io::Result<T>,
) -> std::io::Result<T> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS, SetEntriesInAclW, TRUSTEE_IS_SID,
        TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACL, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: this guard owns the handle returned by OpenProcessToken.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct OwnedAcl(*mut ACL);
    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            // SAFETY: SetEntriesInAclW allocates this ACL with LocalAlloc.
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }

    // SAFETY: all output buffers are initialized and sized as passed to the OS.
    // The SID remains inside aligned token storage until the ACL has copied it;
    // the ACL stays allocated through the supplied operation and is then released.
    unsafe {
        let mut handle = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let token = Token(handle);
        let mut required = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        if !(std::mem::size_of::<TOKEN_USER>() as u32..=64 * 1024).contains(&required) {
            return Err(std::io::Error::last_os_error());
        }
        // usize storage aligns TOKEN_USER and the embedded SID on both Windows ABIs.
        let mut storage = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: inheritance,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: user.User.Sid.cast(),
            },
        };
        let mut acl = null_mut();
        // Start with no old ACL: retaining it would preserve explicit broad grants.
        let status = SetEntriesInAclW(1, &entry, null(), &mut acl);
        if status != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        let acl = OwnedAcl(acl);
        apply(acl.0)
    }
}
