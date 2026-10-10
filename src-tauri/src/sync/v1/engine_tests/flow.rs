//! Snapshot/segment convergence, listing caches and head-state scenarios.

use super::*;

#[test]
fn independent_snapshots_and_incremental_segments_converge() {
    let store = MemoryStore::default();
    store
        .objects
        .lock()
        .unwrap()
        .insert("baseline-old.zip".to_string(), b"old".to_vec());
    let first_paths = temp_paths("first");
    let second_paths = temp_paths("second");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    first.save_item(&text_item("first-item", "first")).unwrap();
    second
        .save_item(&text_item("second-item", "second"))
        .unwrap();

    let first_run =
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert_eq!(first_run.deleted_remote_objects, 1);
    let second_run = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(second_run.applied_entries >= 1);
    assert!(second.get_item("first-item").unwrap().is_some());

    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert!(first.get_item("second-item").unwrap().is_some());

    first.save_item(&text_item("incremental", "new")).unwrap();
    let pushed =
        sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert_eq!(pushed.uploaded_entries, 1);
    let pulled = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(pulled.downloaded_entries, 1);
    assert!(second.get_item("incremental").unwrap().is_some());

    let idle = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(idle.uploaded_entries, 0);
    assert_eq!(idle.downloaded_entries, 0);
    assert_eq!(idle.bytes_uploaded, 0);

    drop(first);
    drop(second);
    fs::remove_dir_all(first_paths.project).unwrap();
    fs::remove_dir_all(second_paths.project).unwrap();
}

#[test]
fn pull_tolerates_a_stray_key_under_a_peer_segment_prefix() {
    let store = MemoryStore::default();
    let first_paths = temp_paths("stray-pull-first");
    let second_paths = temp_paths("stray-pull-second");
    let first = Database::open(&first_paths.database).unwrap();
    let second = Database::open(&second_paths.database).unwrap();
    first.save_item(&text_item("first-item", "first")).unwrap();

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
    assert!(second.get_item("first-item").unwrap().is_some());

    // Publish one more segment, then drop a key that sorts just below it under
    // the same prefix (a foreign tool object / truncation). One stray object
    // must not abort the entire device pull.
    first
        .save_item(&text_item("incremental", "second"))
        .unwrap();
    sync_database(&store, &first, &first_paths, REMOTE_SCOPE, None, options()).unwrap();

    let newest_segment = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with("v1/segments/"))
        .max()
        .cloned()
        .expect("a segment key exists");
    let stray = format!(
        "{}.pac",
        newest_segment
            .strip_suffix(".pack")
            .expect("segment keys end in .pack")
    );
    store
        .objects
        .lock()
        .unwrap()
        .insert(stray.clone(), b"stray".to_vec());

    let pulled = sync_database(
        &store,
        &second,
        &second_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(pulled.applied_entries >= 1);
    assert!(second.get_item("incremental").unwrap().is_some());

    drop(first);
    drop(second);
    fs::remove_dir_all(first_paths.project).unwrap();
    fs::remove_dir_all(second_paths.project).unwrap();
}

