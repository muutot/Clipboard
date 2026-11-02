//! Database initialization, idempotency and head-cache round trips.

use super::*;

fn run_sync_write_during_capture(
    table: &'static str,
    operation: impl FnOnce(&Database) -> Result<(), StorageError>,
) {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let root = std::env::temp_dir().join(format!("clipboard-sync-write-race-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("history.sqlite3");
    let database = Database::open(&path).unwrap();
    let writer = Arc::new(Database::open(&path).unwrap());
    writer
        .save_item(&item("capture", "capture-hash", "synthetic capture"))
        .unwrap();
    writer
        .with_connection(|connection| {
            connection.busy_timeout(std::time::Duration::ZERO)?;
            Ok(())
        })
        .unwrap();
    let attempted = Arc::new(AtomicBool::new(false));
    let attempted_in_hook = attempted.clone();
    let writer_in_hook = writer.clone();
    database
        .with_connection(|connection| {
            connection.authorizer(Some(move |context: AuthContext<'_>| {
                let table_name = match context.action {
                    AuthAction::Insert { table_name } | AuthAction::Update { table_name, .. } => {
                        table_name
                    }
                    _ => return Authorization::Allow,
                };
                if table_name == table && !attempted_in_hook.swap(true, Ordering::SeqCst) {
                    // Preparation of the first write happens after the production
                    // decision read. A real independent usage write either commits
                    // there or is briefly refused by an existing writer reservation.
                    match writer_in_hook.set_last_used("capture") {
                        Ok(true) => {}
                        Err(StorageError::Sqlite(error))
                            if error.sqlite_error_code()
                                == Some(rusqlite::ErrorCode::DatabaseBusy) => {}
                        other => panic!("unexpected capture result: {other:?}"),
                    }
                }
                Authorization::Allow
            }))?;
            Ok(())
        })
        .unwrap();
    let result = operation(&database);
    database
        .with_connection(|connection| {
            connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
            assert!(connection.is_autocommit());
            Ok(())
        })
        .unwrap();
    assert!(writer.set_last_used("capture").unwrap());
    assert!(database.get_item("capture").unwrap().is_some());
    assert!(
        attempted.load(Ordering::SeqCst),
        "real writer interleaving never ran"
    );
    drop(writer);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        result.is_ok(),
        "sync write failed after an unrelated capture: {result:?}"
    );
}

#[test]
fn sync_initialization_survives_an_independent_capture_write() {
    run_sync_write_during_capture("sync_metadata", |database| {
        assert!(database.initialize_sync()?);
        assert!(database.is_sync_initialized()?);
        assert!(!database.initialize_sync()?);
        Ok(())
    });
}

#[test]
fn remote_state_creation_survives_an_independent_capture_write() {
    run_sync_write_during_capture("sync_publication_state", |database| {
        let created = database.get_or_create_sync_remote_state(REMOTE_SCOPE)?;
        assert_eq!(
            database.get_or_create_sync_remote_state(REMOTE_SCOPE)?,
            created
        );
        Ok(())
    });
}

#[test]
fn checkpoint_publication_survives_an_independent_capture_write() {
    run_sync_write_during_capture("sync_checkpoint_state", |database| {
        database.record_sync_checkpoint_published(REMOTE_SCOPE, 1, &"a".repeat(64), &[])?;
        assert_eq!(
            database.get_sync_checkpoint_state(REMOTE_SCOPE)?,
            Some((1, "a".repeat(64)))
        );
        Ok(())
    });
}

#[test]
fn migration_identity_fork_preserves_records_versions_outbox_and_resource_refs() {
    use crate::sync::v1::{ResourceCategory, SyncResourceRef};
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let original_device = database.get_sync_device_id().unwrap();
    let mut initial = item("initial", "hash-initial", "initial");
    initial.icon_path = Some("local-icon.png".into());
    database.save_item(&initial).unwrap();
    let mut batch = database
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .unwrap();
    let resource =
        crate::sync::v1::resource_object_key(ResourceCategory::Icon, &"a".repeat(64), "png")
            .unwrap();
    batch.mutations.upserts[0].item.icon_path = Some(resource.clone());
    let refs = BTreeMap::from([(
        "initial".into(),
        vec![SyncResourceRef {
            slot: "icon".into(),
            ordinal: 0,
            object_key: resource,
        }],
    )]);
    database
        .record_sync_resource_refs(REMOTE_SCOPE, &batch.mutations, &refs)
        .unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database.mark_sync_remote_prepared(REMOTE_SCOPE).unwrap();
    database
        .commit_sync_bootstrap_published(
            REMOTE_SCOPE,
            &state.epoch,
            &ObjectRef {
                key: snapshot_object_key(&original_device, &state.epoch, &"b".repeat(64)).unwrap(),
                sha256: "b".repeat(64),
                stored_size_bytes: 1,
                record_count: 1,
            },
            batch.last_sequence,
        )
        .unwrap();
    database
        .save_item(&item("pending", "hash-pending", "pending"))
        .unwrap();
    database
        .save_item(&item("removed", "hash-removed", "removed"))
        .unwrap();
    database.soft_delete("removed").unwrap();
    let before = database
        .export_sync_snapshot_for_scope(REMOTE_SCOPE)
        .unwrap();
    let pending_before = database
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap();
    let device = database.fork_sync_identity_for_migration().unwrap();
    assert_ne!(device, original_device);
    assert!(Uuid::parse_str(&device).is_ok());
    assert_eq!(database.get_sync_device_id().unwrap(), device);
    assert!(database.is_sync_initialized().unwrap());
    assert!(!database.initialize_sync().unwrap());
    assert_eq!(
        database
            .export_sync_snapshot_for_scope(REMOTE_SCOPE)
            .unwrap(),
        before
    );
    assert_eq!(
        database
            .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
            .unwrap(),
        pending_before
    );
    assert_eq!(
        database
            .list_sync_resource_refs_for_scope(REMOTE_SCOPE)
            .unwrap(),
        refs
    );
    let forked = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    assert_ne!(forked.epoch, state.epoch);
    assert!(!forked.initialized);
    assert!(forked.snapshot.is_none());
    assert_eq!(forked.published_sequence, 0);
    assert!(forked.remote_prepared);
}

#[test]
fn failed_migration_identity_fork_preserves_identity_and_publication_state() {
    let database = Database::open_in_memory().unwrap();
    let device = database.get_sync_device_id().unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database.with_connection(|connection| {
        connection.execute_batch("CREATE TRIGGER fixture_identity_failure BEFORE UPDATE ON sync_publication_state BEGIN SELECT RAISE(FAIL, 'fixture publication reset failure'); END")?;
        Ok(())
    }).unwrap();
    assert!(database.fork_sync_identity_for_migration().is_err());
    assert_eq!(database.get_sync_device_id().unwrap(), device);
    assert_eq!(
        database
            .get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap(),
        state
    );
}

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
