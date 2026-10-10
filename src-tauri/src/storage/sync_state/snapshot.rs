//! Snapshot, outbox and exported-metadata loaders.

use super::*;

pub(super) fn load_snapshot_upsert_batch(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    after_item_id: Option<&str>,
    limit: usize,
) -> Result<MutationBatch, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| StorageError::ValueOutOfRange {
        field: "sync snapshot export batch size",
    })?;
    let mut statement = connection.prepare(
        "SELECT id, kind, title, text_content, html_content, rtf_content,
                resource_path, content_hash, source_app,
                icon_path, size_bytes, created_at_ms,
                is_favorite, metadata_json,
                COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
           FROM clipboard_items
          WHERE deleted = 0
            AND (?1 IS NULL OR id > ?1)
          ORDER BY id
          LIMIT ?2",
    )?;
    let stored_items = statement
        .query_map(
            params![after_item_id, limit],
            StoredReplicatedItem::from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let item_ids = stored_items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut resource_refs = load_sync_resource_ref_map(connection, remote_scope, Some(&item_ids))?;
    let mut upserts = Vec::with_capacity(stored_items.len());
    for stored in stored_items {
        let mut item = stored.into_wire()?;
        if let Some(references) = resource_refs.remove(&item.item.id) {
            restore_sync_resource_refs(&mut item, &references)?;
        }
        upserts.push(item);
    }
    Ok(MutationBatch {
        upserts,
        tombstones: Vec::new(),
    })
}

pub(super) fn load_snapshot_tombstone_batch(
    connection: &rusqlite::Connection,
    after_item_id: Option<&str>,
    limit: usize,
) -> Result<MutationBatch, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| StorageError::ValueOutOfRange {
        field: "sync snapshot export batch size",
    })?;
    let mut statement = connection.prepare(
        "SELECT item_id, kind, content_hash, deleted_at_ms,
                modified_at_ms, writer_device_id
           FROM sync_tombstones
          WHERE (?1 IS NULL OR item_id > ?1)
          ORDER BY item_id
          LIMIT ?2",
    )?;
    let stored_tombstones = statement
        .query_map(params![after_item_id, limit], StoredTombstone::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut tombstones = Vec::with_capacity(stored_tombstones.len());
    for stored in stored_tombstones {
        tombstones.push(stored.into_wire()?);
    }
    Ok(MutationBatch {
        upserts: Vec::new(),
        tombstones,
    })
}

