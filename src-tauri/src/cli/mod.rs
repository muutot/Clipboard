use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{ClipboardItem, ClipboardKind};
use crate::export::{export_database, ExportFormat, ExportOptions};
use crate::storage::{ClipboardRepository, Database};

mod api;
pub mod args;
pub use api::LocalApiServer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CliCommand {
    List,
    Search,
    Copy,
    Paste,
    Delete,
    Export,
    Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliArgs {
    pub command: CliCommand,
    pub query: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
}

pub fn run_cli_command(
    args: &CliArgs,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
) -> Result<String, String> {
    run_cli_command_with_clipboard(
        args,
        database,
        page_size_limit,
        search_page_size_limit,
        read_system_clipboard_text,
        write_system_clipboard_text,
    )
}

fn run_cli_command_with_clipboard<Read, Write>(
    args: &CliArgs,
    database: &Database,
    page_size_limit: u32,
    search_page_size_limit: u32,
    mut read_clipboard: Read,
    mut write_clipboard: Write,
) -> Result<String, String>
where
    Read: FnMut() -> Result<String, String>,
    Write: FnMut(&str) -> Result<(), String>,
{
    match args.command {
        CliCommand::List => {
            let items = database
                .list_recent(
                    args.limit.unwrap_or(20).clamp(1, page_size_limit as usize) as u32,
                    0,
                )
                .map_err(|e| e.to_string())?;
            format_items(&items)
        }
        CliCommand::Search => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| "search requires a query".to_owned())?;
            let wanted = args
                .limit
                .unwrap_or(50)
                .clamp(1, search_page_size_limit as usize);
            let items =
                search_items_by_scanning(database, query, wanted, page_size_limit as usize)?;
            format_items(&items)
        }
        CliCommand::Copy => {
            let id = args
                .query
                .as_deref()
                .ok_or_else(|| "copy requires an item id".to_owned())?;
            let items = database
                .get_items_by_ids(&[id.to_owned()])
                .map_err(|error| error.to_string())?;
            let item = items
                .into_iter()
                .next()
                .ok_or_else(|| format!("item not found: {id}"))?;
            let text = item
                .text_content
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or(&item.title);
            write_clipboard(text)?;
            Ok(format!("copied item: {}", item.id))
        }
        CliCommand::Paste => {
            let text = read_clipboard()?;
            if text.is_empty() {
                return Err("system clipboard does not contain text".to_owned());
            }
            let markers = crate::content::detect_markers(&text);
            let kind = if markers.is_link {
                ClipboardKind::Link
            } else {
                ClipboardKind::Text
            };
            let item = build_text_clipboard_item(text, kind, "cli", "CLI");
            let saved_id = database
                .save_item(&item)
                .map_err(|error| error.to_string())?;
            Ok(format!("saved clipboard item: {saved_id}"))
        }
        CliCommand::Delete => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| "delete requires an item id".to_owned())?;
            if !database.soft_delete(query).map_err(|e| e.to_string())? {
                return Err(format!("item not found: {query}"));
            }
            Ok(format!("deleted item: {query}"))
        }
        CliCommand::Export => {
            let format = parse_export_format(
                args.format
                    .as_deref()
                    .or(args.query.as_deref())
                    .unwrap_or("json"),
            )?;
            let output = export_database(
                database,
                &ExportOptions {
                    format,
                    include_favorites: true,
                    date_from_ms: None,
                    date_to_ms: None,
                    content_types: Vec::new(),
                },
            )?;
            if let Some(path) = args.output_path.as_deref() {
                write_export_file(path, &output)?;
                Ok(format!("exported clipboard items to: {path}"))
            } else {
                Ok(output)
            }
        }
        CliCommand::Stats => {
            let count = database.item_count().map_err(|e| e.to_string())?;
            Ok(format!("total clipboard items: {count}"))
        }
    }
}

fn format_items(items: &[ClipboardItem]) -> Result<String, String> {
    let out = items
        .iter()
        .map(|item| {
            format!(
                "[{}] {} - {}",
                item.id,
                item.title,
                item.text_content
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(out)
}

fn parse_export_format(format: &str) -> Result<ExportFormat, String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "json" => Ok(ExportFormat::Json),
        "csv" => Ok(ExportFormat::Csv),
        "text" | "txt" | "plain" | "plaintext" | "plain-text" => Ok(ExportFormat::PlainText),
        other => Err(format!("unknown export format: {other}")),
    }
}

fn write_export_file(path: &str, output: &str) -> Result<(), String> {
    let path = Path::new(path);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create export directory: {error}"))?;
    }
    std::fs::write(path, output).map_err(|error| format!("failed to write export: {error}"))
}

