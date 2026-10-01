use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::cli::{CliArgs, CliCommand, LocalApiServer};
use crate::commands::lock::lock_state;
use crate::config::ConfigStore;
use crate::state::SelfTriggerState;
use crate::storage::{ClipboardRepository, Database, StoragePaths};

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
/// the Windows branch still has to set a DACL afterwards because Windows has no
/// creation mode to set.
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
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).map_err(|error| error.to_string())?;
        restrict_api_token_permissions(path);
        Ok(())
    }
}

/// Tightens the token file to owner-only. The token authorizes the loopback
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
    use std::process::Command;

    // Best-effort owner-only ACL on Windows, where `set_permissions` cannot
    // express a DACL. `icacls` is a Windows built-in; the principal comes from
    // the environment so this stays dependency-free. A failure leaves the
    // inherited ACL, matching the Unix path's best-effort contract.
    let user = std::env::var("USERNAME").unwrap_or_default();
    if user.is_empty() {
        crate::log_warn!(
            "[api] USERNAME is unset, so no owner-only ACL was applied to {}",
            path.display()
        );
        return;
    }
    let principal = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    };
    // `icacls` can fail for reasons that leave the file readable by others —
    // a blocked process, a filesystem without ACL support, a non-elevated
    // shell against a protected file — and a non-zero status was previously
    // discarded, so a token left world-readable produced no trace at all.
    match Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:F"))
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => crate::log_warn!(
            "[api] icacls exited with {status} for {}; another local account may be able to read \
             the local API token",
            path.display()
        ),
        Err(error) => {
            crate::log_warn!(
                "[api] could not run icacls for {}: {error}; another local account may be able to \
                 read the local API token",
                path.display()
            );
        }
    }
}

#[cfg(all(not(unix), not(target_os = "windows")))]
fn restrict_api_token_permissions(_path: &Path) {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    running: bool,
    port: u16,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn run_cli_command(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    self_trigger: tauri::State<'_, SelfTriggerState>,
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

    // The in-process renderer path shares the capture thread's guard, so mark
    // the pending write before the CLI helper touches the system clipboard.
    // (A standalone CLI subprocess cannot reach this guard; on Windows its
    // OS-level marker still applies, on other platforms a re-capture there
    // remains a known limitation.)
    let mut marked_text: Option<String> = None;
    if command == CliCommand::Copy {
        if let Some(id) = query.as_deref() {
            if let Ok(Some(item)) = database.get_item(id) {
                let text = item
                    .text_content
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .unwrap_or(&item.title)
                    .to_owned();
                if let Ok(mut guard) = self_trigger.0.lock() {
                    guard.mark_clipboard_write(&text);
                    marked_text = Some(text);
                } else {
                    crate::log_error!("[api] self-trigger lock poisoned; copy may re-capture");
                }
            }
        }
    }

    let args = CliArgs {
        command,
        query,
        limit,
        format,
        output_path,
    };

    let page_size_limit = lock_state(&config, "configuration lock is poisoned")?.page_size_limit();
    let search_page_size_limit =
        lock_state(&config, "configuration lock is poisoned")?.search_page_size_limit();

    let result = crate::cli::run_cli_command(
        &args,
        database.inner(),
        page_size_limit,
        search_page_size_limit,
    );
    if result.is_err() {
        if let Some(text) = marked_text.as_deref() {
            if let Ok(mut guard) = self_trigger.0.lock() {
                guard.unmark_clipboard_write(text);
            }
        }
    }
    result
}

#[tauri::command]
pub fn start_local_api(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
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
    api.set_token(load_or_create_api_token(&paths.project)?);
    let bound_port = api.start_with_database(database)?;
    Ok(LocalApiStatus {
        running: true,
        port: bound_port,
    })
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
