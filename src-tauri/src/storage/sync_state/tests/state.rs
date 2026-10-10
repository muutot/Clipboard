//! Database initialization, idempotency and head-cache round trips.

use super::*;

#[test]
fn initializing_v1_preserves_items_and_discards_pending_v1_state() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&item("existing", "hash-existing", "existing"))
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO sync_item_aliases (alias_id, item_id)
                     VALUES ('pending-alias', 'existing')",
                [],
            )?;
            connection.execute(
                "UPDATE clipboard_items
                        SET sync_writer_device_id = 'pending-writer'
                      WHERE id = 'existing'",
                [],
            )?;
            connection.execute(
                "INSERT INTO sync_publication_state (remote_scope, epoch)
                     VALUES ('pending-remote', 'pending-epoch')",
                [],
            )?;
            connection.execute(
                "INSERT INTO sync_outbox
                        (item_id, operation, kind, content_hash, modified_at_ms, writer_device_id)
                     VALUES ('existing', 'upsert', 'text', 'hash-existing', 100, 'pending-writer')",
                [],
            )?;
            connection.execute(
                "INSERT INTO sync_checkpoint_state
                        (remote_scope, generation, checkpoint_sha256)
                     VALUES (?1, 1, ?2)",
                params![REMOTE_SCOPE, "a".repeat(64)],
            )?;
            connection.execute(
                "INSERT INTO sync_metadata (key, value) VALUES (?1, 'stale')",
                [sync_head_cache_key(REMOTE_SCOPE, REMOTE_DEVICE)?],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(!database.is_sync_initialized().unwrap());

    assert!(database.initialize_sync().unwrap());
    assert!(database.is_sync_initialized().unwrap());
    let snapshot = database.export_sync_snapshot().unwrap();
    assert_eq!(snapshot.through_sequence, 0);
    assert_eq!(snapshot.mutations.upserts.len(), 1);
    assert_eq!(snapshot.mutations.upserts[0].item.id, "existing");
    assert_eq!(
        snapshot.mutations.upserts[0].version.writer_device_id,
        database.get_sync_device_id().unwrap()
    );
    database
        .with_connection(|connection| {
            let alias_count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_item_aliases", [], |row| {
                    row.get(0)
                })?;
            let publication_count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_publication_state", [], |row| {
                    row.get(0)
                })?;
            let outbox_count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))?;
            let checkpoint_count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_checkpoint_state", [], |row| {
                    row.get(0)
                })?;
            let metadata_count: i64 =
                connection.query_row("SELECT COUNT(*) FROM sync_metadata", [], |row| row.get(0))?;
            assert_eq!(alias_count, 0);
            assert_eq!(publication_count, 0);
            assert_eq!(outbox_count, 0);
            assert_eq!(checkpoint_count, 0);
            assert_eq!(metadata_count, 2);
            Ok(())
        })
        .unwrap();

    database.save_item(&item("new", "hash-new", "new")).unwrap();
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
}

#[test]
fn sync_head_cache_round_trips_and_ignores_corrupt_values() {
    let database = Database::open_in_memory().unwrap();
    let snapshot_sha256 = "b".repeat(64);
    let head = DeviceHead {
        device_id: REMOTE_DEVICE.to_string(),
        epoch: REMOTE_EPOCH.to_string(),
        snapshot: ObjectRef {
            key: snapshot_object_key(REMOTE_DEVICE, REMOTE_EPOCH, &snapshot_sha256).unwrap(),
            sha256: snapshot_sha256,
            stored_size_bytes: 123,
            record_count: 4,
        },
        published_sequence: 5,
        last_segment_key: None,
        updated_at_ms: 10,
    };
    database
        .record_sync_head_cache(
            REMOTE_SCOPE,
            REMOTE_DEVICE,
            "\"etag-one\"",
            88,
            Some(1234),
            &head,
        )
        .unwrap();

    let cached = database
        .get_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE)
        .unwrap()
        .unwrap();
    assert_eq!(cached.etag, "\"etag-one\"");
    assert_eq!(cached.stored_size_bytes, 88);
    assert_eq!(cached.modified_ms, Some(1234));
    assert!(cached.matches_head(&head));
    assert!(cached.matches_cursor(&DeviceCursor {
        device_id: REMOTE_DEVICE.to_string(),
        epoch: REMOTE_EPOCH.to_string(),
        sequence: 5,
        last_segment_key: None,
    }));

    let key = sync_head_cache_key(REMOTE_SCOPE, REMOTE_DEVICE).unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE sync_metadata SET value = '{broken' WHERE key = ?1",
                [&key],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(database
        .get_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE)
        .unwrap()
        .is_none());
    assert!(database
        .record_sync_head_cache(REMOTE_SCOPE, REMOTE_DEVICE, "bad\netag", 88, None, &head)
        .is_err());
}

#[test]
fn initializing_v1_again_is_idempotent_and_keeps_pending_changes() {
    let database = Database::open_in_memory().unwrap();
    assert!(database.initialize_sync().unwrap());
    database
        .save_item(&item("pending", "hash-pending", "pending"))
        .unwrap();
    assert!(!database.initialize_sync().unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
}