pub(super) fn load_mutations(
    connection: &rusqlite::Connection,
    remote_scope: Option<&str>,
    item_ids: Option<&[String]>,
) -> Result<MutationBatch, StorageError> {
    let mut upserts = BTreeMap::<String, ReplicatedItem>::new();
    let mut tombstones = BTreeMap::<String, Tombstone>::new();
    if let Some(item_ids) = item_ids {
        for chunk in item_ids.chunks(LOOKUP_CHUNK_SIZE) {
            let placeholders = (1..=chunk.len())
                .map(|position| format!("?{position}"))
                .collect::<Vec<_>>()
                .join(", ");
            let item_sql = format!(
                "SELECT id, kind, title, text_content, html_content, rtf_content,
                        resource_path, content_hash, source_app,
                        icon_path, size_bytes, created_at_ms,
                        is_favorite, metadata_json,
                        COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
                   FROM clipboard_items
                  WHERE deleted = 0 AND id IN ({placeholders})"
            );
            let mut item_statement = connection.prepare(&item_sql)?;
            let stored_items = item_statement
                .query_map(
                    params_from_iter(chunk.iter()),
                    StoredReplicatedItem::from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let mut resource_refs = match remote_scope {
                Some(remote_scope) => {
                    load_sync_resource_ref_map(connection, remote_scope, Some(chunk))?
                }
                None => BTreeMap::new(),
            };
            for stored in stored_items {
                let mut item = stored.into_wire()?;
                if let Some(references) = resource_refs.remove(&item.item.id) {
                    restore_sync_resource_refs(&mut item, &references)?;
                }
                upserts.insert(item.item.id.clone(), item);
            }

            let tombstone_sql = format!(
                "SELECT item_id, kind, content_hash, deleted_at_ms,
                        modified_at_ms, writer_device_id
                   FROM sync_tombstones
                  WHERE item_id IN ({placeholders})"
            );
            let mut tombstone_statement = connection.prepare(&tombstone_sql)?;
            let stored_tombstones = tombstone_statement
                .query_map(params_from_iter(chunk.iter()), StoredTombstone::from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            for stored in stored_tombstones {
                let tombstone = stored.into_wire()?;
                tombstones.insert(tombstone.item_id.clone(), tombstone);
            }
        }
    } else {
        let mut item_statement = connection.prepare(
            "SELECT id, kind, title, text_content, html_content, rtf_content,
                    resource_path, content_hash, source_app,
                    icon_path, size_bytes, created_at_ms,
                    is_favorite, metadata_json,
                    COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
               FROM clipboard_items
              WHERE deleted = 0
              ORDER BY id",
        )?;
        let stored_items = item_statement
            .query_map([], StoredReplicatedItem::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut resource_refs = match remote_scope {
            Some(remote_scope) => load_sync_resource_ref_map(connection, remote_scope, None)?,
            None => BTreeMap::new(),
        };
        let mut snapshot_upserts = Vec::with_capacity(stored_items.len());
        for stored in stored_items {
            let mut item = stored.into_wire()?;
            if let Some(references) = resource_refs.remove(&item.item.id) {
                restore_sync_resource_refs(&mut item, &references)?;
            }
            snapshot_upserts.push(item);
        }

        let mut tombstone_statement = connection.prepare(
            "SELECT item_id, kind, content_hash, deleted_at_ms,
                    modified_at_ms, writer_device_id
               FROM sync_tombstones
              ORDER BY item_id",
        )?;
        let stored_tombstones = tombstone_statement
            .query_map([], StoredTombstone::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshot_tombstones = Vec::with_capacity(stored_tombstones.len());
        for stored in stored_tombstones {
            snapshot_tombstones.push(stored.into_wire()?);
        }
        return Ok(MutationBatch {
            upserts: snapshot_upserts,
            tombstones: snapshot_tombstones,
        });
    }

    for (item_id, tombstone) in &tombstones {
        if let Some(item) = upserts.get(item_id) {
            if tombstone.version >= item.version {
                upserts.remove(item_id);
            }
        }
    }
    tombstones.retain(|item_id, tombstone| {
        upserts
            .get(item_id)
            .is_none_or(|item| tombstone.version >= item.version)
    });

    Ok(MutationBatch {
        upserts: upserts.into_values().collect(),
        tombstones: tombstones.into_values().collect(),
    })
}

pub(super) fn load_sync_resource_ref_map(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    item_ids: Option<&[String]>,
) -> Result<BTreeMap<String, Vec<SyncResourceRef>>, StorageError> {
    let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
        return Ok(BTreeMap::new());
    };
    let (sql, values) = if let Some(item_ids) = item_ids {
        if item_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let placeholders = (2..=(item_ids.len() + 1))
            .map(|position| format!("?{position}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(item_ids.len() + 1);
        values.push(Box::new(scope_id));
        values.extend(
            item_ids
                .iter()
                .cloned()
                .map(|item_id| Box::new(item_id) as Box<dyn rusqlite::types::ToSql>),
        );
        (
            format!(
                "SELECT item_id, slot, ordinal, sha256, extension
                   FROM sync_item_resources
                  WHERE scope_id = ?1 AND item_id IN ({placeholders})
                  ORDER BY item_id, slot, ordinal"
            ),
            values,
        )
    } else {
        (
            "SELECT item_id, slot, ordinal, sha256, extension
               FROM sync_item_resources
              WHERE scope_id = ?1
              ORDER BY item_id, slot, ordinal"
                .to_string(),
            vec![Box::new(scope_id) as Box<dyn rusqlite::types::ToSql>],
        )
    };
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map(
            params_from_iter(values.iter().map(|value| value.as_ref())),
            |row| {
                let item_id = row.get::<_, String>(0)?;
                let slot = resource_slot_from_i64(row.get(1)?)?;
                let ordinal = row.get::<_, i64>(2)?;
                let sha256 = row.get::<_, Vec<u8>>(3)?;
                let extension = row.get::<_, String>(4)?;
                let reference = sync_resource_ref_from_parts(
                    slot,
                    u32::try_from(ordinal)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, ordinal))?,
                    &sha256,
                    &extension,
                )?;
                Ok((item_id, reference))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut references = BTreeMap::<String, Vec<SyncResourceRef>>::new();
    for (item_id, reference) in rows {
        references.entry(item_id).or_default().push(reference);
    }
    Ok(references)
}

pub(super) fn restore_sync_resource_refs(
    replicated: &mut ReplicatedItem,
    references: &[SyncResourceRef],
) -> Result<(), StorageError> {
    if references.is_empty() {
        return Ok(());
    }

    let mut file_paths = replicated
        .item
        .text_content
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok());
    let mut metadata = replicated
        .item
        .metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    for reference in references {
        let slot = reference.slot.as_str();
        let ordinal = i64::from(reference.ordinal);
        let object_key = &reference.object_key;
        let parsed = crate::sync::v1::parse_resource_key(object_key)
            .map_err(StorageError::InvalidSyncState)?;
        match slot {
            "image" if ordinal == 0 => {
                if replicated.item.kind != SyncItemKind::Image
                    || parsed.category != crate::sync::v1::ResourceCategory::Image
                {
                    return Err(StorageError::InvalidSyncState(
                        "image sync resource category does not match item kind".to_string(),
                    ));
                }
                replicated.item.resource_path = Some(object_key.clone());
                rewrite_exported_metadata_path(&mut metadata, "resourcePath", object_key);
                rewrite_exported_metadata_path(&mut metadata, "storagePath", object_key);
            }
            "file" => {
                if parsed.category != crate::sync::v1::ResourceCategory::File
                    || replicated.item.kind != SyncItemKind::File
                {
                    return Err(StorageError::InvalidSyncState(
                        "file sync resource category does not match item kind".to_string(),
                    ));
                }
                let index =
                    usize::try_from(ordinal).map_err(|_| StorageError::InvalidStoredValue {
                        field: "sync_item_resources.ordinal",
                        value: ordinal,
                    })?;
                let paths = file_paths.get_or_insert_with(Vec::new);
                if paths.len() <= index {
                    paths.resize(index + 1, String::new());
                }
                paths[index] = object_key.clone();
                if index == 0 {
                    replicated.item.resource_path = Some(object_key.clone());
                    rewrite_exported_metadata_path(&mut metadata, "resourcePath", object_key);
                }
                rewrite_exported_file_metadata_path(&mut metadata, index, object_key);
            }
            "icon" if ordinal == 0 => {
                if parsed.category != crate::sync::v1::ResourceCategory::Icon {
                    return Err(StorageError::InvalidSyncState(
                        "icon sync resource has a non-icon category".to_string(),
                    ));
                }
                replicated.item.icon_path = Some(object_key.clone());
            }
            _ => {
                return Err(StorageError::InvalidSyncState(
                    "sync item resource has an unknown slot".to_string(),
                ));
            }
        }
    }

    if let Some(paths) = file_paths {
        replicated.item.text_content = Some(serde_json::to_string(&paths).map_err(|error| {
            StorageError::InvalidSyncState(format!(
                "failed to encode restored file resource paths: {error}"
            ))
        })?);
    }
    if let Some(metadata) = metadata {
        replicated.item.metadata_json =
            Some(serde_json::to_string(&metadata).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "failed to encode restored resource metadata: {error}"
                ))
            })?);
    }
    Ok(())
}

