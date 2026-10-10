//! Device-local fields and resource-reference commit behavior.

use super::*;

#[test]
fn local_last_used_changes_never_enter_the_sync_outbox_or_wire() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mut local = item("local-usage", "hash-local-usage", "local usage");
    local.last_used_at_ms = Some(150);
    database.save_item(&local).unwrap();
    database.acknowledge_sync_outbox(1).unwrap();

    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE clipboard_items SET last_used_at_ms = 999 WHERE id = 'local-usage'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    let snapshot = database.export_sync_snapshot().unwrap();
    assert_eq!(snapshot.mutations.upserts.len(), 1);
    assert_eq!(snapshot.mutations.upserts[0].item.last_used_at_ms, None);
    assert_eq!(
        database
            .get_item("local-usage")
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        Some(999)
    );
}

#[test]
fn remote_last_used_value_is_ignored_on_insert_and_update() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mut remote = replicated(
        "remote-usage",
        "hash-remote-usage",
        "remote usage",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.created_at_ms = 100;
    remote.item.last_used_at_ms = Some(9_999);
    let first = MutationBatch {
        upserts: vec![remote.clone()],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(1, None), "a", &first)
        .unwrap();
    assert_eq!(
        database
            .get_item("remote-usage")
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        Some(100)
    );

    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE clipboard_items SET last_used_at_ms = 777 WHERE id = 'remote-usage'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    remote.item.title = "remote update".to_string();
    remote.item.last_used_at_ms = Some(8_888);
    remote.version.modified_at_ms = 501;
    let second = MutationBatch {
        upserts: vec![remote],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-2")), &second)
        .unwrap();
    let stored = database.get_item("remote-usage").unwrap().unwrap();
    assert_eq!(stored.title, "remote update");
    assert_eq!(stored.last_used_at_ms, Some(777));
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
}

#[test]
fn resource_refs_commit_with_the_cursor_and_survive_local_only_updates() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mut remote = replicated(
        "remote-image",
        "hash-remote-image",
        "remote image",
        RecordVersion {
            modified_at_ms: 500,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    remote.item.kind = ClipboardKind::Image.into();
    remote.item.text_content = None;
    remote.item.resource_path = None;
    remote.item.metadata_json = Some("{}".to_string());
    let object_key = format!("v1/resources/image/sha256-{}.png", "a".repeat(64));
    let refs = BTreeMap::from([(
        remote.item.id.clone(),
        vec![SyncResourceRef {
            slot: "image".to_string(),
            ordinal: 0,
            object_key: object_key.clone(),
        }],
    )]);
    database
        .apply_sync_snapshot_with_resources(
            REMOTE_SCOPE,
            &cursor(0, None),
            &"a".repeat(64),
            &MutationBatch {
                upserts: vec![remote],
                tombstones: Vec::new(),
            },
            &refs,
        )
        .unwrap();
    database
        .set_preview_path("remote-image", "previews/remote-image.jpg")
        .unwrap();

    let exported = database
        .export_sync_snapshot_for_scope(REMOTE_SCOPE)
        .unwrap();
    assert_eq!(
        exported.mutations.upserts[0].item.resource_path.as_deref(),
        Some(object_key.as_str())
    );
    assert!(exported.mutations.upserts[0].item.preview_path.is_none());

    let mut invalid_refs = refs;
    invalid_refs.get_mut("remote-image").unwrap()[0].object_key = "invalid".to_string();
    let mut newer = exported.mutations.upserts[0].clone();
    newer.version.modified_at_ms += 1;
    assert!(database
        .apply_sync_segment_with_resources(
            REMOTE_SCOPE,
            &cursor(1, Some("segment-1")),
            &MutationBatch {
                upserts: vec![newer],
                tombstones: Vec::new(),
            },
            &invalid_refs,
        )
        .is_err());
    assert!(
        database
            .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .unwrap()
            .sequence
            == 0
    );
    assert_eq!(
        database
            .export_sync_snapshot_for_scope(REMOTE_SCOPE)
            .unwrap()
            .mutations
            .upserts[0]
            .item
            .resource_path
            .as_deref(),
        Some(object_key.as_str())
    );
}