pub(crate) fn build_text_clipboard_item(
    text: String,
    kind: ClipboardKind,
    id_prefix: &str,
    source_app: &str,
) -> ClipboardItem {
    let kind_name = match kind {
        ClipboardKind::Link => "link",
        ClipboardKind::Text => "text",
        _ => "text",
    };
    let content_hash = crate::content::hash::compute_content_hash(kind_name, &text, None);
    let now_ms = current_time_ms();
    let size_bytes = text.len() as u64;
    let title = text.chars().take(200).collect();
    ClipboardItem {
        id: format!("{id_prefix}-{content_hash}-{now_ms}"),
        kind,
        title,
        text_content: Some(text),
        resource_path: None,
        preview_path: None,
        content_hash,
        source_app: Some(source_app.to_owned()),
        size_bytes,
        created_at_ms: now_ms,
        last_used_at_ms: None,
        is_favorite: false,
        icon_path: None,
        metadata_json: None,
    }
}

pub(crate) fn search_items_by_scanning(
    database: &Database,
    query: &str,
    limit: usize,
    scan_page_size: usize,
) -> Result<Vec<ClipboardItem>, String> {
    let normalized = query.to_lowercase();
    let mut offset = 0u32;
    let mut matches = Vec::new();
    loop {
        let page = database
            .list_recent(scan_page_size as u32, offset)
            .map_err(|e| e.to_string())?;
        let page_len = page.len();
        matches.extend(page.into_iter().filter(|item| {
            item.title.to_lowercase().contains(&normalized)
                || item
                    .text_content
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&normalized)
                || item
                    .source_app
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&normalized)
        }));
        if matches.len() >= limit || page_len < scan_page_size {
            break;
        }
        offset = offset.saturating_add(scan_page_size as u32);
    }
    matches.truncate(limit);
    Ok(matches)
}

pub(crate) fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(target_os = "windows")]
fn read_system_clipboard_text() -> Result<String, String> {
    crate::platform::windows_clipboard::read_clipboard_text()
        .ok_or_else(|| "system clipboard does not contain readable text".to_owned())
}

#[cfg(target_os = "windows")]
fn write_system_clipboard_text(text: &str) -> Result<(), String> {
    crate::platform::windows_clipboard::write_clipboard_text_with_self_trigger(text)
}

#[cfg(target_os = "macos")]
fn read_system_clipboard_text() -> Result<String, String> {
    read_clipboard_command("pbpaste", &[])
}

#[cfg(target_os = "macos")]
fn write_system_clipboard_text(text: &str) -> Result<(), String> {
    write_clipboard_command("pbcopy", &[], text)
}

