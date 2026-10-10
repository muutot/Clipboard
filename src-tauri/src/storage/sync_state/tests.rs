//! Regression tests for `sync_state` (moved verbatim from `sync_state.rs`).

use super::*;
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    storage::{ClipboardRepository, TextItemUpdate},
};

fn item(id: &str, hash: &str, text: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_string(),
        kind: ClipboardKind::Text,
        title: text.to_string(),
        text_content: Some(text.to_string()),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: hash.to_string(),
        source_app: Some("test".to_string()),
        icon_path: None,
        size_bytes: text.len() as u64,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some("{}".to_string()),
    }
}

const REMOTE_SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REMOTE_DEVICE: &str = "11111111-1111-4111-8111-111111111111";
const REMOTE_EPOCH: &str = "22222222-2222-4222-8222-222222222222";

fn replicated(id: &str, hash: &str, text: &str, version: RecordVersion) -> ReplicatedItem {
    ReplicatedItem {
        item: item(id, hash, text).into(),
        version,
    }
}

fn cursor(sequence: u64, key: Option<&str>) -> DeviceCursor {
    DeviceCursor {
        device_id: REMOTE_DEVICE.to_string(),
        epoch: REMOTE_EPOCH.to_string(),
        sequence,
        last_segment_key: key.map(str::to_string),
    }
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
fn outbox_batch_coalesces_repeated_changes_to_current_state() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&item("item", "hash-1", "first"))
        .unwrap();
    for (title, text, hash) in [("second", "second", "hash-2"), ("third", "third", "hash-3")] {
        database
            .update_text_item(&TextItemUpdate {
                id: "item",
                kind: ClipboardKind::Text,
                title,
                text_content: text,
                content_hash: hash,
                size_bytes: text.len() as u64,
                metadata_json: None,
            })
            .unwrap();
    }

    let batch = database.get_sync_outbox_batch(100).unwrap().unwrap();
    assert_eq!(batch.first_sequence, 1);
    assert_eq!(batch.last_sequence, 3);
    assert_eq!(batch.mutations.len(), 1);
    assert_eq!(batch.mutations.upserts[0].item.title, "third");
    assert_eq!(batch.mutations.upserts[0].item.content_hash, "hash-3");
    assert_eq!(database.acknowledge_sync_outbox(3).unwrap(), 3);
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(database.export_sync_snapshot().unwrap().through_sequence, 3);
}

