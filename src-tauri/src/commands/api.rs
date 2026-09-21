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
fn load_or_create_api_token(project_directory: &Path) -> Result<String, String> {
    let token_path = project_directory.join("conf").join(API_TOKEN_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let token = existing.trim();
        if !token.is_empty() {
            restrict_api_token_permissions(&token_path);
            return Ok(token.to_owned());
        }
    }

    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&token_path, format!("{token}\n")).map_err(|error| error.to_string())?;
    restrict_api_token_permissions(&token_path);
    Ok(token)
}

/// Tightens the token file to owner-only on Unix. The token authorizes the
/// loopback API, so a world-readable file would let another local account
/// read or delete the clipboard history. Best-effort: a failure leaves the
/// previous permissions rather than blocking the API.
#[cfg(unix)]
fn restrict_api_token_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
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
                    crate::log_event!("[api] self-trigger lock poisoned; copy may re-capture");
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

#[cfg(all(test, unix))]
mod tests {
    use super::load_or_create_api_token;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn api_token_file_is_owner_only() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "clipboard-api-token-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&project).unwrap();

        load_or_create_api_token(&project).unwrap();

        let token_path = project.join("conf").join("api.token");
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        std::fs::remove_dir_all(project).unwrap();
    }
}
