use super::{StoredClipboardItem, ITEM_COLUMNS};
use crate::{
    domain::ClipboardItem,
    storage::{Database, StorageError},
};

impl Database {
    /// Visit active rows in one read transaction. Do not reenter this database in the visitor.
    /// Only the current row is materialized; the caller controls output buffering.
    pub fn visit_active_items(
        &self,
        mut visit: impl FnMut(ClipboardItem) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            {
                let mut statement = tx.prepare(&format!(
                    "SELECT {ITEM_COLUMNS} FROM clipboard_items
                    WHERE deleted = 0 ORDER BY last_used_at_ms DESC, id DESC"
                ))?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    visit(StoredClipboardItem::from_row(row)?.try_into()?)?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }
}
