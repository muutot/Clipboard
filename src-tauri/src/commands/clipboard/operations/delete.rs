//! Removal and recovery: soft delete, clear, restore, permanent delete and
//! storage-kind purge.

use super::*;

#[tauri::command]
pub fn delete_clipboard_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    delete_clipboard_item_record(&database, &id, |payload| emit_items_changed(&app, payload))
}

pub(super) fn delete_clipboard_item_record(
    database: &Database,
    id: &str,
    notify: impl FnOnce(ClipboardItemsChanged),
) -> Result<bool, String> {
    let removed = database
        .delete_item(id)
        .map_err(|error| error.to_string())?;
    if removed {
        notify(ClipboardItemsChanged {
            removed_ids: vec![id.to_owned()],
            ..ClipboardItemsChanged::default()
        });
    }
    Ok(removed)
}

#[tauri::command]
pub fn batch_delete_clipboard_items(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    let updated = database
        .soft_delete_batch(&ids)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_membership_changed(&app, &ids, &[], &[]);
    }
    Ok(updated)
}

#[tauri::command]
pub fn soft_delete_clipboard_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    let updated = crate::item_operations::change_membership(
        &database,
        &id,
        crate::item_operations::MembershipAction::Delete,
    )?;
    if updated {
        broadcast_membership_changed(&app, std::slice::from_ref(&id), &[], &[]);
    }
    Ok(updated)
}

#[tauri::command]
pub fn clear_all_non_favorite_items(
    app: AppHandle,
    database: tauri::State<'_, Database>,
) -> Result<u64, String> {
    clear_non_favorite_records(&database, |payload| emit_items_changed(&app, payload))
}

pub(super) fn clear_non_favorite_records(
    database: &Database,
    notify: impl FnOnce(ClipboardItemsChanged),
) -> Result<u64, String> {
    let ids = database
        .clear_all_non_favorite_items()
        .map_err(|error| error.to_string())?;
    let count = ids.len() as u64;
    if count > 0 {
        notify(ClipboardItemsChanged {
            deleted_ids: ids,
            ..ClipboardItemsChanged::default()
        });
    }
    Ok(count)
}

#[tauri::command]
pub fn restore_clipboard_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    let updated = crate::item_operations::change_membership(
        &database,
        &id,
        crate::item_operations::MembershipAction::Restore,
    )?;
    if updated {
        broadcast_membership_changed(&app, &[], std::slice::from_ref(&id), &[]);
    }
    Ok(updated)
}

#[tauri::command]
pub fn list_deleted_clipboard_items(
    database: tauri::State<'_, Database>,
    config: tauri::State<'_, Mutex<ConfigStore>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ClipboardListItem>, String> {
    let max_limit = lock_state(&config, "configuration lock is poisoned")?.page_size_limit();
    database
        // Recycle-bin keyword filtering is still local; retain its complete bodies.
        .list_deleted(
            limit.unwrap_or(100).clamp(1, max_limit),
            offset.unwrap_or(0),
        )
        .map(|items| {
            items
                .into_iter()
                .map(|item| ClipboardListItem {
                    item,
                    content_loaded: true,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn batch_restore_clipboard_items(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    let updated = database
        .restore_deleted_batch(&ids)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_membership_changed(&app, &[], &ids, &[]);
    }
    Ok(updated)
}

#[tauri::command]
pub fn permanently_delete_clipboard_item(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    id: String,
) -> Result<bool, String> {
    let updated = crate::item_operations::change_membership(
        &database,
        &id,
        crate::item_operations::MembershipAction::Remove,
    )?;
    if updated {
        broadcast_membership_changed(&app, &[], &[], std::slice::from_ref(&id));
    }
    Ok(updated)
}

#[tauri::command]
pub fn batch_permanently_delete_clipboard_items(
    app: AppHandle,
    database: tauri::State<'_, Database>,
    ids: Vec<String>,
) -> Result<bool, String> {
    let updated = database
        .permanently_delete_batch(&ids)
        .map_err(|error| error.to_string())?;
    if updated {
        broadcast_membership_changed(&app, &[], &[], &ids);
    }
    Ok(updated)
}

#[tauri::command]
pub fn permanently_delete_storage_kind(
    database: tauri::State<'_, Database>,
    paths: tauri::State<'_, StoragePaths>,
    search_index: tauri::State<'_, Arc<SearchIndex>>,
    capture: tauri::State<'_, CaptureState>,
    app: tauri::AppHandle,
    kind: ClipboardKind,
    expected: StorageKindDeleteExpectation,
) -> Result<StorageKindDeleteResult, String> {
    let ingestion_guard = lock_state(
        &capture.ingestion_guard,
        "clipboard ingestion lock is poisoned",
    )?;
    let expected = KindStorageStats {
        item_count: expected.item_count,
        size_bytes: expected.size_bytes,
    };
    let mut result = permanently_delete_storage_kind_for(
        database.inner(),
        paths.inner(),
        search_index.inner(),
        kind,
        Some(expected),
    )?;
    drop(ingestion_guard);
    if result.deleted_count > 0 {
        if let Err(error) = app.emit(
            "clipboard-history-invalidated",
            ClipboardHistoryInvalidated {
                deleted_ids: result.deleted_ids.clone(),
            },
        ) {
            result
                .warnings
                .push(format!("main window refresh is pending: {error}"));
        }
    }
    Ok(result)
}
