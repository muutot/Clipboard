use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Manager;

use crate::cli::{CliArgs, CliCommand, LocalApiServer};
use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::state::{CaptureState, SelfTriggerState};
use crate::storage::{Database, StoragePaths};

const API_TOKEN_FILE_NAME: &str = "api.token";

/// Loads the loopback API bearer token from `conf/api.token`, generating and
/// persisting a fresh random token on first use so external scripts can read
/// a stable credential instead of re-reading it after every app start.
///
/// The file deliberately holds the credential in the clear. It is a documented
/// integration point — `cli/api.rs` points a user at this exact path when a
/// request is rejected — so sealing it through the OS secret store would break
/// the scripts the token exists for. Confidentiality therefore rests entirely
/// on the file mode, which is why [`write_api_token`] creates the file with the
/// restrictive mode instead of narrowing it afterwards, and why a failed
/// narrowing is logged rather than swallowed.
fn load_or_create_api_token(project_directory: &Path) -> Result<String, String> {
    let token_path = project_directory.join("conf").join(API_TOKEN_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let token = existing.trim();
        if !token.is_empty() {
            // Re-assert the mode on every start: the project directory is
            // user-configurable, so the file can land on a volume that did not
            // honor the mode it was created with, or be replaced by a backup
            // restore that carried broader permissions.
            restrict_api_token_permissions(&token_path);
            return Ok(token.to_owned());
        }
    }

    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    write_api_token(&token_path, &token)?;
    Ok(token)
}

/// Persists `token`, creating the file owner-readable-only from the start.
///
/// A plain `fs::write` creates the file with the process umask and only then
/// narrows it, so there is a window in which the credential is readable by
/// every local account; on a volume that does not implement POSIX modes
/// (exFAT, some network shares) the later narrowing is a silent no-op and the
/// window never closes. Applying the mode at creation removes the window, and
/// the Windows std::fs path still sets a DACL after writing. Its creation-time
/// exposure window is separate from tightening an existing file's ACL.
fn write_api_token(path: &Path, token: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let contents = format!("{token}\n");

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.write_all(contents.as_bytes())
            .map_err(|error| error.to_string())?;
        // `create` only applies the mode to a file it actually creates, so an
        // existing file keeps whatever mode it had.
        restrict_api_token_permissions(path);
        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).map_err(|error| error.to_string())?;
        restrict_api_token_permissions(path);
        Ok(())
    }
}

/// Tightens the token file to its Unix owner / current Windows user. The token authorizes the loopback
/// API, so a world-readable file would let another local account read or
/// delete the clipboard history. Best-effort: a failure leaves the previous
/// permissions rather than blocking the API.
#[cfg(unix)]
fn restrict_api_token_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        // A volume without POSIX modes (exFAT, some network shares) makes this
        // a permanent no-op, so the token stays group/world readable. That is
        // the exact case a user needs to hear about, and it used to be
        // discarded by `let _ =`.
        crate::log_warn!(
            "[api] could not restrict {} to owner-only; another local account may be able to \
             read the local API token",
            path.display()
        );
        crate::log_error!("[api] set_permissions failed: {error}");
    }
}

#[cfg(target_os = "windows")]
fn restrict_api_token_permissions(path: &Path) {
    if let Err(error) = set_api_token_acl(path) {
        crate::log_warn!(
            "[api] could not restrict {} to the current user: {error}; another local account may be able to read the local API token",
            path.display()
        );
    }
}

#[cfg(target_os = "windows")]
fn set_api_token_acl(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE,
        SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
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

    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if name[..name.len() - 1].contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "token path contains NUL",
        ));
    }
    // SAFETY: all output buffers are initialized and sized as passed to the OS.
    // The SID remains inside aligned token storage until the ACL has copied it;
    // the ACL stays allocated through SetNamedSecurityInfoW and is then released.
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
            grfInheritance: NO_INHERITANCE,
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
        let status = SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl.0,
            null(),
        );
        if status != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
    }
    Ok(())
}