pub(super) fn rewrite_exported_metadata_path(
    metadata: &mut Option<serde_json::Value>,
    key: &str,
    object_key: &str,
) {
    let Some(serde_json::Value::Object(object)) = metadata else {
        return;
    };
    object.insert(
        key.to_string(),
        serde_json::Value::String(object_key.to_string()),
    );
}

pub(super) fn rewrite_exported_file_metadata_path(
    metadata: &mut Option<serde_json::Value>,
    index: usize,
    object_key: &str,
) {
    let Some(serde_json::Value::Object(object)) = metadata else {
        return;
    };
    let Some(serde_json::Value::Array(files)) = object.get_mut("files") else {
        return;
    };
    let Some(serde_json::Value::Object(file)) = files.get_mut(index) else {
        return;
    };
    file.insert(
        "storagePath".to_string(),
        serde_json::Value::String(object_key.to_string()),
    );
}

pub(super) fn set_metadata_path(metadata: &mut serde_json::Value, key: &str, value: &str) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

pub(super) fn set_file_metadata_path(metadata: &mut serde_json::Value, index: usize, value: &str) {
    let Some(files) = metadata
        .get_mut("files")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    let Some(file) = files.get_mut(index).and_then(|value| value.as_object_mut()) else {
        return;
    };
    file.insert(
        "storagePath".to_string(),
        serde_json::Value::String(value.to_string()),
    );
    file.insert(
        "path".to_string(),
        serde_json::Value::String(value.to_string()),
    );
}