#[test]
fn empty_peer_joins_without_rewriting_checkpoint_and_source_keeps_publishing() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("empty-peer-source");
    let target_paths = temp_paths("empty-peer-target");
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
    let checkpoint_before = store
        .objects
        .lock()
        .unwrap()
        .get(CHECKPOINT_HEAD_KEY)
        .cloned()
        .expect("source bootstrap should publish a checkpoint");
    store.puts.lock().unwrap().clear();
    store.deleted.lock().unwrap().clear();

    let joined = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(joined.uploaded_entries, 0);
    assert_eq!(joined.deleted_remote_objects, 0);
    assert_eq!(
        store
            .objects
            .lock()
            .unwrap()
            .get(CHECKPOINT_HEAD_KEY)
            .cloned(),
        Some(checkpoint_before)
    );
    assert!(!store
        .puts
        .lock()
        .unwrap()
        .iter()
        .any(|key| key == CHECKPOINT_HEAD_KEY || key.starts_with("v1/checkpoints/")));
    assert!(store.deleted.lock().unwrap().is_empty());

    source
        .save_item(&text_item("immediate-incremental", "incremental"))
        .unwrap();
    let published = sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert_eq!(published.uploaded_entries, 1);

    sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(target.get_item("immediate-incremental").unwrap().is_some());

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn ordinary_pull_defers_resource_download_and_can_republish_the_reference() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("deferred-source");
    let target_paths = temp_paths("deferred-target");
    fs::create_dir_all(&source_paths.images).unwrap();
    let source_image = source_paths.images.join("source.png");
    fs::write(&source_image, b"image-bytes").unwrap();
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source
        .save_item(&image_item("remote-image", &source_image))
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
    let resource_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .find(|key| key.starts_with("v1/resources/image/"))
        .cloned()
        .unwrap();
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
    assert_eq!(pulled.downloaded_resources, 0);
    assert!(!store.gets.lock().unwrap().contains(&resource_key));
    let local = target.get_item("remote-image").unwrap().unwrap();
    assert!(local.resource_path.is_none());
    assert!(local.preview_path.is_none());
    assert!(!local.metadata_json.unwrap().contains("v1/resources/"));

    let exported = target.export_sync_snapshot_for_scope(REMOTE_SCOPE).unwrap();
    let remote = exported
        .mutations
        .upserts
        .iter()
        .find(|item| item.item.id == "remote-image")
        .unwrap();
    assert_eq!(
        remote.item.resource_path.as_deref(),
        Some(resource_key.as_str())
    );
    assert!(remote.item.preview_path.is_none());
    assert!(remote
        .item
        .metadata_json
        .as_deref()
        .unwrap()
        .contains(resource_key.as_str()));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn restored_database_merges_its_remote_head_before_rotating_epoch() {
    let store = MemoryStore::default();
    let paths = temp_paths("restored-head");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let head_key = head_object_key(&device_id).unwrap();
    let backup_path = paths.project.join("restored.sqlite3");
    database.vacuum_into(&backup_path).unwrap();

    database
        .save_item(&text_item("remote-only", "remote-only"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let advanced_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_eq!(advanced_head.published_sequence, 1);
    drop(database);

    let restored = Database::open(&backup_path).unwrap();
    restored
        .save_item(&text_item("local-after-restore", "local-after-restore"))
        .unwrap();
    let healed = sync_database(&store, &restored, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(healed.downloaded_entries >= 2);
    assert_eq!(healed.uploaded_entries, 3);
    for id in ["initial", "remote-only", "local-after-restore"] {
        assert!(
            restored.get_item(id).unwrap().is_some(),
            "missing item {id}"
        );
    }
    let replacement_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_ne!(replacement_head.epoch, advanced_head.epoch);
    assert!(replacement_head.last_segment_key.is_none());
    assert_eq!(replacement_head.snapshot.record_count, 3);

    drop(restored);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn divergent_local_head_state_does_not_overwrite_remote_history() {
    let store = MemoryStore::default();
    let paths = temp_paths("divergent-head");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let device_id = database.get_sync_device_id().unwrap();
    let head_key = head_object_key(&device_id).unwrap();
    database
        .save_item(&text_item("remote-only", "remote-only"))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let advanced_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE sync_publication_state
                    SET published_sequence = 999,
                        last_segment_key = 'v1/segments/diverged.pack'
                  WHERE remote_scope = ?1",
                [REMOTE_SCOPE],
            )?;
            connection.execute("DELETE FROM clipboard_items WHERE id = 'remote-only'", [])?;
            connection.execute(
                "DELETE FROM sync_tombstones WHERE item_id = 'remote-only'",
                [],
            )?;
            connection.execute("DELETE FROM sync_outbox", [])?;
            Ok(())
        })
        .unwrap();

    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();

    assert!(database.get_item("remote-only").unwrap().is_some());
    let replacement_head =
        decode_device_head(store.objects.lock().unwrap().get(&head_key).unwrap(), None).unwrap();
    assert_ne!(replacement_head.epoch, advanced_head.epoch);
    assert_eq!(replacement_head.snapshot.record_count, 2);

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn resource_category_is_part_of_the_object_identity() {
    let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_ne!(
        crate::sync::v1::resource_object_key(ResourceCategory::Image, digest, "png").unwrap(),
        crate::sync::v1::resource_object_key(ResourceCategory::Icon, digest, "png").unwrap()
    );
}
