use super::{Database, StorageError};

impl Database {
    pub fn set_preview_path(
        &self,
        item_id: &str,
        preview_path: &str,
    ) -> Result<bool, StorageError> {
        self.with_connection(|connection| {
            let affected = connection.execute(
                "UPDATE clipboard_items SET preview_path = ?2 WHERE id = ?1",
                rusqlite::params![item_id, preview_path],
            )?;
            Ok(affected > 0)
        })
    }

    pub fn repair(&self) -> Result<RepairResult, StorageError> {
        self.with_connection(|connection| {
            let integrity: String =
                connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;

            let is_ok = integrity == "ok";
            if !is_ok {
                let _ = connection.execute("PRAGMA quick_check", []);
            }

            let page_count: i64 =
                connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let freelist_count: i64 =
                connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;

            Ok(RepairResult {
                integrity_ok: is_ok,
                integrity_message: integrity,
                page_count,
                freelist_count,
            })
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub integrity_ok: bool,
    pub integrity_message: String,
    pub page_count: i64,
    pub freelist_count: i64,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use crate::domain::{ClipboardItem, ClipboardKind};

    use super::Database;
    use crate::storage::ClipboardRepository;

    fn text_item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("record-{id}"),
            text_content: Some(format!("content-{id}")),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            size_bytes: 12,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-pool-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn set_preview_path_updates_database() {
        let db_path = temporary_path("preview-path");
        let database = Database::open(&db_path).unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        assert!(database
            .set_preview_path("item", "previews/thumb.jpg")
            .unwrap());
        let stored = database.get_item("item").unwrap().unwrap();
        assert_eq!(stored.preview_path.unwrap(), "previews/thumb.jpg");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn repair_checks_integrity() {
        let db_path = temporary_path("repair");
        let database = Database::open(&db_path).unwrap();
        database.save_item(&text_item("item", 100)).unwrap();

        let result = database.repair().unwrap();
        assert!(result.integrity_ok);

        let _ = std::fs::remove_file(&db_path);
    }
}
