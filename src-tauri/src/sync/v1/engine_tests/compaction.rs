//! Checkpoint CAS, compaction, retention and garbage-collection scenarios.

use super::*;

#[test]
fn checkpoint_cas_loser_never_garbage_collects_history() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-cas-loser");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let protected_snapshot = state.snapshot.as_ref().unwrap().key.clone();
    store.deleted.lock().unwrap().clear();
    *store.fail_checkpoint_cas.lock().unwrap() = true;
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();

    let mut result = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &state,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap();

    assert_eq!(result.deleted_remote_objects, 0);
    assert!(result.bytes_uploaded > 0);
    assert!(store.deleted.lock().unwrap().is_empty());
    assert!(store
        .objects
        .lock()
        .unwrap()
        .contains_key(&protected_snapshot));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn compaction_survives_stores_that_omit_etags() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-no-etag");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let previous_checkpoint = database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    // Some S3-compatible stores omit ETag headers on GET responses; compaction
    // must degrade to an unconditional put instead of failing the whole run.
    *store.get_without_etags.lock().unwrap() = true;

    let mut result = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &database.get_sync_device_id().unwrap(),
        &state,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap();

    assert!(result.bytes_uploaded > 0);
    let (generation, _) = database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .unwrap();
    assert!(generation > previous_checkpoint.0);

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn checkpoint_pointer_must_be_readable_before_garbage_collection() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-readback");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let previous_checkpoint = database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    store.deleted.lock().unwrap().clear();
    *store.drop_checkpoint_after_success.lock().unwrap() = true;

    let mut result = SyncEngineResult::default();
    let error = maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &database.get_sync_device_id().unwrap(),
        &state,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap_err();

    assert!(error.contains("disappeared after a successful conditional write"));
    assert_eq!(result.deleted_remote_objects, 0);
    assert!(store.deleted.lock().unwrap().is_empty());
    assert_eq!(
        database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
        Some(previous_checkpoint)
    );

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn successful_compaction_keeps_current_and_previous_checkpoints_only() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-retention");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let first_state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let first_snapshot = first_state.snapshot.as_ref().unwrap().key.clone();
    database.save_item(&text_item("second", "second")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let first_state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let mut first_compaction = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &first_state,
        None,
        options().resource_limits,
        &mut first_compaction,
    )
    .unwrap();
    assert!(!store.objects.lock().unwrap().contains_key(&first_snapshot));

    database.save_item(&text_item("third", "third")).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let mut second_compaction = SyncEngineResult::default();
    maybe_compact(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &device_id,
        &state,
        None,
        options().resource_limits,
        &mut second_compaction,
    )
    .unwrap();

    let checkpoint_count = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with("v1/checkpoints/"))
        .count();
    assert_eq!(checkpoint_count, 2);
    let head = decode_checkpoint_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(CHECKPOINT_HEAD_KEY)
            .unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(head.generation, 3);
    assert!(head.previous_checkpoint.is_some());

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn stale_compactor_never_prunes_a_newer_checkpoint_candidate() {
    let store = MemoryStore::default();
    let device_id = "11111111-1111-4111-8111-111111111111";
    let epoch = "22222222-2222-4222-8222-222222222222";
    let vector = vec![DeviceCursor {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        sequence: 0,
        last_segment_key: None,
    }];
    let checkpoint = |generation| CheckpointFixture {
        generation,
        vector: vector.clone(),
        mutations: crate::sync::v1::MutationBatch {
            upserts: Vec::new(),
            tombstones: Vec::new(),
        },
    };
    let first = insert_checkpoint(&store, &checkpoint(1));
    let second = insert_checkpoint(&store, &checkpoint(2));
    let third = insert_checkpoint(&store, &checkpoint(3));
    let future = insert_checkpoint(&store, &checkpoint(4));
    let head = CheckpointHead {
        generation: 3,
        checkpoint: third.clone(),
        vector,
        previous_checkpoint: Some(second.clone()),
        updated_at_ms: 1,
    };

    assert_eq!(
        engine::test_support::prune_unreferenced_checkpoints(&store, &head).unwrap(),
        1
    );

    let objects = store.objects.lock().unwrap();
    assert!(!objects.contains_key(&first.key));
    assert!(objects.contains_key(&second.key));
    assert!(objects.contains_key(&third.key));
    assert!(objects.contains_key(&future.key));
}

