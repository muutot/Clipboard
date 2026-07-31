use crate::domain::ClipboardItem;
use crate::domain::ClipboardKind;
use crate::storage::StorageError;

pub(super) const ITEM_COLUMNS: &str = "
    id,
    kind,
    title,
    text_content,
    html_content,
    resource_path,
    preview_path,
    content_hash,
    source_app,
    size_bytes,
    created_at_ms,
    last_used_at_ms,
    is_favorite,
    icon_path,
    metadata_json
";
pub(super) const ITEM_LOOKUP_CHUNK_SIZE: usize = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StorageFileReferences {
    pub resource_paths: Vec<String>,
    pub preview_paths: Vec<String>,
    pub icon_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindDeleteScope {
    pub include_favorites: bool,
    pub include_deleted: bool,
}

impl KindDeleteScope {
    /// Includes active records, favorites, and records already in the recycle bin.
    pub const fn all() -> Self {
        Self {
            include_favorites: true,
            include_deleted: true,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KindStorageStats {
    pub item_count: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KindDeleteResult {
    pub stats: KindStorageStats,
    pub deleted_ids: Vec<String>,
}

pub struct TextItemUpdate<'a> {
    pub id: &'a str,
    pub kind: ClipboardKind,
    pub title: &'a str,
    pub text_content: &'a str,
    pub content_hash: &'a str,
    pub size_bytes: u64,
    pub metadata_json: Option<&'a str>,
}

pub trait ClipboardRepository {
    fn save_item(&self, item: &ClipboardItem) -> Result<String, StorageError>;
    /// Atomically replaces the textual payload of an active text/link item.
    ///
    /// Keeping this operation in the repository ensures the content hash and
    /// byte count are updated together with the text.  SQLite's UNIQUE(kind,
    /// content_hash) constraint rejects an edit that would collide with a
    /// different history record without partially updating either row.
    fn update_text_item(&self, update: &TextItemUpdate<'_>) -> Result<bool, StorageError>;
    fn get_item(&self, id: &str) -> Result<Option<ClipboardItem>, StorageError>;
    fn get_items_by_ids(&self, ids: &[String]) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_recent(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError>;
    /// Lists soft-deleted records for the recycle-bin view.
    fn list_deleted(&self, limit: u32, offset: u32) -> Result<Vec<ClipboardItem>, StorageError>;
    fn list_source_applications(&self) -> Result<Vec<String>, StorageError>;
    fn list_source_applications_with_icons(
        &self,
    ) -> Result<Vec<(String, Option<String>)>, StorageError>;
    fn set_favorite(&self, id: &str, is_favorite: bool) -> Result<bool, StorageError>;
    /// Update the favorite flag for all requested records atomically.
    ///
    /// The operation returns `false` and makes no changes when any requested
    /// id does not exist. An empty id list is a no-op and also returns `false`.
    fn set_favorite_batch(&self, ids: &[String], is_favorite: bool) -> Result<bool, StorageError>;
    fn delete_item(&self, id: &str) -> Result<bool, StorageError>;
    fn item_count(&self) -> Result<u64, StorageError>;
    fn delete_older_than(&self, days: u32) -> Result<u64, StorageError>;
    fn enforce_capacity_limit(&self, max_items: u64) -> Result<u64, StorageError>;
    fn cleanup_orphan_search_index(&self) -> Result<u64, StorageError>;
    fn soft_delete(&self, id: &str) -> Result<bool, StorageError>;
    /// Soft-delete all requested active records atomically.
    ///
    /// A favorite active record aborts the whole operation with the same
    /// `FavoriteMustBeRemoved` error as the single-record API. Already deleted
    /// records are treated as idempotent; unknown ids return `false` without
    /// changing any record.
    fn soft_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    fn restore_deleted(&self, id: &str) -> Result<bool, StorageError>;
    /// Restores all requested soft-deleted records atomically.
    fn restore_deleted_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    /// Permanently removes one already soft-deleted record.
    fn permanently_delete(&self, id: &str) -> Result<bool, StorageError>;
    /// Permanently removes requested soft-deleted records atomically.
    fn permanently_delete_batch(&self, ids: &[String]) -> Result<bool, StorageError>;
    fn permanently_delete_expired(&self, days: u32) -> Result<u64, StorageError>;
    fn clear_all_non_favorite_items(&self) -> Result<u64, StorageError>;
    fn count_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    fn size_by_kind(&self, kind: &str) -> Result<u64, StorageError>;
    /// Returns the count and logical byte size for one kind and explicit scope.
    fn kind_storage_stats(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindStorageStats, StorageError>;
    /// Permanently deletes every record matching one kind and explicit scope.
    ///
    /// `include_favorites` controls whether favorite records may be removed and
    /// `include_deleted` controls whether records already in the recycle bin
    /// are included. `KindDeleteScope::all()` therefore deletes the complete
    /// category, including favorites and recycle-bin records. The returned
    /// statistics and sorted ids are derived from the rows actually deleted.
    ///
    /// SQLite's delete trigger queues a search-index delete for every removed
    /// row, while the OCR foreign key removes associated OCR data. Filesystem
    /// resources must subsequently be reclaimed by the ownership-aware orphan
    /// cleanup after this transaction commits.
    fn permanently_delete_by_kind(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
    ) -> Result<KindDeleteResult, StorageError>;
    /// Deletes a category only when its current statistics still match the
    /// values shown in the destructive confirmation dialog.
    fn permanently_delete_by_kind_if_stats_match(
        &self,
        kind: ClipboardKind,
        scope: KindDeleteScope,
        expected: KindStorageStats,
    ) -> Result<KindDeleteResult, StorageError>;
    /// Returns every filesystem reference still owned by a database record.
    ///
    /// Soft-deleted records remain recoverable, so their resources must stay
    /// referenced until the record is permanently removed.
    fn list_storage_file_references(&self) -> Result<StorageFileReferences, StorageError>;
}
