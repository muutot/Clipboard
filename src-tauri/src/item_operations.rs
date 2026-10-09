//! Item actions shared by the desktop, CLI and loopback API.
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    storage::{ClipboardRepository, Database, StoragePaths},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum CopyError {
    ResourceMissing(String),
    Failed(String),
}
impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceMissing(message) => write!(f, "resource missing: {message}"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

#[derive(Clone, Default)]
pub struct CopyContext {
    pub paths: Option<StoragePaths>,
    pub self_trigger: Option<Arc<Mutex<crate::content::self_trigger::SelfTriggerGuard>>>,
}
impl CopyContext {
    pub fn write(&self, database: &Database, item: &ClipboardItem) -> Result<(), CopyError> {
        let platform = crate::platform::platform();
        match item.kind {
            ClipboardKind::Text | ClipboardKind::Link => {
                let text = item
                    .text_content
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .unwrap_or(&item.title);
                self.write_marked(text, || {
                    platform.write_clipboard_text_with_self_trigger(text)
                })
            }
            ClipboardKind::Image | ClipboardKind::File => {
                let paths = self.paths.as_ref().ok_or_else(|| {
                    CopyError::ResourceMissing("storage paths are not configured".into())
                })?;
                let files = resolve_clipboard_file_paths(item, paths, Some(database))?
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                self.write_marked(&files.join("\n"), || {
                    platform.write_clipboard_files_with_self_trigger(&files)
                })
            }
        }
    }
    fn write_marked(
        &self,
        text: &str,
        write: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), CopyError> {
        if let Some(guard) = &self.self_trigger {
            if let Ok(mut guard) = guard.lock() {
                guard.mark_clipboard_write(text);
            }
        }
        let result = write();
        if result.is_err() {
            if let Some(guard) = &self.self_trigger {
                if let Ok(mut guard) = guard.lock() {
                    guard.unmark_clipboard_write(text);
                }
            }
        }
        result.map_err(CopyError::Failed)
    }
}

/// The OS write decides copy success. A failed usage stamp is logged, and must
/// never turn an already successful clipboard write into a retryable failure.
pub fn copy_item_with(
    database: &Database,
    id: &str,
    write: impl FnOnce(&ClipboardItem) -> Result<(), CopyError>,
) -> Result<(ClipboardItem, bool), CopyError> {
    let mut item = database
        .get_item(id)
        .map_err(|e| CopyError::Failed(e.to_string()))?
        .ok_or_else(|| CopyError::Failed(format!("item not found: {id}")))?;
    write(&item)?;
    let updated = match record_usage(database, id) {
        Ok(updated) => updated,
        Err(error) => {
            crate::log_warn!("[copy] clipboard written but usage could not be saved: {error}");
            false
        }
    };
    if updated {
        if let Ok(Some(current)) = database.get_item(id) {
            item.last_used_at_ms = current.last_used_at_ms;
        }
    }
    Ok((item, updated))
}
pub fn record_usage(database: &Database, id: &str) -> Result<bool, String> {
    database.set_last_used(id).map_err(|e| e.to_string())
}
#[derive(Clone, Copy)]
pub enum MembershipAction {
    Delete,
    Restore,
    Remove,
}
pub fn change_membership(
    database: &Database,
    id: &str,
    action: MembershipAction,
) -> Result<bool, String> {
    match action {
        MembershipAction::Delete => database.soft_delete(id),
        MembershipAction::Restore => database.restore_deleted(id),
        MembershipAction::Remove => database.permanently_delete(id),
    }
    .map_err(|e| e.to_string())
}

/// Resolves the existing local files behind an image/file record. Image items
/// use the stored png; file items re-reference the still-existing original
/// file for each entry (so pasting keeps the original name) and fall back to
/// the managed storage copy / the record's resource path when the original is
/// gone. When `database` is given and the original for a managed copy is gone,
/// the freshest other record sharing that managed file is consulted so the
/// pasted name stays the one from the most recent copy of the same content.
/// Pass-through files that exceed the copy-size limit keep their original
/// absolute location and are accepted here: the paths originate from the
/// record, never from the webview.
/// Directory captures also remain references to the original directory; only
/// file-kind records may resolve directories (images must be regular files).
///
/// The error type doubles as the command's rejection payload, so "the files are
/// gone" reaches the frontend as a distinct kind rather than as prose.
pub(crate) fn resolve_clipboard_file_paths(
    item: &ClipboardItem,
    paths: &StoragePaths,
    database: Option<&Database>,
) -> Result<Vec<std::path::PathBuf>, CopyError> {
    let mut candidates = Vec::new();
    match item.kind {
        ClipboardKind::Image => {
            if let Some(resource) = item.resource_path.as_deref() {
                candidates.push(resource.to_owned());
            }
        }
        ClipboardKind::File => {
            if let Some(entries) = file_metadata_entries(&item.metadata_json) {
                for (storage, original) in entries {
                    // OS clipboard semantics: re-reference the original file so
                    // the pasted copy keeps its original name instead of the
                    // managed (hash-named) storage copy. The managed copy stays
                    // the fallback when the original no longer exists on disk;
                    // with a database, the latest copy of the same content then
                    // donates its recorded original name.
                    if let Some(original) = original {
                        if is_copyable_path(std::path::Path::new(&original), item.kind) {
                            candidates.push(original);
                            continue;
                        }
                    }
                    if let Some(storage) = storage {
                        if let Some(database) = database {
                            if let Some(inherited) =
                                latest_copied_original_path(database, &storage, &item.id)
                            {
                                candidates.push(inherited.to_string_lossy().to_string());
                                continue;
                            }
                        }
                        candidates.push(storage);
                    }
                }
            }
            if candidates.is_empty() {
                if let Some(resource) = item.resource_path.as_deref() {
                    candidates.push(resource.to_owned());
                }
            }
        }
        _ => {
            return Err(CopyError::Failed(
                "clipboard item is not an image or file".to_owned(),
            ));
        }
    }

    let mut resolved = Vec::new();
    for candidate in &candidates {
        let raw = std::path::Path::new(candidate);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            [&paths.images, &paths.files, &paths.storage]
                .iter()
                .map(|root| root.join(raw))
                .find(|candidate| is_copyable_path(candidate, item.kind))
                .unwrap_or_else(|| paths.storage.join(raw))
        };
        if is_copyable_path(&path, item.kind) {
            resolved.push(path);
        }
    }

    if resolved.is_empty() {
        return Err(CopyError::ResourceMissing(
            "clipboard item has no available files on disk".to_owned(),
        ));
    }
    Ok(resolved)
}

