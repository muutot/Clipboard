//! Resource-reference persistence and local path merging.

use super::*;

pub(super) fn load_sync_resource_refs(
    connection: &rusqlite::Connection,
    remote_scope: &str,
    item_id: &str,
) -> Result<Vec<SyncResourceRef>, StorageError> {
    let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        "SELECT slot, ordinal, sha256, extension
           FROM sync_item_resources
          WHERE scope_id = ?1 AND item_id = ?2
          ORDER BY slot, ordinal",
    )?;
    let references = statement
        .query_map(params![scope_id, item_id], |row| {
            let ordinal = row.get::<_, i64>(1)?;
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, ordinal))?;
            let slot = resource_slot_from_i64(row.get(0)?)?;
            let sha256 = row.get::<_, Vec<u8>>(2)?;
            let extension = row.get::<_, String>(3)?;
            sync_resource_ref_from_parts(slot, ordinal, &sha256, &extension)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(references)
}

pub(super) fn resource_slot_matches(
    old_refs: &[SyncResourceRef],
    new_refs: &[SyncResourceRef],
    slot: &str,
) -> bool {
    let old = old_refs
        .iter()
        .filter(|reference| reference.slot == slot)
        .collect::<Vec<_>>();
    let new = new_refs
        .iter()
        .filter(|reference| reference.slot == slot)
        .collect::<Vec<_>>();
    old == new
}

pub(super) fn merge_local_resource_metadata(
    remote_json: Option<&str>,
    local_json: Option<&str>,
    preserve_resource: bool,
    preserve_icon: bool,
) -> Result<Option<String>, StorageError> {
    let mut remote = remote_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    let Some(local) =
        local_json.and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
    else {
        return Ok(remote_json.map(str::to_string));
    };
    if preserve_resource {
        copy_json_key(&local, &mut remote, "resourcePath");
        copy_json_key(&local, &mut remote, "storagePath");
        copy_json_key(&local, &mut remote, "previewPath");
        copy_file_storage_paths(&local, &mut remote);
    }
    if preserve_icon {
        copy_json_key(&local, &mut remote, "iconPath");
    }
    serde_json::to_string(&remote).map(Some).map_err(|error| {
        StorageError::InvalidSyncState(format!("failed to merge local resource metadata: {error}"))
    })
}

pub(super) fn copy_json_key(source: &serde_json::Value, target: &mut serde_json::Value, key: &str) {
    let (Some(source), Some(target)) = (source.as_object(), target.as_object_mut()) else {
        return;
    };
    match source.get(key) {
        Some(value) => {
            target.insert(key.to_string(), value.clone());
        }
        None => {
            target.remove(key);
        }
    }
}

pub(super) fn copy_file_storage_paths(source: &serde_json::Value, target: &mut serde_json::Value) {
    let Some(source_files) = source.get("files").and_then(serde_json::Value::as_array) else {
        return;
    };
    let Some(target_files) = target
        .get_mut("files")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (source, target) in source_files.iter().zip(target_files.iter_mut()) {
        copy_json_key(source, target, "storagePath");
        copy_json_key(source, target, "path");
    }
}

pub(super) fn replace_sync_resource_refs(
    transaction: &Transaction<'_>,
    remote_scope: &str,
    item_id: &str,
    references: &[SyncResourceRef],
) -> Result<(), StorageError> {
    let scope_id = get_or_create_sync_resource_scope(transaction, remote_scope)?;
    transaction.execute(
        "DELETE FROM sync_item_resources
          WHERE scope_id = ?1 AND item_id = ?2",
        params![scope_id, item_id],
    )?;
    for reference in references {
        let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
            .map_err(StorageError::InvalidSyncState)?;
        let slot_is_valid = match reference.slot.as_str() {
            "image" => {
                reference.ordinal == 0
                    && parsed.category == crate::sync::v1::ResourceCategory::Image
            }
            "file" => parsed.category == crate::sync::v1::ResourceCategory::File,
            "icon" => {
                reference.ordinal == 0 && parsed.category == crate::sync::v1::ResourceCategory::Icon
            }
            _ => false,
        };
        if !slot_is_valid {
            return Err(StorageError::InvalidSyncState(
                "sync resource reference has an invalid slot/category".to_string(),
            ));
        }
        transaction.execute(
            "INSERT INTO sync_item_resources
                (scope_id, item_id, slot, ordinal, sha256, extension)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                scope_id,
                item_id,
                resource_slot_to_i64(&reference.slot)?,
                i64::from(reference.ordinal),
                hex::decode(&parsed.sha256).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "sync resource digest is not hexadecimal: {error}"
                    ))
                })?,
                &parsed.extension,
            ],
        )?;
    }
    Ok(())
}

pub(super) fn sync_resource_scope_id(
    connection: &rusqlite::Connection,
    remote_scope: &str,
) -> Result<Option<i64>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT id FROM sync_resource_scopes WHERE remote_scope = ?1",
            [remote_scope],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn get_or_create_sync_resource_scope(
    transaction: &Transaction<'_>,
    remote_scope: &str,
) -> Result<i64, StorageError> {
    transaction.execute(
        "INSERT INTO sync_resource_scopes (remote_scope) VALUES (?1)
         ON CONFLICT(remote_scope) DO NOTHING",
        [remote_scope],
    )?;
    Ok(transaction.query_row(
        "SELECT id FROM sync_resource_scopes WHERE remote_scope = ?1",
        [remote_scope],
        |row| row.get(0),
    )?)
}

pub(super) fn resource_slot_to_i64(slot: &str) -> Result<i64, StorageError> {
    match slot {
        "image" => Ok(0),
        "file" => Ok(1),
        "icon" => Ok(2),
        _ => Err(StorageError::InvalidSyncState(
            "sync resource reference has an unknown slot".to_string(),
        )),
    }
}

pub(super) fn resource_slot_from_i64(slot: i64) -> rusqlite::Result<&'static str> {
    match slot {
        0 => Ok("image"),
        1 => Ok("file"),
        2 => Ok("icon"),
        _ => Err(rusqlite::Error::IntegralValueOutOfRange(0, slot)),
    }
}

pub(super) fn sync_resource_ref_from_parts(
    slot: &str,
    ordinal: u32,
    sha256: &[u8],
    extension: &str,
) -> rusqlite::Result<SyncResourceRef> {
    if sha256.len() != 32 {
        return Err(rusqlite::Error::InvalidColumnType(
            2,
            "sha256".to_string(),
            rusqlite::types::Type::Blob,
        ));
    }
    let category = match slot {
        "image" => crate::sync::v1::ResourceCategory::Image,
        "file" => crate::sync::v1::ResourceCategory::File,
        "icon" => crate::sync::v1::ResourceCategory::Icon,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let object_key =
        crate::sync::v1::resource_object_key(category, &hex::encode(sha256), extension)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(SyncResourceRef {
        slot: slot.to_string(),
        ordinal,
        object_key,
    })
}