#[test]
fn fourth_device_bootstraps_after_three_device_compaction_and_gc() {
    let store = MemoryStore::default();
    let first_paths = temp_paths("compact-first");
    let second_paths = temp_paths("compact-second");
    let third_paths = temp_paths("compact-third");
    let fourth_paths = temp_paths("compact-fourth");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    let third = Database::open(&third_paths.database).unwrap();
    let fourth = Database::open(&fourth_paths.database).unwrap();
    first.save_item(&text_item("first-only", "first")).unwrap();
    second
        .save_item(&text_item("second-only", "second"))
        .unwrap();
    third.save_item(&text_item("third-only", "third")).unwrap();

    for _ in 0..2 {
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
        sync_database(
            &store,
            &second,
            &second_paths,
            REMOTE_SCOPE,
            None,
            options(),
        )
        .unwrap();
        sync_database(&store, &third, &third_paths, REMOTE_SCOPE, None, options()).unwrap();
    }
    let all_ids = ["first-only", "second-only", "third-only"];
    for database in [&first, &second, &third] {
        for id in all_ids {
            assert!(
                database.get_item(id).unwrap().is_some(),
                "missing item {id}"
            );
        }
    }

    first
        .save_item(&text_item("after-convergence", "after convergence"))
        .unwrap();
    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

    let first_id = first.get_sync_device_id().unwrap();
    let first_state = first.get_or_create_sync_remote_state(REMOTE_SCOPE).unwrap();
    first
        .with_connection(|connection| {
            connection.execute(
                "DELETE FROM sync_checkpoint_cursors WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
    let mut compacted = SyncEngineResult::default();
    maybe_compact(
        &store,
        &first,
        &first_paths,
        REMOTE_SCOPE,
        &first_id,
        &first_state,
        None,
        options().resource_limits,
        &mut compacted,
    )
    .unwrap();
    assert!(compacted.deleted_remote_objects > 0);

    store.gets.lock().unwrap().clear();
    let bootstrap = sync_database(
        &store,
        &fourth,
        &fourth_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(bootstrap.downloaded_entries >= 3);
    for id in [
        "first-only",
        "second-only",
        "third-only",
        "after-convergence",
    ] {
        assert!(fourth.get_item(id).unwrap().is_some(), "missing item {id}");
    }
    assert!(store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY));

    drop(first);
    drop(second);
    drop(third);
    drop(fourth);
    for paths in [first_paths, second_paths, third_paths, fourth_paths] {
        fs::remove_dir_all(paths.project).unwrap();
    }
}

#[test]
fn missing_segment_chain_recovers_from_newer_checkpoint() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("gap-source");
    let target_paths = temp_paths("gap-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("initial", "initial")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("segment-one", "one")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("segment-two", "two")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source
        .save_item(&text_item("segment-three", "three"))
        .unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    let source_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let second_segment_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with(&segment_prefix(&source_id, &state.epoch).unwrap()))
        .find(|key| parse_segment_key(key).unwrap().first_sequence == 2)
        .cloned()
        .unwrap();
    store.objects.lock().unwrap().remove(&second_segment_key);
    let current_head = decode_checkpoint_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(CHECKPOINT_HEAD_KEY)
            .unwrap(),
        None,
    )
    .unwrap();
    let checkpoint = CheckpointFixture {
        generation: current_head.generation + 1,
        vector: vec![DeviceCursor {
            device_id: source_id.clone(),
            epoch: state.epoch.clone(),
            sequence: 3,
            last_segment_key: state.last_segment_key.clone(),
        }],
        mutations: source.export_sync_snapshot().unwrap().mutations,
    };
    let reference = insert_checkpoint(&store, &checkpoint);
    insert_checkpoint_head(
        &store,
        &checkpoint,
        reference,
        Some(current_head.checkpoint),
    );
    target
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            checkpoint.generation,
            &engine::test_support::checkpoint_digest_for_generation(
                &decode_checkpoint_head(
                    &store
                        .objects
                        .lock()
                        .unwrap()
                        .get(CHECKPOINT_HEAD_KEY)
                        .unwrap()
                        .clone(),
                    None,
                )
                .unwrap(),
                checkpoint.generation,
            )
            .unwrap(),
            &checkpoint.vector,
            &checkpoint.mutations,
        )
        .unwrap();
    assert_eq!(
        target
            .get_sync_cursor(REMOTE_SCOPE, &source_id)
            .unwrap()
            .unwrap()
            .sequence,
        3
    );
    target
        .with_connection(|connection| {
            connection.execute(
                "UPDATE sync_cursors
                    SET sequence = 1, last_segment_key = ?3
                  WHERE remote_scope = ?1 AND device_id = ?2",
                rusqlite::params![REMOTE_SCOPE, &source_id, {
                    let first_segment = store
                        .objects
                        .lock()
                        .unwrap()
                        .keys()
                        .filter(|key| {
                            key.starts_with(&segment_prefix(&source_id, &state.epoch).unwrap())
                        })
                        .find(|key| parse_segment_key(key).unwrap().first_sequence == 1)
                        .cloned()
                        .unwrap();
                    first_segment
                }],
            )?;
            Ok(())
        })
        .unwrap();

    let recovered = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert!(recovered.downloaded_entries >= 4);
    for id in ["initial", "segment-one", "segment-two", "segment-three"] {
        assert!(target.get_item(id).unwrap().is_some(), "missing item {id}");
    }
    assert_eq!(
        target
            .get_sync_cursor(REMOTE_SCOPE, &source_id)
            .unwrap()
            .unwrap()
            .sequence,
        3
    );

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn pulls_and_garbage_collects_more_than_one_thousand_segments() {
    const SEGMENT_COUNT: u64 = 1_001;

    let store = MemoryStore::default();
    let paths = temp_paths("segment-pagination");
    let database = Database::open(&paths.database).unwrap();
    database.initialize_sync().unwrap();
    let device_id = "11111111-1111-4111-8111-111111111111";
    let epoch = "22222222-2222-4222-8222-222222222222";
    let snapshot_header = SnapshotPackHeader {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        through_sequence: 0,
    };
    let directory = std::env::temp_dir().join(format!(
        "clipboard-snapshot-fixture-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let encoded_snapshot = crate::sync::v1::wire::encode_snapshot_pack(
        &directory,
        &snapshot_header,
        std::iter::empty::<MutationBatch>(),
        None,
    )
    .unwrap();
    let snapshot_key = snapshot_object_key(device_id, epoch, &encoded_snapshot.sha256).unwrap();
    let snapshot_ref = ObjectRef {
        key: snapshot_key.clone(),
        sha256: encoded_snapshot.sha256.clone(),
        stored_size_bytes: encoded_snapshot.stored_size_bytes,
        record_count: 0,
    };
    store
        .objects
        .lock()
        .unwrap()
        .insert(snapshot_key, fs::read(encoded_snapshot.path()).unwrap());
    drop(encoded_snapshot);
    let _ = fs::remove_dir(&directory);

    let mut last_segment_key = None;
    let mut thousandth_segment_key = None;
    for sequence in 1..=SEGMENT_COUNT {
        let segment = Segment {
            device_id: device_id.to_string(),
            epoch: epoch.to_string(),
            first_sequence: sequence,
            last_sequence: sequence,
            mutations: crate::sync::v1::MutationBatch {
                upserts: vec![replicated_text(
                    &format!("segment-{sequence}"),
                    &format!("segment {sequence}"),
                    sequence as i64,
                    device_id,
                )],
                tombstones: Vec::new(),
            },
        };
        let encoded = encode_segment(&segment, None).unwrap();
        let key =
            segment_object_key(device_id, epoch, sequence, sequence, &encoded.sha256).unwrap();
        if sequence == 1_000 {
            thousandth_segment_key = Some(key.clone());
        }
        store
            .objects
            .lock()
            .unwrap()
            .insert(key.clone(), encoded.bytes);
        last_segment_key = Some(key);
    }
    let head = DeviceHead {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        snapshot: snapshot_ref,
        published_sequence: SEGMENT_COUNT,
        last_segment_key: last_segment_key.clone(),
        updated_at_ms: 1,
    };
    let mut result = SyncEngineResult::default();

    pull_device(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        &head,
        None,
        options().resource_limits,
        &mut result,
    )
    .unwrap();

    assert_eq!(result.downloaded_entries, SEGMENT_COUNT);
    assert_eq!(result.applied_entries, SEGMENT_COUNT);
    assert!(database
        .get_item(&format!("segment-{SEGMENT_COUNT}"))
        .unwrap()
        .is_some());
    assert_eq!(
        database
            .get_sync_cursor(REMOTE_SCOPE, device_id)
            .unwrap()
            .unwrap()
            .sequence,
        SEGMENT_COUNT
    );

    let deleted = engine::test_support::garbage_collect_covered_history(
        &store,
        &[DeviceCursor {
            device_id: device_id.to_string(),
            epoch: epoch.to_string(),
            sequence: 1_000,
            last_segment_key: thousandth_segment_key,
        }],
    )
    .unwrap();
    assert_eq!(deleted, 1_001);
    assert!(store
        .objects
        .lock()
        .unwrap()
        .contains_key(last_segment_key.as_ref().unwrap()));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn garbage_collection_tolerates_non_segment_keys() {
    let store = MemoryStore::default();
    let device_id = "33333333-3333-4333-8333-333333333333";
    let epoch = "44444444-4444-4444-8444-444444444444";

    let segment = Segment {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        first_sequence: 1,
        last_sequence: 1,
        mutations: MutationBatch {
            upserts: vec![replicated_text("gc-1", "gc", 1, device_id)],
            tombstones: Vec::new(),
        },
    };
    let encoded = encode_segment(&segment, None).unwrap();
    let key = segment_object_key(device_id, epoch, 1, 1, &encoded.sha256).unwrap();
    let stray_key = format!("{}README", segment_prefix(device_id, epoch).unwrap());
    {
        let mut objects = store.objects.lock().unwrap();
        objects.insert(key.clone(), encoded.bytes);
        objects.insert(stray_key.clone(), b"not a segment".to_vec());
    }

    let deleted = engine::test_support::garbage_collect_covered_history(
        &store,
        &[DeviceCursor {
            device_id: device_id.to_string(),
            epoch: epoch.to_string(),
            sequence: 1,
            last_segment_key: None,
        }],
    )
    .unwrap();

    assert_eq!(deleted, 1);
    let objects = store.objects.lock().unwrap();
    assert!(!objects.contains_key(&key));
    assert!(objects.contains_key(&stray_key));
}
