//! Checkpoint recovery, listing staleness and idle-sync scenarios.

use super::*;

#[test]
fn corrupt_peer_head_does_not_block_other_devices() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("isolated-source");
    let target_paths = temp_paths("isolated-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source
        .save_item(&text_item("healthy-peer-item", "healthy"))
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
    store.objects.lock().unwrap().insert(
        "v1/heads/00000000-0000-4000-8000-000000000000.bin".to_string(),
        b"corrupt-head".to_vec(),
    );

    let result = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert_eq!(result.failed_peers, 1);
    assert!(result.applied_entries >= 1);
    assert!(target.get_item("healthy-peer-item").unwrap().is_some());

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn empty_device_bootstraps_from_checkpoint_without_peer_history() {
    let store = MemoryStore::default();
    let source_device = "11111111-1111-4111-8111-111111111111";
    let source_epoch = "22222222-2222-4222-8222-222222222222";
    let checkpoint = CheckpointFixture {
        generation: 1,
        vector: vec![DeviceCursor {
            device_id: source_device.to_string(),
            epoch: source_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        }],
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "checkpoint-only",
                "checkpoint",
                100,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let reference = insert_checkpoint(&store, &checkpoint);
    insert_checkpoint_head(&store, &checkpoint, reference, None);
    let paths = temp_paths("checkpoint-bootstrap");
    let database = Database::open(&paths.database).unwrap();

    let result = sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(result.downloaded_entries, 1);
    assert_eq!(result.applied_entries, 1);
    assert!(database.get_item("checkpoint-only").unwrap().is_some());
    assert!(database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .is_some_and(|(generation, _)| generation >= 1));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn corrupt_current_checkpoint_falls_back_to_previous_generation() {
    let store = MemoryStore::default();
    let source_device = "11111111-1111-4111-8111-111111111111";
    let source_epoch = "22222222-2222-4222-8222-222222222222";
    let previous = CheckpointFixture {
        generation: 1,
        vector: vec![DeviceCursor {
            device_id: source_device.to_string(),
            epoch: source_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        }],
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "previous-only",
                "previous",
                100,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let previous_ref = insert_checkpoint(&store, &previous);
    let current = CheckpointFixture {
        generation: 2,
        vector: previous.vector.clone(),
        mutations: crate::sync::v1::MutationBatch {
            upserts: vec![replicated_text(
                "current-only",
                "current",
                200,
                source_device,
            )],
            tombstones: Vec::new(),
        },
    };
    let current_ref = insert_checkpoint(&store, &current);
    store
        .objects
        .lock()
        .unwrap()
        .insert(current_ref.key.clone(), b"corrupt".to_vec());
    insert_checkpoint_head(&store, &current, current_ref, Some(previous_ref.clone()));
    let paths = temp_paths("checkpoint-fallback");
    let database = Database::open(&paths.database).unwrap();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(database.get_item("previous-only").unwrap().is_some());
    assert!(database.get_item("current-only").unwrap().is_none());
    assert!(database
        .get_sync_checkpoint_state(REMOTE_SCOPE)
        .unwrap()
        .is_some_and(|(generation, _)| generation >= 1));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn idle_sync_does_not_read_or_publish_a_checkpoint() {
    let store = MemoryStore::default();
    let paths = temp_paths("checkpoint-idle");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    store.gets.lock().unwrap().clear();
    store.puts.lock().unwrap().clear();

    let idle = sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.bytes_uploaded, 0);
    assert_eq!(idle.deleted_remote_objects, 0);
    assert!(!store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY));
    assert!(!store
        .puts
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY || key.starts_with("v1/checkpoints/")));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn idle_sync_uses_one_head_listing_and_zero_head_gets_after_cache_warmup() {
    let store = MemoryStore::default();
    let first_paths = temp_paths("head-cache-first");
    let second_paths = temp_paths("head-cache-second");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    first.save_item(&text_item("first", "first")).unwrap();
    second.save_item(&text_item("second", "second")).unwrap();

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
    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    store.gets.lock().unwrap().clear();
    store.lists.lock().unwrap().clear();

    let idle = sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.downloaded_entries, 0);
    assert_eq!(idle.bytes_downloaded, 0);
    assert_eq!(
        store
            .lists
            .lock()
            .unwrap()
            .iter()
            .filter(|prefix| prefix.as_str() == HEADS_PREFIX)
            .count(),
        1
    );
    assert!(!store
        .gets
        .lock()
        .unwrap()
        .iter()
        .any(|key| key.starts_with(HEADS_PREFIX)));

    drop(first);
    drop(second);
    fs::remove_dir_all(first_paths.project).unwrap();
    fs::remove_dir_all(second_paths.project).unwrap();
}

#[test]
fn changed_head_etag_invalidates_cache_and_applies_the_new_segment() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("head-cache-change-source");
    let target_paths = temp_paths("head-cache-change-target");
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
    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    source.save_item(&text_item("later", "later")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let source_head = head_object_key(&source.get_sync_device_id().unwrap()).unwrap();
    store.gets.lock().unwrap().clear();

    let pulled = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();

    assert_eq!(pulled.downloaded_entries, 1);
    assert!(target.get_item("later").unwrap().is_some());
    assert!(store.gets.lock().unwrap().contains(&source_head));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn missing_list_etag_falls_back_to_head_gets() {
    let store = MemoryStore::default();
    let paths = temp_paths("head-cache-no-etag");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let local_head = head_object_key(&database.get_sync_device_id().unwrap()).unwrap();
    *store.list_without_etags.lock().unwrap() = true;
    store.gets.lock().unwrap().clear();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(store.gets.lock().unwrap().contains(&local_head));

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}
