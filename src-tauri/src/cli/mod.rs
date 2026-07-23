use serde::{Deserialize, Serialize};

use crate::domain::ClipboardItem;
use crate::storage::{ClipboardRepository, Database};

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
}

pub fn run_cli_command(args: &CliArgs, database: &Database) -> Result<String, String> {
    match args.command {
        CliCommand::List => {
            let items = database
                .list_recent(
                    args.limit.unwrap_or(20) as u32,
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
            let items = database
                .list_recent(args.limit.unwrap_or(50) as u32, 0)
                .map_err(|e| e.to_string())?;
            let filtered: Vec<&ClipboardItem> = items
                .iter()
                .filter(|item| {
                    item.title.to_lowercase().contains(&query.to_lowercase())
                        || item
                            .text_content
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                })
                .collect();
            format_items_ref(&filtered)
        }
        CliCommand::Copy => {
            let _ = "TODO: copy to system clipboard".to_owned();
            Ok(String::new())
        }
        CliCommand::Paste => {
            let _ = "TODO: paste from system clipboard".to_owned();
            Ok(String::new())
        }
        CliCommand::Delete => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| "delete requires an item id".to_owned())?;
            database
                .delete_item(query)
                .map_err(|e| e.to_string())?;
            Ok(format!("deleted item: {query}"))
        }
        CliCommand::Export => {
            let _ = "TODO: export clipboard items".to_owned();
            Ok(String::new())
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

fn format_items_ref(items: &[&ClipboardItem]) -> Result<String, String> {
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

pub struct LocalApiServer {
    pub port: u16,
}

impl LocalApiServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub fn start(&self) -> Result<(), String> {
        println!(
            "Local API server starting on port {} (placeholder)",
            self.port
        );
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        println!("Local API server stopped (placeholder)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClipboardItem, ClipboardKind};

    #[test]
    fn parse_cli_args_from_serde() {
        let json = r#"{"command":"list","limit":10}"#;
        let args: CliArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.command, CliCommand::List);
        assert_eq!(args.limit, Some(10));
        assert!(args.query.is_none());
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
        };
        ClipboardRepository::save_item(&database, &item).unwrap();

        let args = CliArgs {
            command: CliCommand::Stats,
            query: None,
            limit: None,
        };
        let result = run_cli_command(&args, &database).unwrap();
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
        };
        ClipboardRepository::save_item(&database, &item).unwrap();

        let args = CliArgs {
            command: CliCommand::Delete,
            query: Some("to-delete".to_owned()),
            limit: None,
        };
        let result = run_cli_command(&args, &database).unwrap();
        assert!(result.contains("deleted item"));
        assert_eq!(database.item_count().unwrap(), 0);
    }
}
