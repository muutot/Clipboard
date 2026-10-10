//! Tombstone publication, purging and restore scenarios.

use super::*;

#[test]
fn permanent_delete_keeps_a_compact_tombstone() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&item("deleted", "hash-deleted", "deleted"))
        .unwrap();
    assert!(database.soft_delete("deleted").unwrap());
    let before_purge = database.count_sync_outbox().unwrap();
    assert!(database.permanently_delete("deleted").unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), before_purge);

    let snapshot = database.export_sync_snapshot().unwrap();
    assert!(snapshot.mutations.upserts.is_empty());
    assert_eq!(snapshot.mutations.tombstones.len(), 1);
    assert_eq!(snapshot.mutations.tombstones[0].item_id, "deleted");
    assert_eq!(
        snapshot.mutations.tombstones[0].content_hash,
        "hash-deleted"
    );
}

#[test]
fn purging_a_remote_tombstone_does_not_echo_a_delete() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&item(
            "remote-deleted",
            "hash-remote-deleted",
            "remote deleted",
        ))
        .unwrap();
    database.acknowledge_sync_outbox(1).unwrap();
    let remote_delete_version = current_time_ms() + 10_000;
    let delete = MutationBatch {
        upserts: Vec::new(),
        tombstones: vec![Tombstone {
            item_id: "remote-deleted".to_string(),
            kind: ClipboardKind::Text.into(),
            content_hash: "hash-remote-deleted".to_string(),
            deleted_at_ms: remote_delete_version,
            version: RecordVersion {
                modified_at_ms: remote_delete_version,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        }],
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(1, None), "a", &delete)
        .unwrap();

    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert!(database.permanently_delete("remote-deleted").unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    let snapshot = database.export_sync_snapshot().unwrap();
    assert_eq!(snapshot.mutations.tombstones.len(), 1);
    assert_eq!(
        snapshot.mutations.tombstones[0].version.writer_device_id,
        REMOTE_DEVICE
    );
}

#[test]
fn deleting_an_active_row_directly_still_publishes_a_tombstone() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&item(
            "direct-delete",
            "hash-direct-delete",
            "direct delete",
        ))
        .unwrap();
    database.acknowledge_sync_outbox(1).unwrap();

    assert!(database.delete_item("direct-delete").unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
    let snapshot = database.export_sync_snapshot().unwrap();
    assert_eq!(snapshot.mutations.tombstones.len(), 1);
    assert_eq!(snapshot.mutations.tombstones[0].item_id, "direct-delete");
}

#[test]
fn restoring_a_soft_deleted_item_replaces_its_tombstone() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&item("restored", "hash-restored", "restored"))
        .unwrap();
    assert!(database.soft_delete("restored").unwrap());
    assert!(database.restore_deleted("restored").unwrap());

    let snapshot = database.export_sync_snapshot().unwrap();
    assert_eq!(snapshot.mutations.upserts.len(), 1);
    assert!(snapshot.mutations.tombstones.is_empty());
}

#[test]
fn remote_publication_state_acknowledges_only_after_snapshot_or_segment_commit() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let first = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    assert!(!first.initialized);
    assert!(!first.remote_prepared);
    assert_eq!(
        database
            .get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap()
            .epoch,
        first.epoch
    );
    database.mark_sync_remote_prepared(REMOTE_SCOPE).unwrap();

    database
        .save_item(&item("snapshot", "hash-snapshot", "snapshot"))
        .unwrap();
    let snapshot = ObjectRef {
        key: "v1/snapshots/device/epoch/hash.pack".to_string(),
        sha256: "b".repeat(64),
        stored_size_bytes: 123,
        record_count: 1,
    };
    let published = database
        .commit_sync_bootstrap_published(REMOTE_SCOPE, &first.epoch, &snapshot, 1)
        .unwrap();
    assert!(published.initialized);
    assert!(published.remote_prepared);
    assert_eq!(published.snapshot.as_ref(), Some(&snapshot));
    assert_eq!(database.count_sync_outbox().unwrap(), 0);

    database
        .save_item(&item("segment", "hash-segment", "segment"))
        .unwrap();
    let segment_key = "v1/segments/device/epoch/segment.pack";
    let published = database
        .commit_sync_segment_published(REMOTE_SCOPE, &first.epoch, segment_key, 2)
        .unwrap();
    assert_eq!(published.published_sequence, 2);
    assert_eq!(published.last_segment_key.as_deref(), Some(segment_key));
    assert_eq!(database.count_sync_outbox().unwrap(), 0);

    let reset = database.reset_sync_remote_state(REMOTE_SCOPE).unwrap();
    assert_ne!(reset.epoch, first.epoch);
    assert!(reset.remote_prepared);
    assert!(!reset.initialized);
    assert!(reset.snapshot.is_none());
}
