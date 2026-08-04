#![windows_subsystem = "windows"]

use std::ffi::OsString;
use std::path::PathBuf;

use clipboard_desktop_lib::cli::args::{parse_process_args, ProcessAction, CLI_USAGE};
use clipboard_desktop_lib::cli::CliArgs;
use clipboard_desktop_lib::storage::{Database, StoragePaths};

fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let args = match raw_args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| "arguments must be valid UTF-8")
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    };

    match parse_process_args(&args) {
        Ok(ProcessAction::LaunchGui) => {
            attach_hidden_console();
            clipboard_desktop_lib::run()
        }
        action => {
            ensure_cli_console();
            match action {
                Ok(ProcessAction::ShowHelp) => print!("{CLI_USAGE}"),
                Ok(ProcessAction::ShowVersion) => println!("{}", env!("CARGO_PKG_VERSION")),
                Ok(ProcessAction::RunCli(cli_args)) => {
                    if let Err(error) = run_cli_process(&cli_args) {
                        eprintln!("error: {error}");
                        std::process::exit(1);
                    }
                }
                Err(error) => {
                    eprintln!("error: {error}\n\n{CLI_USAGE}");
                    std::process::exit(2);
                }
                Ok(ProcessAction::LaunchGui) => unreachable!("GUI action handled above"),
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn attach_hidden_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn AllocConsole() -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetConsoleWindow() -> isize;
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    }

    const SW_HIDE: i32 = 0;

    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            AllocConsole();
        }
        let hwnd = GetConsoleWindow();
        if hwnd != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_hidden_console() {}

#[cfg(target_os = "windows")]
fn ensure_cli_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn AllocConsole() -> i32;
    }

    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn ensure_cli_console() {}

fn run_cli_process(args: &CliArgs) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("cannot locate executable: {error}"))?;
    let project_directory = executable
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_owned())?;
    let config = clipboard_desktop_lib::config::ConfigStore::load(project_directory)
        .map_err(|error| error.to_string())?;
    let paths = StoragePaths::initialize_with_resource_directories(
        project_directory.to_path_buf(),
        config.storage_directory().map(PathBuf::from),
        config.image_storage_path().map(PathBuf::from),
        config.file_storage_path().map(PathBuf::from),
    )
    .map_err(|error| error.to_string())?;
    let database = Database::open(&paths.database).map_err(|error| error.to_string())?;
    let page_size_limit = config.page_size_limit();
    let search_page_size_limit = config.search_page_size_limit();
    let output = clipboard_desktop_lib::cli::run_cli_command(
        args,
        &database,
        page_size_limit,
        search_page_size_limit,
    )?;
    if !output.is_empty() {
        print!("{output}");
        if !output.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}
