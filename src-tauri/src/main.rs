// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;

use clipboard_desktop_lib::cli::{CliArgs, CliCommand};
use clipboard_desktop_lib::storage::Database;

const CLI_USAGE: &str = r#"Usage:
  clipboard-desktop                  Launch the desktop application
  clipboard-desktop list [--limit N]
  clipboard-desktop search --query TEXT [--limit N]
  clipboard-desktop copy --query ITEM_ID
  clipboard-desktop paste
  clipboard-desktop delete --query ITEM_ID
  clipboard-desktop export [--format json|csv|text] [--output-path PATH]
  clipboard-desktop stats

Options:
  --query TEXT         Search text or an item id for copy/delete
  --limit N            Maximum number of records to print
  --format FORMAT      Export format: json, csv, or text
  --output-path PATH   Write export output to a file instead of stdout
  -h, --help           Show this help
  -V, --version        Show the application version
"#;

#[derive(Debug)]
enum ProcessAction {
    LaunchGui,
    ShowHelp,
    ShowVersion,
    RunCli(CliArgs),
}

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
        Ok(ProcessAction::LaunchGui) => clipboard_desktop_lib::run(),
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
    }
}

fn parse_process_args(args: &[String]) -> Result<ProcessAction, String> {
    let Some(command) = args.first() else {
        return Ok(ProcessAction::LaunchGui);
    };

    match command.as_str() {
        "-h" | "--help" => {
            if args.len() == 1 {
                Ok(ProcessAction::ShowHelp)
            } else {
                Err("--help does not accept additional arguments".to_owned())
            }
        }
        "-V" | "--version" => {
            if args.len() == 1 {
                Ok(ProcessAction::ShowVersion)
            } else {
                Err("--version does not accept additional arguments".to_owned())
            }
        }
        command => parse_cli_invocation(command, &args[1..]).map(ProcessAction::RunCli),
    }
}

fn parse_cli_invocation(command: &str, args: &[String]) -> Result<CliArgs, String> {
    let command = parse_command(command)?;
    let mut query = None;
    let mut limit = None;
    let mut format = None;
    let mut output_path = None;
    let mut positionals = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let token = &args[index];
        if token == "--" {
            positionals.extend(args[index + 1..].iter().cloned());
            break;
        }

        if let Some(option) = token.strip_prefix("--") {
            let (name, inline_value) = option
                .split_once('=')
                .map_or((option, None), |(name, value)| (name, Some(value)));
            let value = match inline_value {
                Some(value) => value.to_owned(),
                None => {
                    index += 1;
                    args.get(index)
                        .filter(|value| !value.starts_with('-'))
                        .cloned()
                        .ok_or_else(|| format!("option --{name} requires a value"))?
                }
            };

            match name {
                "query" => set_once(&mut query, value, "--query")?,
                "limit" => {
                    let parsed = value
                        .parse::<usize>()
                        .map_err(|_| "--limit must be a positive integer".to_owned())?;
                    if parsed == 0 {
                        return Err("--limit must be a positive integer".to_owned());
                    }
                    set_once(&mut limit, parsed, "--limit")?;
                }
                "format" => set_once(&mut format, value, "--format")?,
                "output-path" => set_once(&mut output_path, value, "--output-path")?,
                other => return Err(format!("unknown option --{other}")),
            }
        } else if token.starts_with('-') {
            return Err(format!("unknown option {token}"));
        } else {
            positionals.push(token.clone());
        }
        index += 1;
    }

    let positional = match positionals.as_slice() {
        [] => None,
        [value] => Some(value.clone()),
        _ => return Err("only one positional argument is supported".to_owned()),
    };

    let (query, format) = match command {
        CliCommand::List => {
            reject_unused(
                &query,
                "list does not accept --query or positional arguments",
            )?;
            reject_unused(
                &positional,
                "list does not accept --query or positional arguments",
            )?;
            reject_unused(
                &format,
                "list does not accept --format or positional arguments",
            )?;
            reject_unused(&output_path, "list does not accept --output-path")?;
            (None, None)
        }
        CliCommand::Search => {
            let query = merge_positional(query, positional, "query")?;
            let query = query.ok_or_else(|| "search requires --query TEXT".to_owned())?;
            reject_unused(&format, "search does not accept --format")?;
            reject_unused(&output_path, "search does not accept --output-path")?;
            (Some(query), None)
        }
        CliCommand::Copy | CliCommand::Delete => {
            let query = merge_positional(query, positional, "query")?;
            let query = query
                .ok_or_else(|| format!("{} requires --query ITEM_ID", command_name(&command)))?;
            reject_unused(
                &limit,
                &format!("{} does not accept --limit", command_name(&command)),
            )?;
            reject_unused(
                &format,
                &format!("{} does not accept --format", command_name(&command)),
            )?;
            reject_unused(
                &output_path,
                &format!("{} does not accept --output-path", command_name(&command)),
            )?;
            (Some(query), None)
        }
        CliCommand::Paste => {
            reject_unused(
                &query,
                "paste does not accept --query or positional arguments",
            )?;
            reject_unused(
                &positional,
                "paste does not accept --query or positional arguments",
            )?;
            reject_unused(&limit, "paste does not accept --limit")?;
            reject_unused(&format, "paste does not accept --format")?;
            reject_unused(&output_path, "paste does not accept --output-path")?;
            (None, None)
        }
        CliCommand::Export => {
            let format = merge_positional(format, positional, "format")?;
            reject_unused(&query, "export uses --format FORMAT")?;
            reject_unused(&limit, "export does not accept --limit")?;
            (None, format)
        }
        CliCommand::Stats => {
            reject_unused(
                &query,
                "stats does not accept --query or positional arguments",
            )?;
            reject_unused(
                &positional,
                "stats does not accept --query or positional arguments",
            )?;
            reject_unused(&limit, "stats does not accept --limit")?;
            reject_unused(&format, "stats does not accept --format")?;
            reject_unused(&output_path, "stats does not accept --output-path")?;
            (None, None)
        }
    };

    Ok(CliArgs {
        command,
        query,
        limit,
        format,
        output_path,
    })
}