fn is_copyable_path(path: &std::path::Path, kind: ClipboardKind) -> bool {
    path.metadata().is_ok_and(|metadata| {
        metadata.is_file() || (kind == ClipboardKind::File && metadata.is_dir())
    })
}

/// Locates the original file recorded by the most recent copy of the same
/// managed file (excluding `exclude_id`) and returns its path only when it
/// still exists on disk. Content-storage dedup guarantees a matching record
/// holds the identical bytes, so its original path is the last name given to
/// this content by the user.
fn latest_copied_original_path(
    database: &Database,
    storage: &str,
    exclude_id: &str,
) -> Option<std::path::PathBuf> {
    let record = database
        .latest_file_record_referencing_storage(storage, exclude_id)
        .ok()??;
    let original = file_metadata_entries(&record.metadata_json)?
        .into_iter()
        .find_map(
            |(entry_storage, entry_original)| match (entry_storage, entry_original) {
                (Some(entry_storage), Some(entry_original))
                    if entry_storage.as_str() == storage =>
                {
                    Some(entry_original)
                }
                _ => None,
            },
        )?;
    let path = std::path::PathBuf::from(&original);
    path.is_file().then_some(path)
}

/// Reads the `(storagePath, originalPath)` pair (legacy `path` for storage) of
/// each entry in the `files` array of the record's resource metadata. The
/// original path is optional: it is absent when the source is not a file on
/// disk (for example an in-memory screenshot).
pub(crate) fn file_metadata_entries(
    metadata_json: &Option<String>,
) -> Option<Vec<(Option<String>, Option<String>)>> {
    let json = metadata_json.as_deref()?;
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let files = parsed.get("files")?.as_array()?;
    let mut entries = Vec::new();
    for file in files {
        let storage = file
            .get("storagePath")
            .or_else(|| file.get("path"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let original = file
            .get("originalPath")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if storage.is_some() || original.is_some() {
            entries.push((storage, original));
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Broad invalidation is also used for changes committed by independent CLI/API
/// connections; their writes cannot reach the GUI's in-process event emitters.
pub(crate) fn invalidate_desktop(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    if let Some(cache) = app.try_state::<crate::commands::clipboard::SearchResultCache>() {
        cache.clear();
    }
    if let Err(error) = app.emit(
        "clipboard-history-invalidated",
        crate::commands::clipboard::ClipboardHistoryInvalidated {
            deleted_ids: Vec::new(),
        },
    ) {
        crate::log_warn!("[items] unable to broadcast invalidation: {error}");
    }
}

pub(crate) struct ExternalChangeWorker {
    stop: std::sync::mpsc::Sender<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl ExternalChangeWorker {
    pub fn start(app: tauri::AppHandle) -> Result<Self, String> {
        use tauri::Manager;
        let mut last = app
            .state::<Database>()
            .data_version()
            .map_err(|e| e.to_string())?;
        let (stop, receiver) = std::sync::mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("external-item-changes".into())
            .spawn(move || {
                while matches!(
                    receiver.recv_timeout(std::time::Duration::from_millis(500)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ) {
                    match app.state::<Database>().data_version() {
                        Ok(version) if version != last => {
                            last = version;
                            invalidate_desktop(&app);
                        }
                        Ok(_) => {}
                        Err(error) => {
                            crate::log_warn!("[items] external change probe failed: {error}")
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
    pub fn stop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
impl Drop for ExternalChangeWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> ClipboardItem {
        crate::cli::build_text_clipboard_item(
            "full body".into(),
            ClipboardKind::Text,
            "test",
            "test",
        )
    }
    #[test]
    fn usage_follows_successful_write_and_metadata_failure_keeps_copy_successful() {
        let db = Database::open_in_memory().unwrap();
        let mut row = record();
        row.last_used_at_ms = Some(1);
        db.save_item(&row).unwrap();
        assert!(copy_item_with(&db, &row.id, |_| Err(CopyError::Failed("busy".into()))).is_err());
        assert_eq!(
            db.get_item(&row.id).unwrap().unwrap().last_used_at_ms,
            Some(1)
        );
        let (_, updated) = copy_item_with(&db, &row.id, |item| {
            assert_eq!(item.text_content.as_deref(), Some("full body"));
            assert_eq!(
                db.get_item(&row.id).unwrap().unwrap().last_used_at_ms,
                Some(1)
            );
            Ok(())
        })
        .unwrap();
        assert!(updated);
        db.with_connection(|conn| { conn.execute_batch("CREATE TRIGGER reject_usage BEFORE UPDATE OF last_used_at_ms ON clipboard_items BEGIN SELECT RAISE(ABORT, 'test failure'); END;")?; Ok(()) }).unwrap();
        assert!(!copy_item_with(&db, &row.id, |_| Ok(())).unwrap().1);
    }
    #[test]
    fn missing_media_never_copies_its_title_or_updates_usage() {
        let db = Database::open_in_memory().unwrap();
        for kind in [ClipboardKind::Image, ClipboardKind::File] {
            let mut row = record();
            row.id = format!("missing-{kind:?}");
            row.kind = kind;
            row.last_used_at_ms = Some(1);
            db.save_item(&row).unwrap();
            let context = CopyContext::default();
            assert!(matches!(
                copy_item_with(&db, &row.id, |item| context.write(&db, item)),
                Err(CopyError::ResourceMissing(_))
            ));
            assert_eq!(
                db.get_item(&row.id).unwrap().unwrap().last_used_at_ms,
                Some(1)
            );
        }
    }
    #[test]
    fn captured_directories_remain_copyable_without_becoming_image_resources() {
        let root = std::env::temp_dir().join(format!("clipboard-folders-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths::initialize(root.join("managed")).unwrap();
        let directory = root.join("original-folder");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("child.txt"), b"keep").unwrap();
        let inputs = vec![directory.to_string_lossy().into_owned()];
        let refs =
            crate::commands::capture::store_captured_file_references(&inputs, &paths.files, 1024);
        let mut row = record();
        row.kind = ClipboardKind::File;
        row.resource_path = Some(refs[0].storage_path.clone());
        row.metadata_json = Some(crate::commands::capture::captured_file_metadata(&refs));
        assert_eq!(
            resolve_clipboard_file_paths(&row, &paths, None).unwrap(),
            vec![directory.clone()]
        );
        row.metadata_json = None; // Legacy single-path records follow the same contract.
        assert_eq!(
            resolve_clipboard_file_paths(&row, &paths, None).unwrap(),
            vec![directory.clone()]
        );
        row.kind = ClipboardKind::Image;
        assert!(matches!(
            resolve_clipboard_file_paths(&row, &paths, None),
            Err(CopyError::ResourceMissing(_))
        ));
        assert_eq!(std::fs::read(directory.join("child.txt")).unwrap(), b"keep");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_write_removes_the_shared_capture_marker() {
        let guard = Arc::new(Mutex::new(
            crate::content::self_trigger::SelfTriggerGuard::new(),
        ));
        let context = CopyContext {
            paths: None,
            self_trigger: Some(guard.clone()),
        };
        assert!(context
            .write_marked("sample", || {
                assert!(guard
                    .lock()
                    .unwrap()
                    .is_text_write_self_triggered("text", "sample"));
                Err("busy".into())
            })
            .is_err());
        assert!(!guard
            .lock()
            .unwrap()
            .is_text_write_self_triggered("text", "sample"));
    }
    #[test]
    fn membership_actions_preserve_favorites_and_recycle_guards() {
        let db = Database::open_in_memory().unwrap();
        let mut row = record();
        row.is_favorite = true;
        db.save_item(&row).unwrap();
        assert!(change_membership(&db, &row.id, MembershipAction::Delete).is_err());
        assert!(!change_membership(&db, &row.id, MembershipAction::Remove).unwrap());
        db.set_favorite(&row.id, false).unwrap();
        assert!(change_membership(&db, &row.id, MembershipAction::Delete).unwrap());
        assert!(change_membership(&db, &row.id, MembershipAction::Restore).unwrap());
        assert!(!change_membership(&db, &row.id, MembershipAction::Remove).unwrap());
        assert!(change_membership(&db, &row.id, MembershipAction::Delete).unwrap());
        assert!(change_membership(&db, &row.id, MembershipAction::Remove).unwrap());
    }
    #[test]
    fn data_version_detects_external_usage_and_membership_without_outbox_events() {
        let dir = std::env::temp_dir().join(format!("clipboard-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::open(dir.join("history.sqlite")).unwrap();
        let cli = Database::open(dir.join("history.sqlite")).unwrap();
        let row = record();
        db.save_item(&row).unwrap();
        let before = db.data_version().unwrap();
        db.set_last_used(&row.id).unwrap();
        assert_eq!(
            db.data_version().unwrap(),
            before,
            "same-connection writes do not loop"
        );
        cli.set_last_used(&row.id).unwrap();
        let after = db.data_version().unwrap();
        assert_ne!(after, before);
        cli.soft_delete(&row.id).unwrap();
        assert_ne!(db.data_version().unwrap(), after);
        drop(cli);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
