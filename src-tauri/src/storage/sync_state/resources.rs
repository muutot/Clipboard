//! `resources` sync persistence (moved verbatim from `sync_state.rs`).

use super::*;

impl Database {
    pub fn record_sync_resource_refs(
        &self,
        remote_scope: &str,
        mutations: &MutationBatch,
        resource_refs: &BTreeMap<String, Vec<SyncResourceRef>>,
    ) -> Result<(), StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for replicated in &mutations.upserts {
                let current = transaction
                    .query_row(
                        "SELECT COALESCE(modified_at_ms, created_at_ms), sync_writer_device_id
                           FROM clipboard_items
                          WHERE id = ?1 AND deleted = 0",
                        [&replicated.item.id],
                        |row| {
                            Ok(RecordVersion {
                                modified_at_ms: row.get(0)?,
                                writer_device_id: row.get(1)?,
                            })
                        },
                    )
                    .optional()?;
                if current.as_ref() == Some(&replicated.version) {
                    replace_sync_resource_refs(
                        &transaction,
                        remote_scope,
                        &replicated.item.id,
                        resource_refs
                            .get(&replicated.item.id)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                    )?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Returns the canonical remote resource references for one item in the
    /// configured scope. Local paths are deliberately not inferred here: a
    /// missing path means the caller must materialize the returned keys first.
    pub fn get_sync_resource_refs(
        &self,
        remote_scope: &str,
        item_id: &str,
    ) -> Result<Vec<SyncResourceRef>, StorageError> {
        validate_remote_scope(remote_scope)?;
        if item_id.is_empty() {
            return Ok(Vec::new());
        }
        self.with_connection(|connection| {
            load_sync_resource_refs(connection, remote_scope, item_id)
        })
    }

    /// Records a successfully materialized path while retaining the canonical
    /// reference. The reference is the stable content identity used to keep a
    /// later remote metadata update attached to the local cache; local path
    /// changes or deletion clear it through the resource trigger. This is a
    /// local-only cache update: it must not alter the replicated version or
    /// enqueue an outbox mutation.
    pub fn mark_sync_resource_materialized(
        &self,
        remote_scope: &str,
        item_id: &str,
        reference: &SyncResourceRef,
        local_path: &str,
    ) -> Result<bool, StorageError> {
        self.mark_sync_resources_materialized(
            remote_scope,
            item_id,
            &[(reference.clone(), local_path.to_string())],
        )
    }

    /// Atomically writes all paths materialized for one item while retaining
    /// their canonical remote references. This is deliberately a local cache
    /// update: replicated versions and the sync outbox remain unchanged.
    pub fn mark_sync_resources_materialized(
        &self,
        remote_scope: &str,
        item_id: &str,
        materialized: &[(SyncResourceRef, String)],
    ) -> Result<bool, StorageError> {
        validate_remote_scope(remote_scope)?;
        if item_id.is_empty() || materialized.is_empty() {
            return Ok(false);
        }
        for (reference, local_path) in materialized {
            if local_path.trim().is_empty() {
                return Ok(false);
            }
            crate::sync::v1::parse_resource_key(&reference.object_key)
                .map_err(StorageError::InvalidSyncState)?;
        }
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let Some(scope_id) = sync_resource_scope_id(&transaction, remote_scope)? else {
                transaction.commit()?;
                return Ok(false);
            };
            let Some(item_kind) = transaction
                .query_row(
                    "SELECT kind FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                    [item_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            else {
                transaction.commit()?;
                return Ok(false);
            };
            let mut stored_refs = Vec::with_capacity(materialized.len());
            for (reference, local_path) in materialized {
                let slot = resource_slot_to_i64(&reference.slot)?;
                let ordinal = sequence_to_i64(u64::from(reference.ordinal), "resource ordinal")?;
                let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
                    .map_err(StorageError::InvalidSyncState)?;
                let digest = hex::decode(parsed.sha256).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "sync resource digest is not hexadecimal: {error}"
                    ))
                })?;
                let stored: Option<()> = transaction
                    .query_row(
                        "SELECT 1
                           FROM sync_item_resources
                          WHERE scope_id = ?1 AND item_id = ?2 AND slot = ?3 AND ordinal = ?4
                            AND sha256 = ?5",
                        params![scope_id, item_id, slot, ordinal, digest],
                        |_row| Ok(()),
                    )
                    .optional()?;
                if stored.is_none() {
                    transaction.commit()?;
                    return Ok(false);
                }
                if (reference.slot == "image" && item_kind != "image")
                    || (reference.slot == "file" && item_kind != "file")
                    || (reference.slot != "image"
                        && reference.slot != "file"
                        && reference.slot != "icon")
                    || (reference.slot == "icon"
                        && parsed.category != crate::sync::v1::ResourceCategory::Icon)
                {
                    transaction.commit()?;
                    return Ok(false);
                }
                let stored_path = if reference.slot == "icon" {
                    std::path::Path::new(local_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| {
                            StorageError::InvalidSyncState(
                                "materialized icon path has no portable file name".to_string(),
                            )
                        })?
                        .to_string()
                } else {
                    local_path.clone()
                };
                stored_refs.push((reference.clone(), stored_path));
            }

            let (mut resource_path, mut icon_path, mut text_content, metadata_json): (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = transaction.query_row(
                "SELECT resource_path, icon_path, text_content, metadata_json
                   FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                [item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let mut metadata = metadata_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            let mut file_paths = if item_kind == "file" {
                match text_content.as_deref() {
                    Some(json) => {
                        Some(serde_json::from_str::<Vec<String>>(json).map_err(|error| {
                            StorageError::InvalidSyncState(format!(
                                "stored file resource paths are invalid JSON: {error}"
                            ))
                        })?)
                    }
                    None => None,
                }
            } else {
                None
            };
            let mut changed = false;
            set_changelog_suppressed(&transaction, true)?;
            for (reference, stored_path) in &stored_refs {
                match reference.slot.as_str() {
                    "image" => {
                        changed |= resource_path.as_deref() != Some(stored_path.as_str());
                        resource_path = Some(stored_path.clone());
                        set_metadata_path(&mut metadata, "resourcePath", stored_path);
                        set_metadata_path(&mut metadata, "storagePath", stored_path);
                    }
                    "file" => {
                        let index = usize::try_from(reference.ordinal).map_err(|_| {
                            StorageError::ValueOutOfRange {
                                field: "sync resource ordinal",
                            }
                        })?;
                        if index == 0 {
                            changed |= resource_path.as_deref() != Some(stored_path.as_str());
                            resource_path = Some(stored_path.clone());
                            set_metadata_path(&mut metadata, "resourcePath", stored_path);
                        }
                        if let Some(paths) = file_paths.as_mut() {
                            if paths.len() <= index {
                                paths.resize(index + 1, String::new());
                            }
                            changed |= paths[index] != *stored_path;
                            paths[index] = stored_path.clone();
                        } else if index > 0 {
                            set_changelog_suppressed(&transaction, false)?;
                            transaction.commit()?;
                            return Ok(false);
                        }
                        set_file_metadata_path(&mut metadata, index, stored_path);
                    }
                    "icon" => {
                        changed |= icon_path.as_deref() != Some(stored_path.as_str());
                        icon_path = Some(stored_path.clone());
                        set_metadata_path(&mut metadata, "iconPath", stored_path);
                    }
                    other => {
                        // Validation above rejects unknown slots, but return an
                        // error (rolling back) rather than panicking if a future
                        // path or stored row slips through.
                        return Err(StorageError::InvalidSyncState(format!(
                            "unknown materialized resource slot: {other}"
                        )));
                    }
                }
            }
            let encoded_metadata = serde_json::to_string(&metadata).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "failed to encode materialized resource metadata: {error}"
                ))
            })?;
            if let Some(paths) = file_paths {
                text_content = Some(serde_json::to_string(&paths).map_err(|error| {
                    StorageError::InvalidSyncState(format!(
                        "failed to encode materialized file paths: {error}"
                    ))
                })?);
            }
            if metadata_json.as_deref() != Some(encoded_metadata.as_str()) {
                changed = true;
            }
            if !changed {
                set_changelog_suppressed(&transaction, false)?;
                transaction.commit()?;
                return Ok(false);
            }
            transaction.execute(
                "UPDATE clipboard_items
                    SET resource_path = ?2,
                        text_content = ?3,
                        icon_path = ?4,
                        metadata_json = ?5
                  WHERE id = ?1 AND deleted = 0",
                params![
                    item_id,
                    resource_path,
                    text_content,
                    icon_path,
                    encoded_metadata
                ],
            )?;
            set_changelog_suppressed(&transaction, false)?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn materialized_sync_resource_path(
        &self,
        remote_scope: &str,
        item_id: &str,
        reference: &SyncResourceRef,
    ) -> Result<Option<String>, StorageError> {
        validate_remote_scope(remote_scope)?;
        self.with_connection(|connection| {
            let Some(scope_id) = sync_resource_scope_id(connection, remote_scope)? else {
                return Ok(None);
            };
            let slot = resource_slot_to_i64(&reference.slot)?;
            let ordinal = sequence_to_i64(u64::from(reference.ordinal), "resource ordinal")?;
            let parsed = crate::sync::v1::parse_resource_key(&reference.object_key)
                .map_err(StorageError::InvalidSyncState)?;
            let digest = hex::decode(parsed.sha256).map_err(|error| {
                StorageError::InvalidSyncState(format!(
                    "sync resource digest is not hexadecimal: {error}"
                ))
            })?;
            let exists: bool = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sync_item_resources
                     WHERE scope_id = ?1 AND item_id = ?2 AND slot = ?3 AND ordinal = ?4
                       AND sha256 = ?5
                )",
                params![scope_id, item_id, slot, ordinal, digest],
                |row| row.get(0),
            )?;
            if !exists {
                return Ok(None);
            }
            let (kind, resource_path, icon_path, text_content): (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = connection.query_row(
                "SELECT kind, resource_path, icon_path, text_content
                   FROM clipboard_items WHERE id = ?1 AND deleted = 0",
                [item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            match reference.slot.as_str() {
                "image" if kind == "image" && reference.ordinal == 0 => Ok(resource_path),
                "icon" if reference.ordinal == 0 => Ok(icon_path),
                "file" if kind == "file" => {
                    let index = usize::try_from(reference.ordinal).map_err(|_| {
                        StorageError::ValueOutOfRange {
                            field: "sync resource ordinal",
                        }
                    })?;
                    let listed_path = text_content
                        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
                        .and_then(|paths| paths.get(index).cloned())
                        .filter(|path| !path.is_empty());
                    Ok(listed_path.or_else(|| (index == 0).then_some(resource_path).flatten()))
                }
                _ => Ok(None),
            }
        })
    }
}
