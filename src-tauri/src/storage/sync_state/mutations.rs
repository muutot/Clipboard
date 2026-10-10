//! Mutation-batch application against local rows.

use super::*;

pub(super) fn apply_mutations(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    mutations: &MutationBatch,
    resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
) -> Result<u64, StorageError> {
    let mut applied = 0u64;
    for replicated in &mutations.upserts {
        validate_record_version(&replicated.version)?;
        let kind = kind_to_storage(replicated.item.kind);
        let target_id = resolve_sync_item_id(
            transaction,
            &replicated.item.id,
            kind,
            &replicated.item.content_hash,
        )?
        .unwrap_or_else(|| replicated.item.id.clone());
        if winning_local_version(transaction, &target_id)?
            .is_some_and(|local| replicated.version <= local)
        {
            continue;
        }
        let size_bytes = i64::try_from(replicated.item.size_bytes).map_err(|_| {
            StorageError::ValueOutOfRange {
                field: "clipboard_items.size_bytes",
            }
        })?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM clipboard_items WHERE id = ?1)",
            [&target_id],
            |row| row.get::<_, bool>(0),
        )?;
        let refs = resource_refs
            .get(&replicated.item.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let old_refs = load_sync_resource_refs(transaction, remote_scope, &target_id)?;
        let content_slot = match replicated.item.kind {
            SyncItemKind::Image => "image",
            SyncItemKind::File => "file",
            SyncItemKind::Text | SyncItemKind::Link => "",
        };
        let preserve_local_resource = exists
            && !content_slot.is_empty()
            && resource_slot_matches(&old_refs, refs, content_slot)
            && replicated.item.resource_path.is_none();
        let preserve_local_icon = exists
            && resource_slot_matches(&old_refs, refs, "icon")
            && replicated.item.icon_path.is_none();
        let local_paths = exists.then(|| {
            transaction.query_row(
                "SELECT resource_path, preview_path, icon_path, text_content, metadata_json
                   FROM clipboard_items
                  WHERE id = ?1",
                [&target_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
        });
        let (local_resource, local_preview, local_icon, local_text, local_metadata) =
            match local_paths {
                Some(row) => row?,
                None => (None, None, None, None, None),
            };
        let resource_path = if preserve_local_resource {
            local_resource.as_ref()
        } else {
            replicated.item.resource_path.as_ref()
        };
        let preview_path = if preserve_local_resource {
            local_preview.as_ref()
        } else {
            replicated.item.preview_path.as_ref()
        };
        let icon_path = if preserve_local_icon {
            local_icon.as_ref()
        } else {
            replicated.item.icon_path.as_ref()
        };
        let text_content = if preserve_local_resource && replicated.item.kind == SyncItemKind::File
        {
            local_text.as_ref()
        } else {
            replicated.item.text_content.as_ref()
        };
        let metadata_json = if preserve_local_resource || preserve_local_icon {
            merge_local_resource_metadata(
                replicated.item.metadata_json.as_deref(),
                local_metadata.as_deref(),
                preserve_local_resource,
                preserve_local_icon,
            )?
        } else {
            replicated.item.metadata_json.clone()
        };
        if exists {
            transaction.execute(
                "UPDATE clipboard_items
                    SET kind = ?2,
                        title = ?3,
                        text_content = ?4,
                        html_content = ?5,
                        rtf_content = ?6,
                        resource_path = ?7,
                        preview_path = ?8,
                        content_hash = ?9,
                        source_app = ?10,
                        icon_path = ?11,
                        size_bytes = ?12,
                        created_at_ms = ?13,
                        is_favorite = ?14,
                        metadata_json = ?15,
                        deleted = 0,
                        deleted_at_ms = NULL,
                        modified_at_ms = ?16,
                        sync_writer_device_id = ?17
                  WHERE id = ?1",
                params![
                    &target_id,
                    kind,
                    &replicated.item.title,
                    text_content,
                    &replicated.item.html_content,
                    &replicated.item.rtf_content,
                    resource_path,
                    preview_path,
                    &replicated.item.content_hash,
                    &replicated.item.source_app,
                    icon_path,
                    size_bytes,
                    replicated.item.created_at_ms,
                    replicated.item.is_favorite,
                    &metadata_json,
                    replicated.version.modified_at_ms,
                    &replicated.version.writer_device_id,
                ],
            )?;
        } else {
            transaction.execute(
                "INSERT INTO clipboard_items
                    (id, kind, title, text_content, html_content, rtf_content,
                     resource_path, preview_path, content_hash, source_app,
                     icon_path, size_bytes, created_at_ms, last_used_at_ms,
                     is_favorite, metadata_json, deleted, deleted_at_ms,
                     modified_at_ms, sync_writer_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                         ?11, ?12, ?13, ?13, ?14, ?15, 0, NULL, ?16, ?17)",
                params![
                    &target_id,
                    kind,
                    &replicated.item.title,
                    &replicated.item.text_content,
                    &replicated.item.html_content,
                    &replicated.item.rtf_content,
                    &replicated.item.resource_path,
                    &replicated.item.preview_path,
                    &replicated.item.content_hash,
                    &replicated.item.source_app,
                    &replicated.item.icon_path,
                    size_bytes,
                    replicated.item.created_at_ms,
                    replicated.item.is_favorite,
                    &replicated.item.metadata_json,
                    replicated.version.modified_at_ms,
                    &replicated.version.writer_device_id,
                ],
            )?;
        }
        transaction.execute(
            "DELETE FROM sync_tombstones WHERE item_id IN (?1, ?2)",
            params![&target_id, &replicated.item.id],
        )?;
        replace_item_tags(transaction, &target_id, metadata_json.as_deref())?;
        replace_sync_resource_refs(transaction, remote_scope, &target_id, refs)?;
        applied += 1;
    }

    for tombstone in &mutations.tombstones {
        validate_record_version(&tombstone.version)?;
        let kind = kind_to_storage(tombstone.kind);
        let target_id = resolve_sync_item_id(
            transaction,
            &tombstone.item_id,
            kind,
            &tombstone.content_hash,
        )?
        .unwrap_or_else(|| tombstone.item_id.clone());
        if winning_local_version(transaction, &target_id)?
            .is_some_and(|local| tombstone.version <= local)
        {
            continue;
        }
        transaction.execute(
            "UPDATE clipboard_items
                SET deleted = 1,
                    deleted_at_ms = ?2,
                    modified_at_ms = ?3,
                    sync_writer_device_id = ?4
              WHERE id = ?1",
            params![
                &target_id,
                tombstone.deleted_at_ms,
                tombstone.version.modified_at_ms,
                &tombstone.version.writer_device_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO sync_tombstones
                (item_id, kind, content_hash, deleted_at_ms,
                 modified_at_ms, writer_device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(item_id) DO UPDATE SET
                kind = excluded.kind,
                content_hash = excluded.content_hash,
                deleted_at_ms = excluded.deleted_at_ms,
                modified_at_ms = excluded.modified_at_ms,
                writer_device_id = excluded.writer_device_id",
            params![
                &target_id,
                kind,
                &tombstone.content_hash,
                tombstone.deleted_at_ms,
                tombstone.version.modified_at_ms,
                &tombstone.version.writer_device_id,
            ],
        )?;
        if target_id != tombstone.item_id {
            transaction.execute(
                "DELETE FROM sync_tombstones WHERE item_id = ?1",
                [&tombstone.item_id],
            )?;
        }
        transaction.execute("DELETE FROM item_tags WHERE item_id = ?1", [&target_id])?;
        transaction.execute(
            "DELETE FROM sync_item_resources WHERE item_id = ?1",
            [&target_id],
        )?;
        applied += 1;
    }
    Ok(applied)
}
