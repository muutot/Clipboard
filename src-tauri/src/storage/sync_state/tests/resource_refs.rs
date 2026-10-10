//! Materialized resource paths, staleness and reuse on pull.

use super::*;

#[test]
fn materialized_resources_update_paths_without_replicating_the_cache_write() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let first_key = format!("v1/resources/file/sha256-{}.txt", "b".repeat(64));
    let second_key = format!("v1/resources/file/sha256-{}.txt", "c".repeat(64));
    let icon_key = format!("v1/resources/icon/sha256-{}.png", "d".repeat(64));
    let mut remote = replicated(
        "materialized-files",
        "hash-materialized-files",
        "materialized files",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.kind = ClipboardKind::File.into();
    remote.item.resource_path = None;
    remote.item.text_content = Some("[\"\",\"\"]".to_string());
    remote.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": null,
            "files": [{"storagePath": null}, {"storagePath": null}],
            "iconPath": null
        })
        .to_string(),
    );
    let refs = vec![
        SyncResourceRef {
            slot: "file".to_string(),
            ordinal: 0,
            object_key: first_key.clone(),
        },
        SyncResourceRef {
            slot: "file".to_string(),
            ordinal: 1,
            object_key: second_key.clone(),
        },
        SyncResourceRef {
            slot: "icon".to_string(),
            ordinal: 0,
            object_key: icon_key.clone(),
        },
    ];
    database
        .apply_sync_snapshot_with_resources(
            REMOTE_SCOPE,
            &cursor(0, None),
            &"a".repeat(64),
            &MutationBatch {
                upserts: vec![remote],
                tombstones: Vec::new(),
            },
            &BTreeMap::from([("materialized-files".to_string(), refs.clone())]),
        )
        .unwrap();
    let version_before = database
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT modified_at_ms, sync_writer_device_id
                       FROM clipboard_items WHERE id = 'materialized-files'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?)
        })
        .unwrap();

    let sep = std::path::MAIN_SEPARATOR_STR;
    let first_path = format!("{sep}cache{sep}first.txt");
    let second_path = format!("{sep}cache{sep}second.txt");
    let icon_materialized_path = format!("{sep}cache{sep}icons{sep}source.png");

    assert!(database
        .mark_sync_resources_materialized(
            REMOTE_SCOPE,
            "materialized-files",
            &[
                (refs[0].clone(), first_path.clone()),
                (refs[1].clone(), second_path.clone()),
                (refs[2].clone(), icon_materialized_path),
            ],
        )
        .unwrap());

    let stored = database.get_item("materialized-files").unwrap().unwrap();
    assert_eq!(stored.resource_path.as_deref(), Some(first_path.as_str()));
    assert_eq!(stored.icon_path.as_deref(), Some("source.png"));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(stored.text_content.as_deref().unwrap()).unwrap(),
        [first_path.clone(), second_path.clone()]
    );
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["files"][1]["storagePath"], second_path.as_str());
    assert_eq!(metadata["iconPath"], "source.png");
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(
        database
            .get_sync_resource_refs(REMOTE_SCOPE, "materialized-files")
            .unwrap(),
        refs
    );
    let version_after = database
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT modified_at_ms, sync_writer_device_id
                       FROM clipboard_items WHERE id = 'materialized-files'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(version_after, version_before);
    let exported = database
        .export_sync_snapshot_for_scope(REMOTE_SCOPE)
        .unwrap();
    let item = &exported.mutations.upserts[0].item;
    assert_eq!(item.resource_path.as_deref(), Some(first_key.as_str()));
    assert_eq!(item.icon_path.as_deref(), Some(icon_key.as_str()));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(item.text_content.as_deref().unwrap()).unwrap(),
        vec![first_key, second_key]
    );
}

#[test]
fn materialized_resource_write_is_atomic_when_any_reference_is_stale() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let object_key = format!("v1/resources/image/sha256-{}.png", "a".repeat(64));
    let reference = SyncResourceRef {
        slot: "image".to_string(),
        ordinal: 0,
        object_key: object_key.clone(),
    };
    let mut remote = replicated(
        "atomic-image",
        "hash-atomic-image",
        "atomic image",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.kind = ClipboardKind::Image.into();
    remote.item.text_content = None;
    remote.item.resource_path = None;
    database
        .apply_sync_snapshot_with_resources(
            REMOTE_SCOPE,
            &cursor(0, None),
            &"a".repeat(64),
            &MutationBatch {
                upserts: vec![remote],
                tombstones: Vec::new(),
            },
            &BTreeMap::from([("atomic-image".to_string(), vec![reference.clone()])]),
        )
        .unwrap();
    let stale = SyncResourceRef {
        object_key: format!("v1/resources/icon/sha256-{}.png", "b".repeat(64)),
        slot: "icon".to_string(),
        ordinal: 0,
    };

    assert!(!database
        .mark_sync_resources_materialized(
            REMOTE_SCOPE,
            "atomic-image",
            &[
                (reference, "C:\\cache\\image.png".to_string()),
                (stale, "C:\\cache\\icon.png".to_string()),
            ],
        )
        .unwrap());
    assert!(database
        .get_item("atomic-image")
        .unwrap()
        .unwrap()
        .resource_path
        .is_none());
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
}

