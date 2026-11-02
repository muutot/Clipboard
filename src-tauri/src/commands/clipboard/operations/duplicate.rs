//! Metadata-only duplication and save-as-new commands.

use super::*;

/// Create a metadata-only duplicate of a clipboard item.
///
/// The duplicate copies **metadata only** — `resource_path` and
/// `preview_path` still point at the original item's underlying file (image,
/// file copy, etc.). The shared file is safe: disk reclamation happens only
/// through reference-scanned orphan cleanup. A rename preserves the old file
/// for duplicates that read it before their references commit; known shared
/// files receive only a display-title change.
///
/// `id` and `content_hash` are namespaced with a random UUID suffix so
/// `UNIQUE (kind, content_hash)` admits a second row for the same content and
/// two rapid duplicates never collapse into one upsert.
#[tauri::command]
pub fn duplicate_clipboard_item(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    let item = duplicate_clipboard_item_record(database.inner(), &id)?;
    if let Err(error) = app.emit("clipboard-item-added", &item) {
        crate::log_error!("[clipboard] failed to emit duplicate: {error}");
    }
    Ok(item.id)
}

pub fn duplicate_clipboard_item_record(
    database: &Database,
    id: &str,
) -> Result<ClipboardItem, String> {
    duplicate_clipboard_item_record_with_save(database, id, |item| {
        database
            .save_item(item)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

fn duplicate_clipboard_item_record_with_save(
    database: &Database,
    id: &str,
    save: impl FnOnce(&ClipboardItem) -> Result<(), String>,
) -> Result<ClipboardItem, String> {
    let _resource_publication = database.begin_resource_write().map_err(|e| e.to_string())?;
    let items = database
        .get_items_by_ids(&[id.to_owned()])
        .map_err(|e| e.to_string())?;
    let mut item = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let unique = uuid::Uuid::new_v4().simple();
    let namespaced = format!("{}-{unique}", item.content_hash);
    item.id = namespaced.clone();
    item.content_hash = namespaced;
    item.created_at_ms = now_ms;
    item.last_used_at_ms = Some(now_ms);
    item.is_favorite = false;
    save(&item)?;
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_read_before_rename_retains_readable_resource_paths() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-duplicate-rename-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = StoragePaths::initialize(root.clone()).unwrap();
        let database = Database::open(&paths.database).unwrap();
        let renamer = Database::open(&paths.database).unwrap();
        let old = paths.files.join("old.txt");
        std::fs::write(&old, b"fixture").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(3600))
            .unwrap();
        let old_path = old.display().to_string();
        let source = ClipboardItem {
            id: "source".into(),
            kind: ClipboardKind::File,
            title: "old".into(),
            text_content: Some(serde_json::to_string(&vec![&old_path]).unwrap()),
            html_content: None,
            rtf_content: None,
            resource_path: Some(old_path.clone()),
            preview_path: Some(old_path.clone()),
            content_hash: "fixture".into(),
            source_app: None,
            size_bytes: 7,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: Some(
                serde_json::json!({"files": [{"storagePath": &old_path}]}).to_string(),
            ),
        };
        database.save_item(&source).unwrap();
        let duplicated = duplicate_clipboard_item_record_with_save(&database, &source.id, |item| {
            // The duplicate has already read the old paths, but has not yet
            // committed its new row. Another real connection finishes rename.
            let renamed = super::super::rename_item_record(
                &renamer,
                &paths,
                source.id.clone(),
                "new".into(),
            )?;
            assert_eq!(
                renamed.resource_path.as_deref(),
                paths.files.join("new.txt").to_str()
            );
            database
                .save_item(item)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        // A completed duplicate must stay readable even after later orphan GC.
        crate::commands::cleanup::cleanup_orphan_storage_files_with_grace(
            &renamer,
            &paths,
            std::time::Duration::ZERO,
        )
        .unwrap();
        let stored = database.get_item(&duplicated.id).unwrap().unwrap();
        assert_eq!(stored.resource_path.as_deref(), Some(old_path.as_str()));
        assert_eq!(
            std::fs::read(stored.resource_path.unwrap()).unwrap(),
            b"fixture"
        );
        assert_eq!(
            std::fs::read(paths.files.join("new.txt")).unwrap(),
            b"fixture"
        );
        drop(renamer);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn save_as_new_capture_interleave(kind: ClipboardKind, deleted: bool) {
        let root =
            std::env::temp_dir().join(format!("clipboard-save-new-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths::initialize(root.clone()).unwrap();
        let database = Database::open(&paths.database).unwrap();
        let capturing = Database::open(&paths.database).unwrap();
        let source = ClipboardItem {
            id: "source".into(),
            kind,
            title: "source".into(),
            text_content: Some("old fixture".into()),
            html_content: Some("<b>old fixture</b>".into()),
            rtf_content: Some("old rich text".into()),
            resource_path: None,
            preview_path: None,
            content_hash: "original-fixture".into(),
            source_app: Some("editor".into()),
            size_bytes: 11,
            created_at_ms: 1,
            last_used_at_ms: Some(1),
            is_favorite: false,
            icon_path: None,
            metadata_json: Some(serde_json::json!({"tags": ["editor-tag"]}).to_string()),
        };
        database.save_item(&source).unwrap();
        let canonical = content::hash::compute_content_hash(
            if kind == ClipboardKind::Text {
                "text"
            } else {
                "link"
            },
            "edited fixture",
            None,
        );
        let mut captured_before = None;
        let created = save_clipboard_item_as_new_record_with_save(
            &database,
            "source",
            "edited title",
            "edited fixture",
            |prepared| {
                // Another real connection captures the edited content after
                // command preparation but before its actual persistence.
                assert_eq!(prepared.content_hash, canonical);
                let mut capture = prepared.clone();
                capture.id = "concurrent-capture".into();
                capture.title = "capture title".into();
                capture.source_app = Some("capture-app".into());
                capture.metadata_json =
                    Some(serde_json::json!({"tags": ["capture-tag"]}).to_string());
                capturing.save_item(&capture).unwrap();
                if deleted {
                    capturing.soft_delete(&capture.id).unwrap();
                }
                captured_before = capturing.get_item(&capture.id).unwrap();
                persist_save_as_new_record(&database, prepared)
            },
        )
        .unwrap();
        let saved = database.get_item(&created.id).unwrap();
        let captured_after = capturing.get_item("concurrent-capture").unwrap();
        let deleted_ids: Vec<_> = database
            .list_deleted(10, 0)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect();
        let tags = database.list_all_tags().unwrap();
        drop(capturing);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            saved.is_some(),
            "save-as-new returned a nonexistent ID: {}",
            created.id
        );
        assert_ne!(created.content_hash, canonical);
        assert_eq!(saved.unwrap(), created);
        assert_eq!(
            captured_after, captured_before,
            "save-as-new rewrote the competing capture"
        );
        assert_eq!(
            deleted_ids.contains(&"concurrent-capture".to_owned()),
            deleted
        );
        assert!(created.html_content.is_none() && created.rtf_content.is_none());
        assert!(tags
            .iter()
            .any(|tag| tag.name == "editor-tag" && tag.count == 2));
        assert_eq!(
            tags.iter()
                .find(|tag| tag.name == "capture-tag")
                .map(|tag| tag.count),
            (!deleted).then_some(1)
        );
    }

    #[test]
    fn save_as_new_keeps_a_distinct_row_after_concurrent_text_capture() {
        save_as_new_capture_interleave(ClipboardKind::Text, false);
    }

    #[test]
    fn save_as_new_keeps_a_distinct_row_after_concurrent_link_capture() {
        save_as_new_capture_interleave(ClipboardKind::Link, false);
    }

    #[test]
    fn save_as_new_does_not_resurrect_a_concurrent_deleted_capture() {
        save_as_new_capture_interleave(ClipboardKind::Text, true);
    }
}

/// Insert a new text/link record from an edited copy of `id` in one step.
///
/// Replaces the UI's former `duplicate_clipboard_item` + `update_clipboard_text`
/// pair: a failure can no longer leave a half-created duplicate, and a content
/// hash that already exists (including unchanged text saved beside its source)
/// is UUID-namespaced instead of tripping `UNIQUE (kind, content_hash)`.
#[tauri::command]
pub fn save_clipboard_item_as_new(
    database: tauri::State<'_, Database>,
    app: tauri::AppHandle,
    id: String,
    new_title: String,
    new_text_content: String,
) -> Result<String, String> {
    let item =
        save_clipboard_item_as_new_record(database.inner(), &id, &new_title, &new_text_content)?;
    if let Err(error) = app.emit("clipboard-item-added", &item) {
        crate::log_error!("[clipboard] failed to emit save-as-new: {error}");
    }
    Ok(item.id)
}

pub fn save_clipboard_item_as_new_record(
    database: &Database,
    id: &str,
    new_title: &str,
    new_text_content: &str,
) -> Result<ClipboardItem, String> {
    save_clipboard_item_as_new_record_with_save(database, id, new_title, new_text_content, |item| {
        persist_save_as_new_record(database, item)
    })
}

fn save_clipboard_item_as_new_record_with_save(
    database: &Database,
    id: &str,
    new_title: &str,
    new_text_content: &str,
    save: impl FnOnce(ClipboardItem) -> Result<ClipboardItem, String>,
) -> Result<ClipboardItem, String> {
    crate::storage::validate_clipboard_text(new_title, Some(new_text_content), None, None)
        .map_err(|error| error.to_string())?;
    if new_text_content.trim().is_empty() {
        return Err("text content cannot be empty".to_owned());
    }
    let items = database
        .get_items_by_ids(&[id.to_owned()])
        .map_err(|e| e.to_string())?;
    let source = items
        .into_iter()
        .next()
        .ok_or_else(|| "item not found".to_string())?;
    if !matches!(source.kind, ClipboardKind::Text | ClipboardKind::Link) {
        return Err("only text and link items can be edited".to_owned());
    }
    let kind_name = match source.kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image | ClipboardKind::File => unreachable!(),
    };
    let custom_title =
        resolve_custom_title(new_title, new_text_content, source.metadata_json.as_deref());
    let metadata_json = set_custom_title_metadata(source.metadata_json.as_deref(), custom_title)?;
    let proposed_hash = content::hash::compute_content_hash(kind_name, new_text_content, None);
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut item = source;
    item.id = format!("{proposed_hash}-{unique}");
    item.title = new_title.to_owned();
    item.text_content = Some(new_text_content.to_owned());
    item.html_content = None;
    item.rtf_content = None;
    item.content_hash = proposed_hash;
    item.size_bytes = new_text_content.len() as u64;
    item.created_at_ms = now_ms;
    item.last_used_at_ms = Some(now_ms);
    item.is_favorite = false;
    item.metadata_json = Some(metadata_json);
    save(item)
}

fn persist_save_as_new_record(
    database: &Database,
    item: ClipboardItem,
) -> Result<ClipboardItem, String> {
    database.save_item_as_new(item).map_err(|e| e.to_string())
}