#[test]
fn snapshot_visitor_is_deterministic_bounded_and_point_in_time() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    for index in 0..7 {
        database
            .save_item(&item(
                &format!("item-{index:02}"),
                &format!("hash-{index:02}"),
                &format!("value-{index:02}"),
            ))
            .unwrap();
    }
    database.soft_delete("item-06").unwrap();
    let expected_sequence = current_sequence(&database.connection.lock().unwrap()).unwrap();

    let mut batches = Vec::new();
    let export = database
        .visit_sync_snapshot_for_scope(REMOTE_SCOPE, 2, |batch| {
            assert!(batch.len() <= 2);
            batches.push(batch);
            Ok(())
        })
        .unwrap();

    assert_eq!(export.through_sequence, expected_sequence);
    assert_eq!(export.record_count, 7);
    let upsert_ids = batches
        .iter()
        .flat_map(|batch| batch.upserts.iter().map(|item| item.item.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        upsert_ids,
        vec!["item-00", "item-01", "item-02", "item-03", "item-04", "item-05"]
    );
    assert_eq!(
        batches
            .iter()
            .flat_map(|batch| batch.tombstones.iter())
            .map(|tombstone| tombstone.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["item-06"]
    );
}

#[test]
fn batched_snapshot_apply_rolls_back_every_prior_chunk_on_late_failure() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let good = MutationBatch {
        upserts: vec![replicated(
            "remote-good",
            "remote-good-hash",
            "good",
            RecordVersion {
                modified_at_ms: 10,
                writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    let bad = MutationBatch {
        upserts: vec![replicated(
            "remote-bad",
            "remote-bad-hash",
            "bad",
            RecordVersion {
                modified_at_ms: 11,
                writer_device_id: "not-a-uuid".to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    let cursor = DeviceCursor {
        device_id: "22222222-2222-4222-8222-222222222222".to_string(),
        epoch: "33333333-3333-4333-8333-333333333333".to_string(),
        sequence: 2,
        last_segment_key: None,
    };

    assert!(database
        .apply_sync_snapshot_batches(
            REMOTE_SCOPE,
            &cursor,
            &"a".repeat(64),
            [Ok((good, BTreeMap::new())), Ok((bad, BTreeMap::new())),],
        )
        .is_err());
    assert!(database.get_item("remote-good").unwrap().is_none());
    assert!(database
        .get_sync_cursor(REMOTE_SCOPE, &cursor.device_id)
        .unwrap()
        .is_none());
}

/// Peak residency during a pack apply must be one batch, not the whole pack.
///
/// The apply used to `collect()` the decoded iterator before opening the
/// transaction, so a pack near the 1 GiB global uncompressed limit had every
/// decoded batch resident at once. The probe below records how many decoded
/// batches are alive at the moment the iterator is asked for the next one and
/// while the current one is applied. A materialising implementation would
/// show an ever-growing count; a lazy one shows exactly one.
#[test]
fn decoded_pack_batches_are_folded_lazily_so_residency_stays_bounded() {
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Probe {
        live: Rc<RefCell<usize>>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            *self.live.borrow_mut() -= 1;
        }
    }

    const BATCHES: usize = 8;
    let live = Rc::new(RefCell::new(0usize));
    let peak_at_yield = Rc::new(RefCell::new(0usize));
    let peak_at_apply = Rc::new(RefCell::new(0usize));

    let mut next_id = 0usize;
    let yield_live = Rc::clone(&live);
    let peak_on_yield = Rc::clone(&peak_at_yield);
    let mut batches = std::iter::from_fn(move || {
        if next_id == BATCHES {
            return None;
        }
        next_id += 1;
        // The moment before the next batch exists: whatever is still alive
        // here is what the previous iteration failed to release.
        let resident = *yield_live.borrow();
        let mut peak = peak_on_yield.borrow_mut();
        if resident > *peak {
            *peak = resident;
        }
        *yield_live.borrow_mut() += 1;
        Some(Ok(Probe {
            live: Rc::clone(&yield_live),
        }))
    });

    let apply_live = Rc::clone(&live);
    let peak_in_apply = Rc::clone(&peak_at_apply);
    let applied = Database::fold_decoded_batches(&mut batches, |_probe| {
        let resident = *apply_live.borrow();
        let mut peak = peak_in_apply.borrow_mut();
        if resident > *peak {
            *peak = resident;
        }
        assert!(
            resident == 1,
            "{resident} decoded batches were resident while one was being applied, so \
                 residency is not bounded by a single batch"
        );
        Ok(1)
    })
    .expect("every batch applies cleanly");

    assert_eq!(applied, BATCHES as u64, "every batch must be applied");
    assert_eq!(
        *peak_at_yield.borrow(),
        0,
        "the previous batch must be released before the next one is decoded"
    );
    assert_eq!(
        *peak_at_apply.borrow(),
        1,
        "only the current batch is applied"
    );
    assert_eq!(*live.borrow(), 0, "every batch must be released");
}

/// A batch that fails to decode must roll the whole apply back, not leave
/// the earlier batches committed.
#[test]
fn a_failing_late_batch_rolls_back_the_earlier_ones() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let first = MutationBatch {
        upserts: vec![replicated(
            "remote-first",
            "remote-first-hash",
            "first",
            RecordVersion {
                modified_at_ms: 10,
                writer_device_id: "11111111-1111-4111-8111-111111111111".to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    let cursor = DeviceCursor {
        device_id: "22222222-2222-4222-8222-222222222222".to_string(),
        epoch: "33333333-3333-4333-8333-333333333333".to_string(),
        sequence: 1,
        last_segment_key: None,
    };

    let result = database.apply_sync_snapshot_batches(
        REMOTE_SCOPE,
        &cursor,
        &"a".repeat(64),
        [
            Ok((first, BTreeMap::new())),
            Err(StorageError::InvalidSyncState(
                "checkpoint payload does not match its reference".to_string(),
            )),
        ],
    );
    assert!(result.is_err(), "the decode failure must surface");
    assert!(
        database.get_item("remote-first").unwrap().is_none(),
        "a batch applied before the failure must be rolled back"
    );
    assert!(database
        .get_sync_cursor(REMOTE_SCOPE, &cursor.device_id)
        .unwrap()
        .is_none());
}

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
fn initializing_v1_again_is_idempotent_and_keeps_pending_changes() {
    let database = Database::open_in_memory().unwrap();
    assert!(database.initialize_sync().unwrap());
    database
        .save_item(&item("pending", "hash-pending", "pending"))
        .unwrap();
    assert!(!database.initialize_sync().unwrap());
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
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

#[test]
fn remote_snapshot_apply_is_atomic_idempotent_and_does_not_echo() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let writer = RecordVersion {
        modified_at_ms: 500,
        writer_device_id: REMOTE_DEVICE.to_string(),
    };
    let mut remote = replicated("remote", "hash-remote", "remote", writer);
    remote.item.metadata_json = Some(r#"{"tags":["shared","work"]}"#.to_string());
    let mutations = MutationBatch {
        upserts: vec![remote],
        tombstones: Vec::new(),
    };

    assert_eq!(
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
            .unwrap(),
        1
    );
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(
        database.get_item("remote").unwrap().unwrap().title,
        "remote"
    );
    let tags = database.list_all_tags().unwrap();
    assert_eq!(
        tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>(),
        vec!["shared", "work"]
    );
    assert_eq!(
        database
            .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .unwrap(),
        cursor(0, None)
    );
    assert_eq!(
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
            .unwrap(),
        0
    );
}

#[test]
fn record_writer_breaks_equal_timestamp_ties_deterministically() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let lower_writer = "00000000-0000-4000-8000-000000000001";
    let higher_writer = "ffffffff-ffff-4fff-bfff-ffffffffffff";
    let initial = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie",
            "lower",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: lower_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"d".repeat(64), &initial)
        .unwrap();

    let winner = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie-winner",
            "higher",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: higher_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-1")), &winner)
        .unwrap();
    assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");

    let loser = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie-loser",
            "lower-again",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: lower_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    assert_eq!(
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-2")), &loser,)
            .unwrap(),
        0
    );
    assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");
}

#[test]
fn checkpoint_apply_updates_mutations_cursors_and_generation_atomically() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let second_device = "33333333-3333-4333-8333-333333333333";
    let second_epoch = "44444444-4444-4444-8444-444444444444";
    let first_key =
        crate::sync::v1::segment_object_key(REMOTE_DEVICE, REMOTE_EPOCH, 1, 2, &"a".repeat(64))
            .unwrap();
    let cursors = vec![
        cursor(2, Some(&first_key)),
        DeviceCursor {
            device_id: second_device.to_string(),
            epoch: second_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        },
    ];
    let mutations = MutationBatch {
        upserts: vec![replicated(
            "checkpoint-item",
            "hash-checkpoint",
            "checkpoint",
            RecordVersion {
                modified_at_ms: 900,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };

    assert_eq!(
        database
            .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
            .unwrap(),
        1
    );
    assert!(database.get_item("checkpoint-item").unwrap().is_some());
    assert_eq!(database.list_sync_cursors(REMOTE_SCOPE).unwrap(), cursors);
    assert_eq!(
        database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
        cursors
    );
    assert_eq!(
        database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
        Some((1, "b".repeat(64)))
    );
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(
        database
            .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
            .unwrap(),
        0
    );
}

#[test]
fn checkpoint_apply_does_not_regress_cursors_ahead_of_the_frozen_vector() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mutations = MutationBatch {
        upserts: vec![replicated(
            "checkpoint-item",
            "hash-checkpoint",
            "checkpoint",
            RecordVersion {
                modified_at_ms: 900,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    let first_key =
        crate::sync::v1::segment_object_key(REMOTE_DEVICE, REMOTE_EPOCH, 1, 2, &"a".repeat(64))
            .unwrap();

    // Generation 1 freezes the device at sequence 2.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            1,
            &"b".repeat(64),
            &[cursor(2, Some(&first_key))],
            &mutations,
        )
        .unwrap();
    // Generation 2 publishes a newer vector (sequence 5) that the pull
    // path applies.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"e".repeat(64),
            &[cursor(5, None)],
            &mutations,
        )
        .unwrap();
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![cursor(5, None)]
    );

    // Re-applying the generation-2 checkpoint must not reset the cursor
    // back to its stale in-vector value: the segments between 2 and 5
    // were already applied, so a regression would force a full re-pull.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"e".repeat(64),
            &[cursor(2, Some(&first_key))],
            &mutations,
        )
        .unwrap();
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![cursor(5, None)]
    );
}

#[test]
fn invalid_checkpoint_rolls_back_rows_cursors_and_generation() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let existing = cursor(1, None);
    let initial = MutationBatch {
        upserts: vec![replicated(
            "existing-checkpoint",
            "hash-existing-checkpoint",
            "existing",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            1,
            &"c".repeat(64),
            std::slice::from_ref(&existing),
            &initial,
        )
        .unwrap();
    let invalid = MutationBatch {
        upserts: vec![ReplicatedItem {
            item: item("broken-checkpoint", "hash-broken-checkpoint", "broken").into(),
            version: RecordVersion {
                modified_at_ms: 600,
                writer_device_id: "not-a-uuid".to_string(),
            },
        }],
        tombstones: Vec::new(),
    };

    assert!(database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"d".repeat(64),
            &[DeviceCursor {
                device_id: "33333333-3333-4333-8333-333333333333".to_string(),
                epoch: "44444444-4444-4444-8444-444444444444".to_string(),
                sequence: 0,
                last_segment_key: None,
            }],
            &invalid,
        )
        .is_err());
    assert!(database.get_item("broken-checkpoint").unwrap().is_none());
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![existing.clone()]
    );
    assert_eq!(
        database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
        vec![existing]
    );
    assert_eq!(
        database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
        Some((1, "c".repeat(64)))
    );
}

#[test]
fn tombstone_blocks_an_older_later_segment_from_resurrecting_a_row() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let initial = MutationBatch {
        upserts: vec![replicated(
            "victim",
            "hash-victim",
            "live",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"e".repeat(64), &initial)
        .unwrap();
    let deletion = MutationBatch {
        upserts: Vec::new(),
        tombstones: vec![Tombstone {
            item_id: "victim".to_string(),
            kind: ClipboardKind::Text.into(),
            content_hash: "hash-victim".to_string(),
            deleted_at_ms: 200,
            version: RecordVersion {
                modified_at_ms: 200,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        }],
    };
    database
        .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-delete")), &deletion)
        .unwrap();
    let stale = MutationBatch {
        upserts: vec![replicated(
            "victim",
            "hash-victim",
            "stale",
            RecordVersion {
                modified_at_ms: 150,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    assert_eq!(
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-stale")), &stale,)
            .unwrap(),
        0
    );
    assert!(database
        .export_sync_snapshot()
        .unwrap()
        .mutations
        .upserts
        .is_empty());
    assert_eq!(
        database
            .export_sync_snapshot()
            .unwrap()
            .mutations
            .tombstones
            .len(),
        1
    );
}

#[test]
fn failed_remote_batch_rolls_back_rows_suppression_and_cursor() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mut image = item("local-image", "same-image-hash", "image");
    image.kind = ClipboardKind::Image;
    database.save_item(&image).unwrap();
    database.acknowledge_sync_outbox(1).unwrap();

    let first = replicated(
        "would-be-inserted",
        "unique-text-hash",
        "first",
        RecordVersion {
            modified_at_ms: 300,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    let mut collision = replicated(
        "different-image-id",
        "same-image-hash",
        "collision",
        RecordVersion {
            modified_at_ms: 300,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    collision.item.kind = ClipboardKind::Image.into();
    let failed = MutationBatch {
        upserts: vec![first, collision],
        tombstones: Vec::new(),
    };
    assert!(database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"f".repeat(64), &failed)
        .is_err());
    assert!(database.get_item("would-be-inserted").unwrap().is_none());
    assert!(database
        .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
        .unwrap()
        .is_none());

    database
        .save_item(&item("after-failure", "hash-after", "after"))
        .unwrap();
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
}