#[cfg(all(not(unix), not(target_os = "windows")))]
fn restrict_api_token_permissions(_path: &Path) {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    running: bool,
    port: u16,
}

/// Async: CLI dispatch performs blocking SQLite work (and `export` can write
/// large files), so it runs on the dedicated blocking pool instead of the
/// main thread.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn run_cli_command(
    app: tauri::AppHandle,
    command: String,
    query: Option<String>,
    limit: Option<usize>,
    format: Option<String>,
    output_path: Option<String>,
) -> Result<String, String> {
    let command = match command.as_str() {
        "list" => CliCommand::List,
        "search" => CliCommand::Search,
        "copy" => CliCommand::Copy,
        "paste" => CliCommand::Paste,
        "delete" => CliCommand::Delete,
        "export" => CliCommand::Export,
        "stats" => CliCommand::Stats,
        other => return Err(format!("unknown command: {other}")),
    };

    let args = CliArgs {
        command,
        query,
        limit,
        format,
        output_path,
    };

    tauri::async_runtime::spawn_blocking(move || {
        let paths = app.state::<StoragePaths>();
        let database = app.state::<Database>();
        let config = app.state::<Mutex<ConfigStore>>();
        let capture = app.state::<CaptureState>();
        let self_trigger = app.state::<SelfTriggerState>();

        let page_size_limit =
            lock_state(&config, "configuration lock is poisoned")?.page_size_limit();
        let search_page_size_limit =
            lock_state(&config, "configuration lock is poisoned")?.search_page_size_limit();

        let result = crate::cli::run_cli_command_with_context(
            &args,
            database.inner(),
            page_size_limit,
            search_page_size_limit,
            &crate::item_operations::CopyContext {
                paths: Some(paths.inner().clone()),
                self_trigger: Some(self_trigger.0.clone()),
            },
            // The paste command writes to the database directly instead of going
            // through the clipboard worker, so it must enforce the same privacy
            // rules itself.
            Some(capture.inner()),
        );
        if result.is_ok()
            && matches!(
                args.command,
                CliCommand::Copy | CliCommand::Paste | CliCommand::Delete
            )
        {
            crate::item_operations::invalidate_desktop(&app);
        }
        result
    })
    .await
    .map_err(|error| format!("cli task join failed: {error}"))?
}