#[cfg(target_os = "linux")]
fn read_system_clipboard_text() -> Result<String, String> {
    let commands: [(&str, &[&str]); 3] = [
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-out"]),
        ("xsel", &["--clipboard", "--output"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in commands {
        match read_clipboard_command(program, args) {
            Ok(text) => return Ok(text),
            Err(error) => errors.push(error),
        }
    }
    Err(format!(
        "no supported clipboard reader succeeded: {}",
        errors.join("; ")
    ))
}

#[cfg(target_os = "linux")]
fn write_system_clipboard_text(text: &str) -> Result<(), String> {
    let commands: [(&str, &[&str]); 3] = [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard", "-in"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in commands {
        match write_clipboard_command(program, args, text) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }
    Err(format!(
        "no supported clipboard writer succeeded: {}",
        errors.join("; ")
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_clipboard_command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{program} returned invalid UTF-8: {error}"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_clipboard_command(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| format!("{program}: stdin is unavailable"))?
        .write_all(text.as_bytes())
        .map_err(|error| format!("{program}: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClipboardItem, ClipboardKind};

    fn item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("title-{id}"),
            text_content: Some(format!("content-{id}")),
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            size_bytes: id.len() as u64,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    fn args(command: CliCommand) -> CliArgs {
        CliArgs {
            command,
            query: None,
            limit: None,
            format: None,
            output_path: None,
        }
    }

    #[test]
    fn parse_cli_args_from_serde() {
        let json = r#"{"command":"list","limit":10}"#;
        let args: CliArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.command, CliCommand::List);
        assert_eq!(args.limit, Some(10));
        assert!(args.query.is_none());
        assert!(args.format.is_none());
        assert!(args.output_path.is_none());
    }

    #[test]
    fn cli_stats_returns_item_count() {
        let database = Database::open_in_memory().unwrap();
        let item = ClipboardItem {
            id: "test".to_owned(),
            kind: ClipboardKind::Text,
            title: "test".to_owned(),
            text_content: Some("content".to_owned()),
            resource_path: None,
            preview_path: None,
            content_hash: "hash".to_owned(),
            source_app: None,
            size_bytes: 7,
            created_at_ms: 1000,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        };
        ClipboardRepository::save_item(&database, &item).unwrap();

        let args = args(CliCommand::Stats);
        let result = run_cli_command(&args, &database, 500, 500).unwrap();
        assert_eq!(result, "total clipboard items: 1");
    }

    #[test]
    fn cli_delete_removes_item() {
        let database = Database::open_in_memory().unwrap();
        let item = ClipboardItem {
            id: "to-delete".to_owned(),
            kind: ClipboardKind::Text,
            title: "delete me".to_owned(),
            text_content: Some("bye".to_owned()),
            resource_path: None,
            preview_path: None,
            content_hash: "hash".to_owned(),
            source_app: None,
            size_bytes: 3,
            created_at_ms: 1000,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        };
        ClipboardRepository::save_item(&database, &item).unwrap();

        let mut args = args(CliCommand::Delete);
        args.query = Some("to-delete".to_owned());
        let result = run_cli_command(&args, &database, 500, 500).unwrap();
        assert!(result.contains("deleted item"));
        assert_eq!(database.item_count().unwrap(), 0);
        assert_eq!(database.list_deleted(10, 0).unwrap().len(), 1);
    }

    #[test]
    fn cli_copy_requires_an_item_id() {
        let database = Database::open_in_memory().unwrap();
        let args = args(CliCommand::Copy);
        let result = run_cli_command_with_clipboard(
            &args,
            &database,
            500,
            500,
            || Ok(String::new()),
            |_| Ok(()),
        );
        assert_eq!(result.unwrap_err(), "copy requires an item id");
    }

    #[test]
    fn cli_copy_writes_the_selected_item() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&item("copy-me", 1)).unwrap();
        let mut args = args(CliCommand::Copy);
        args.query = Some("copy-me".to_owned());
        let mut copied = String::new();

        let result = run_cli_command_with_clipboard(
            &args,
            &database,
            500,
            500,
            || Ok(String::new()),
            |text| {
                copied = text.to_owned();
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(result, "copied item: copy-me");
        assert_eq!(copied, "content-copy-me");
    }

    #[test]
    fn cli_paste_persists_system_clipboard_text() {
        let database = Database::open_in_memory().unwrap();
        let args = args(CliCommand::Paste);

        let result = run_cli_command_with_clipboard(
            &args,
            &database,
            500,
            500,
            || Ok("https://example.com".to_owned()),
            |_| Ok(()),
        )
        .unwrap();

        assert!(result.starts_with("saved clipboard item: "));
        let saved = database.list_recent(10, 0).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].kind, ClipboardKind::Link);
        assert_eq!(
            saved[0].text_content.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(saved[0].source_app.as_deref(), Some("CLI"));
    }

    #[test]
    fn cli_search_scans_beyond_the_first_database_page() {
        let database = Database::open_in_memory().unwrap();
        for index in 0..501 {
            let mut record = item(&format!("item-{index}"), index);
            if index == 0 {
                record.source_app = Some("Needle App".to_owned());
            }
            database.save_item(&record).unwrap();
        }
        let mut args = args(CliCommand::Search);
        args.query = Some("needle app".to_owned());

        let result = run_cli_command(&args, &database, 500, 500).unwrap();

        assert!(result.contains("[item-0]"));
    }

    #[test]
    fn cli_export_includes_records_beyond_the_first_database_page() {
        let database = Database::open_in_memory().unwrap();
        for index in 0..501 {
            database
                .save_item(&item(&format!("export-{index}"), index))
                .unwrap();
        }
        let args = args(CliCommand::Export);

        let result = run_cli_command(&args, &database, 500, 500).unwrap();
        let exported: Vec<ClipboardItem> = serde_json::from_str(&result).unwrap();

        assert_eq!(exported.len(), 501);
        assert!(exported.iter().any(|item| item.id == "export-0"));
    }
}