#[test]
fn single_file_materialization_keeps_null_text_content_and_reuses_resource_path() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let object_key = format!("v1/resources/file/sha256-{}.txt", "e".repeat(64));
    let reference = SyncResourceRef {
        slot: "file".to_string(),
        ordinal: 0,
        object_key,
    };
    let mut remote = replicated(
        "single-file",
        "hash-single-file",
        "single file",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.kind = ClipboardKind::File.into();
    remote.item.resource_path = None;
    remote.item.text_content = None;
    remote.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": null,
            "files": [{"storagePath": null}]
        })
        .to_string(),
    );
    database
        .apply_sync_snapshot_with_resources(
            REMOTE_SCOPE,
            &cursor(0, None),
            &"f".repeat(64),
            &MutationBatch {
                upserts: vec![remote],
                tombstones: Vec::new(),
            },
            &BTreeMap::from([("single-file".to_string(), vec![reference.clone()])]),
        )
        .unwrap();

    let local_path = "C:\\cache\\single.txt";
    assert!(database
        .mark_sync_resource_materialized(REMOTE_SCOPE, "single-file", &reference, local_path,)
        .unwrap());
    let stored = database.get_item("single-file").unwrap().unwrap();
    assert_eq!(stored.resource_path.as_deref(), Some(local_path));
    assert!(stored.text_content.is_none());
    assert_eq!(
        database
            .materialized_sync_resource_path(REMOTE_SCOPE, "single-file", &reference)
            .unwrap()
            .as_deref(),
        Some(local_path)
    );
    assert!(!database
        .mark_sync_resource_materialized(REMOTE_SCOPE, "single-file", &reference, local_path,)
        .unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
}

#[test]
fn unchanged_remote_file_refs_preserve_materialized_local_paths() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let first_key = format!("v1/resources/file/sha256-{}.txt", "b".repeat(64));
    let second_key = format!("v1/resources/file/sha256-{}.txt", "c".repeat(64));
    let local_paths = vec!["C:\\cache\\first.txt", "C:\\cache\\second.txt"];
    let mut remote = replicated(
        "remote-files",
        "hash-remote-files",
        "remote files",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.kind = ClipboardKind::File.into();
    remote.item.resource_path = Some(local_paths[0].to_string());
    remote.item.text_content = Some(serde_json::to_string(&local_paths).unwrap());
    remote.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": local_paths[0],
            "files": [
                {"storagePath": local_paths[0]},
                {"storagePath": local_paths[1]}
            ]
        })
        .to_string(),
    );
    let refs = BTreeMap::from([(
        remote.item.id.clone(),
        vec![
            SyncResourceRef {
                slot: "file".to_string(),
                ordinal: 0,
                object_key: first_key.clone(),
            },
            SyncResourceRef {
                slot: "file".to_string(),
                ordinal: 1,
                object_key: second_key.clone(),
            },
        ],
    )]);
    database
        .apply_sync_snapshot_with_resources(
            REMOTE_SCOPE,
            &cursor(0, None),
            &"a".repeat(64),
            &MutationBatch {
                upserts: vec![remote.clone()],
                tombstones: Vec::new(),
            },
            &refs,
        )
        .unwrap();

    remote.version.modified_at_ms += 1;
    remote.item.title = "renamed remotely".to_string();
    remote.item.resource_path = None;
    remote.item.text_content = Some("[\"\",\"\"]".to_string());
    remote.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": null,
            "files": [{"storagePath": null}, {"storagePath": null}],
            "tags": ["remote"]
        })
        .to_string(),
    );
    database
        .apply_sync_segment_with_resources(
            REMOTE_SCOPE,
            &cursor(1, Some("segment-1")),
            &MutationBatch {
                upserts: vec![remote],
                tombstones: Vec::new(),
            },
            &refs,
        )
        .unwrap();

    let stored = database.get_item("remote-files").unwrap().unwrap();
    assert_eq!(stored.resource_path.as_deref(), Some(local_paths[0]));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(stored.text_content.as_deref().unwrap()).unwrap(),
        local_paths
    );
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["files"][1]["storagePath"], local_paths[1]);
    assert_eq!(metadata["tags"], serde_json::json!(["remote"]));
    let exported = database
        .export_sync_snapshot_for_scope(REMOTE_SCOPE)
        .unwrap();
    let item = &exported.mutations.upserts[0].item;
    assert_eq!(item.resource_path.as_deref(), Some(first_key.as_str()));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(item.text_content.as_deref().unwrap()).unwrap(),
        vec![first_key, second_key]
    );
}