fn write_api_port(project_directory: &Path, port: u16) -> Result<(), String> {
    let path = project_directory.join("conf").join("api.port");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, format!("{port}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub(crate) fn start_local_api_with(
    app: &tauri::AppHandle,
    self_trigger: &SelfTriggerState,
    api: &Mutex<LocalApiServer>,
    paths: &StoragePaths,
    config: &Mutex<ConfigStore>,
    capture: &CaptureState,
    port: Option<u16>,
) -> Result<LocalApiStatus, String> {
    let database = Arc::new(Database::open(&paths.database).map_err(|error| error.to_string())?);
    let mut api = lock_state(&api, "local API server lock is poisoned")?;
    if let Some(port) = port {
        api.set_port(port)?;
    }
    {
        let config = lock_state(&config, "configuration lock is poisoned")?;
        api.set_limits(config.page_size_limit(), config.search_page_size_limit());
    }
    api.set_copy_context(crate::item_operations::CopyContext {
        paths: Some(paths.clone()),
        self_trigger: Some(self_trigger.0.clone()),
    });
    // The paste endpoint writes to the database directly instead of going
    // through the clipboard worker, so it needs the live capture policy
    // (pause state, sensitive patterns, size ceiling) to gate itself.
    api.set_capture_state(capture.clone());
    let app = app.clone();
    api.set_change_callback(Arc::new(move || {
        crate::item_operations::invalidate_desktop(&app)
    }));
    api.set_token(load_or_create_api_token(&paths.project)?);
    let bound_port = api.start_with_database(database)?;
    write_api_port(&paths.project, bound_port)?;
    Ok(LocalApiStatus {
        running: true,
        port: bound_port,
    })
}

#[tauri::command]
pub fn start_local_api(
    app: tauri::AppHandle,
    self_trigger: tauri::State<'_, SelfTriggerState>,
    api: tauri::State<'_, Mutex<LocalApiServer>>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    capture: tauri::State<'_, CaptureState>,
    port: Option<u16>,
) -> Result<LocalApiStatus, String> {
    start_local_api_with(
        &app,
        self_trigger.inner(),
        api.inner(),
        paths.inner(),
        config.inner(),
        capture.inner(),
        port,
    )
}
#[tauri::command]
pub fn stop_local_api(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
) -> Result<LocalApiStatus, String> {
    let mut api = lock_state(&api, "local API server lock is poisoned")?;
    api.stop()?;
    Ok(LocalApiStatus {
        running: false,
        port: api.port,
    })
}

#[tauri::command]
pub fn get_local_api_status(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
) -> Result<LocalApiStatus, String> {
    let api = lock_state(&api, "local API server lock is poisoned")?;
    Ok(LocalApiStatus {
        running: api.is_running(),
        port: api.port,
    })
}

#[cfg(test)]
mod tests {
    use super::{load_or_create_api_token, write_api_token};

    use std::path::PathBuf;

    fn temp_project(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "clipboard-api-token-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&project).unwrap();
        project
    }

    /// The token is an integration point: `cli/api.rs` sends a user to this
    /// file when a request is rejected, and the file exists so external scripts
    /// can read a stable credential. Anything that made the stored text differ
    /// from the token the server expects would break both at once while the file
    /// still looked plausible.
    #[test]
    fn the_token_file_contains_exactly_the_issued_token() {
        let project = temp_project("contents");
        let token_path = project.join("conf").join("api.token");

        let issued = load_or_create_api_token(&project).unwrap();
        let stored = std::fs::read_to_string(&token_path).unwrap();

        assert_eq!(stored, format!("{issued}\n"));
        assert_eq!(issued.len(), 64, "expected 32 random bytes as hex");
        assert!(issued.chars().all(|c| c.is_ascii_hexdigit()));

        let _ = std::fs::remove_dir_all(project);
    }

    /// Restarting must not rotate the credential, or every client that cached
    /// it would start getting 401s with no way to tell that from a bad token.
    #[test]
    fn the_token_survives_a_restart() {
        let project = temp_project("stable");

        let first = load_or_create_api_token(&project).unwrap();
        let second = load_or_create_api_token(&project).unwrap();

        assert_eq!(first, second, "the token must not rotate on every start");

        let _ = std::fs::remove_dir_all(project);
    }

    /// An existing token is authoritative: a file left by an earlier install, or
    /// restored from a backup, keeps working instead of being replaced.
    #[test]
    fn an_existing_token_is_reused_rather_than_replaced() {
        let project = temp_project("existing");
        let token_path = project.join("conf").join("api.token");
        std::fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        std::fs::write(&token_path, "0123456789abcdef\n").unwrap();

        assert_eq!(
            load_or_create_api_token(&project).unwrap(),
            "0123456789abcdef"
        );

        let _ = std::fs::remove_dir_all(project);
    }

    /// An empty or whitespace-only file is not a usable credential, so it must
    /// be replaced rather than accepted — otherwise the API would authorize
    /// against an empty string.
    #[test]
    fn an_empty_token_file_is_replaced() {
        let project = temp_project("empty");
        let token_path = project.join("conf").join("api.token");
        std::fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        std::fs::write(&token_path, "   \n").unwrap();

        let issued = load_or_create_api_token(&project).unwrap();

        assert_eq!(issued.len(), 64);
        assert_eq!(
            std::fs::read_to_string(&token_path).unwrap(),
            format!("{issued}\n")
        );

        let _ = std::fs::remove_dir_all(project);
    }

    /// Rewriting an existing token must replace the previous value outright: a
    /// partial write would leave a truncated credential that authorizes nobody
    /// while still reading as a token to a script.
    #[test]
    fn rewriting_the_token_replaces_the_previous_value() {
        let project = temp_project("rewrite");
        let token_path = project.join("conf").join("api.token");

        write_api_token(&token_path, "first").unwrap();
        write_api_token(&token_path, "second").unwrap();

        assert_eq!(std::fs::read_to_string(&token_path).unwrap(), "second\n");

        let _ = std::fs::remove_dir_all(project);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_token_with_explicit_everyone_access_is_narrowed_on_load() {
        use std::process::Command;
        let project = temp_project("explicit-acl");
        let token_path = project.join("conf").join("api.token");
        std::fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        std::fs::write(&token_path, "synthetic-token\n").unwrap();
        let grant = Command::new("icacls")
            .arg(&token_path)
            .arg("/grant")
            .arg("*S-1-1-0:R")
            .output()
            .unwrap();
        assert!(
            grant.status.success(),
            "fixture must grant real explicit Everyone access"
        );
        let read_acl = |name: &str| {
            let saved = project.join(name);
            let outcome = Command::new("icacls")
                .arg(&token_path)
                .arg("/save")
                .arg(&saved)
                .arg("/q")
                .output()
                .unwrap();
            assert!(
                outcome.status.success(),
                "fixture must read the actual Windows DACL"
            );
            let bytes = std::fs::read(saved).unwrap();
            let words: Vec<_> = bytes
                .chunks_exact(2)
                .map(|word| u16::from_le_bytes([word[0], word[1]]))
                .collect();
            String::from_utf16(&words).unwrap()
        };
        let before = read_acl("before-acl.txt");
        assert!(
            before.contains(";;;WD)") || before.contains(";;;S-1-1-0)"),
            "fixture must contain an explicit world-readable ACE: {before}"
        );
        assert_eq!(
            load_or_create_api_token(&project).unwrap(),
            "synthetic-token"
        );
        let after = read_acl("after-acl.txt");
        assert!(
            !after.contains(";;;WD)") && !after.contains(";;;S-1-1-0)"),
            "loading must remove explicit Everyone access, not only inherited ACEs: {after}"
        );
        assert_eq!(
            after.matches("(A;").count(),
            1,
            "the replacement must contain one current-user grant"
        );
        assert!(
            after.contains("D:P"),
            "the ACL must remain protected from parent inheritance"
        );
        assert_eq!(
            std::fs::read_to_string(&token_path).unwrap(),
            "synthetic-token\n",
            "owner read and stable credential must remain available"
        );
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn token_acl_supports_unicode_paths_and_reports_native_failures() {
        let project = temp_project("权限-😀");
        let token_path = project.join("conf").join("api.token");
        write_api_token(&token_path, "synthetic-unicode-token").unwrap();
        super::set_api_token_acl(&token_path).unwrap();
        assert_eq!(
            load_or_create_api_token(&project).unwrap(),
            "synthetic-unicode-token"
        );
        let missing = project.join("missing.token");
        assert!(super::set_api_token_acl(&missing).is_err());
        assert!(!missing.exists());
        assert!(super::set_api_token_acl(std::path::Path::new("bad\0token")).is_err());
        assert_eq!(
            std::fs::read_to_string(&token_path).unwrap(),
            "synthetic-unicode-token\n"
        );
        std::fs::remove_dir_all(project).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn api_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let project = temp_project("mode");

        load_or_create_api_token(&project).unwrap();

        let token_path = project.join("conf").join("api.token");
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let _ = std::fs::remove_dir_all(project);
    }

    /// The project directory is user-configurable, so the token file can land
    /// on a volume that ignored the mode it was created with, or be replaced by
    /// a restore that carried broader permissions. Every start re-asserts the
    /// mode for exactly that reason.
    #[cfg(unix)]
    #[test]
    fn a_broadly_permissioned_token_file_is_narrowed_on_load() {
        use std::os::unix::fs::PermissionsExt;

        let project = temp_project("broad");
        let token_path = project.join("conf").join("api.token");
        std::fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        std::fs::write(&token_path, "0123456789abcdef\n").unwrap();
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        load_or_create_api_token(&project).unwrap();

        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a world-readable token must be re-narrowed");

        let _ = std::fs::remove_dir_all(project);
    }
}