fn parse_command(value: &str) -> Result<CliCommand, String> {
    match value.to_ascii_lowercase().as_str() {
        "list" => Ok(CliCommand::List),
        "search" => Ok(CliCommand::Search),
        "copy" => Ok(CliCommand::Copy),
        "paste" => Ok(CliCommand::Paste),
        "delete" => Ok(CliCommand::Delete),
        "export" => Ok(CliCommand::Export),
        "stats" => Ok(CliCommand::Stats),
        other => Err(format!("unknown command '{other}'")),
    }
}

fn command_name(command: &CliCommand) -> &'static str {
    match command {
        CliCommand::List => "list",
        CliCommand::Search => "search",
        CliCommand::Copy => "copy",
        CliCommand::Paste => "paste",
        CliCommand::Delete => "delete",
        CliCommand::Export => "export",
        CliCommand::Stats => "stats",
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("option {name} may only be specified once"));
    }
    *slot = Some(value);
    Ok(())
}

fn merge_positional<T>(
    option: Option<T>,
    positional: Option<T>,
    name: &str,
) -> Result<Option<T>, String> {
    match (option, positional) {
        (Some(_), Some(_)) => Err(format!(
            "use either --{name} or a positional argument, not both"
        )),
        (value, None) | (None, value) => Ok(value),
    }
}

fn reject_unused<T>(value: &Option<T>, message: &str) -> Result<(), String> {
    if value.is_some() {
        Err(message.to_owned())
    } else {
        Ok(())
    }
}

fn run_cli_process(args: &CliArgs) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("cannot locate executable: {error}"))?;
    let project_directory = executable
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_owned())?;
    let database_path = project_directory
        .join("storage")
        .join("database")
        .join("clipboard.sqlite3");
    let database = Database::open(&database_path).map_err(|error| error.to_string())?;
    let output = clipboard_desktop_lib::cli::run_cli_command(args, &database)?;
    if !output.is_empty() {
        print!("{output}");
        if !output.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn no_arguments_launches_the_gui() {
        assert!(matches!(
            parse_process_args(&[]).unwrap(),
            ProcessAction::LaunchGui
        ));
    }

    #[test]
    fn parses_search_options_and_positional_query() {
        let action = parse_process_args(&args(&["search", "--limit=12", "clipboard"])).unwrap();
        let ProcessAction::RunCli(parsed) = action else {
            panic!("expected CLI action");
        };
        assert_eq!(parsed.command, CliCommand::Search);
        assert_eq!(parsed.query.as_deref(), Some("clipboard"));
        assert_eq!(parsed.limit, Some(12));
    }

    #[test]
    fn parses_export_format_and_output_path() {
        let action = parse_process_args(&args(&[
            "export",
            "--format",
            "csv",
            "--output-path",
            "backup.csv",
        ]))
        .unwrap();
        let ProcessAction::RunCli(parsed) = action else {
            panic!("expected CLI action");
        };
        assert_eq!(parsed.command, CliCommand::Export);
        assert_eq!(parsed.format.as_deref(), Some("csv"));
        assert_eq!(parsed.output_path.as_deref(), Some("backup.csv"));
    }

    #[test]
    fn parses_all_supported_commands() {
        for (command, expected) in [
            ("list", CliCommand::List),
            ("paste", CliCommand::Paste),
            ("stats", CliCommand::Stats),
        ] {
            let ProcessAction::RunCli(parsed) = parse_process_args(&args(&[command])).unwrap()
            else {
                panic!("expected CLI action");
            };
            assert_eq!(parsed.command, expected);
        }
        for command in ["copy", "delete"] {
            let ProcessAction::RunCli(parsed) =
                parse_process_args(&args(&[command, "--query", "item-1"])).unwrap()
            else {
                panic!("expected CLI action");
            };
            assert_eq!(parsed.command, parse_command(command).unwrap());
            assert_eq!(parsed.query.as_deref(), Some("item-1"));
        }
    }

    #[test]
    fn rejects_missing_values_and_unknown_options() {
        assert!(parse_process_args(&args(&["search", "--query"])).is_err());
        assert!(parse_process_args(&args(&["list", "--unknown"])).is_err());
        assert!(parse_process_args(&args(&["list", "--limit", "0"])).is_err());
    }

    #[test]
    fn help_and_version_are_process_actions() {
        assert!(matches!(
            parse_process_args(&args(&["--help"])).unwrap(),
            ProcessAction::ShowHelp
        ));
        assert!(matches!(
            parse_process_args(&args(&["--version"])).unwrap(),
            ProcessAction::ShowVersion
        ));
    }
}
