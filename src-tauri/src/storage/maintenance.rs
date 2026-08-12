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
            let integrity_ok = integrity == "ok";
            if !integrity_ok {
                let _ = connection.execute("PRAGMA quick_check", []);
            }

            let page_count: i64 =
                connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let freelist_count: i64 =
                connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;

            Ok(RepairResult {
                integrity_ok,
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
    use crate::{
        domain::{ClipboardItem, ClipboardKind},
        storage::{ClipboardRepository, Database},
    };

    fn item() -> ClipboardItem {
        ClipboardItem {
            id: "item".to_string(),
            kind: ClipboardKind::Text,
            title: "item".to_string(),
            text_content: Some("content".to_string()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-item".to_string(),
            source_app: None,
            icon_path: None,
            size_bytes: 7,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn preview_update_and_integrity_check_use_the_current_database() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&item()).unwrap();

        assert!(database
            .set_preview_path("item", "previews/item.jpg")
            .unwrap());
        assert_eq!(
            database
                .get_item("item")
                .unwrap()
                .unwrap()
                .preview_path
                .as_deref(),
            Some("previews/item.jpg")
        );
        assert!(database.repair().unwrap().integrity_ok);
    }
}
