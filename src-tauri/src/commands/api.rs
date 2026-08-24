use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::cli::{CliArgs, CliCommand, LocalApiServer};
use crate::config::ConfigStore;
use crate::storage::{Database, StoragePaths};

const API_TOKEN_FILE_NAME: &str = "api.token";

/// Loads the loopback API bearer token from `conf/api.token`, generating and
/// persisting a fresh random token on first use so external scripts can read
/// a stable credential instead of re-reading it after every app start.
fn load_or_create_api_token(project_directory: &Path) -> Result<String, String> {
    let token_path = project_directory.join("conf").join(API_TOKEN_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let token = existing.trim();
        if !token.is_empty() {
            return Ok(token.to_owned());
        }
    }

    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&token_path, format!("{token}\n")).map_err(|error| error.to_string())?;
    Ok(token)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiStatus {
    running: bool,
    port: u16,
}

#[tauri::command]
pub fn run_cli_command(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
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

    let page_size_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .page_size_limit();
    let search_page_size_limit = config
        .lock()
        .map_err(|_| "configuration lock is poisoned".to_owned())?
        .search_page_size_limit();

    crate::cli::run_cli_command(
        &args,
        database.inner(),
        page_size_limit,
        search_page_size_limit,
    )
}

#[tauri::command]
pub fn start_local_api(
    api: tauri::State<'_, Mutex<LocalApiServer>>,
    paths: tauri::State<'_, StoragePaths>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    port: Option<u16>,
) -> Result<LocalApiStatus, String> {
    let database = Arc::new(Database::open(&paths.database).map_err(|error| error.to_string())?);
    let mut api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    if let Some(port) = port {
        api.set_port(port)?;
    }
    {
        let config = config
            .lock()
            .map_err(|_| "configuration lock is poisoned".to_owned())?;
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
    let mut api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
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
    let api = api
        .lock()
        .map_err(|_| "local API server lock is poisoned".to_owned())?;
    Ok(LocalApiStatus {
        running: api.is_running(),
        port: api.port,
    })
}
