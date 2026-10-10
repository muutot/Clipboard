//! Encryption mode binding, resource privacy and wrong-password scenarios.

use super::*;

#[test]
fn encrypted_resource_stays_private_and_materializes_only_on_demand() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("encrypted-resource-source");
    let target_paths = temp_paths("encrypted-resource-target");
    fs::create_dir_all(&source_paths.images).unwrap();
    let source_image = source_paths.images.join("private.png");
    let plaintext = b"private-image-fragment".repeat(4096);
    fs::write(&source_image, &plaintext).unwrap();
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    source
        .save_item(&image_item("encrypted-image", &source_image))
        .unwrap();

    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        Some(&key),
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
    let stored_resource = store
        .objects
        .lock()
        .unwrap()
        .get(&resource_key)
        .cloned()
        .unwrap();
    assert_ne!(stored_resource, plaintext);
    assert!(!stored_resource
        .windows(b"private-image-fragment".len())
        .any(|window| window == b"private-image-fragment"));
    store.gets.lock().unwrap().clear();

    let pulled = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();
    assert_eq!(pulled.downloaded_resources, 0);
    assert!(!store.gets.lock().unwrap().contains(&resource_key));
    assert!(target
        .get_item("encrypted-image")
        .unwrap()
        .unwrap()
        .resource_path
        .is_none());

    let exported = target.export_sync_snapshot_for_scope(REMOTE_SCOPE).unwrap();
    let forwarded = exported
        .mutations
        .upserts
        .iter()
        .find(|item| item.item.id == "encrypted-image")
        .unwrap();
    assert_eq!(
        forwarded.item.resource_path.as_deref(),
        Some(resource_key.as_str())
    );

    let materialized = materialize_resource(
        &store,
        &resource_key,
        &target_paths.images,
        options().resource_limits.image_bytes,
        Some(&key),
    )
    .unwrap();
    assert_eq!(fs::read(materialized.path).unwrap(), plaintext);
    assert_eq!(materialized.transferred_bytes, stored_resource.len() as u64);

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn wrong_password_is_rejected_before_a_new_device_publishes() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("password-source");
    let target_paths = temp_paths("password-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("private", "private")).unwrap();
    let right = SessionKey::derive("right", REMOTE_SCOPE).unwrap();
    let wrong = SessionKey::derive("wrong", REMOTE_SCOPE).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        Some(&right),
        options(),
    )
    .unwrap();
    let target_device_id = target.get_sync_device_id().unwrap();
    let target_head_key = head_object_key(&target_device_id).unwrap();
    store.puts.lock().unwrap().clear();

    let error = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&wrong),
        options(),
    )
    .unwrap_err();
    assert!(error.contains("cannot authenticate the existing sync v1 namespace"));
    assert!(store.puts.lock().unwrap().is_empty());
    assert!(!store.objects.lock().unwrap().contains_key(&target_head_key));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn encryption_mode_change_is_rejected_before_a_new_device_publishes() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("plaintext-source");
    let target_paths = temp_paths("encrypted-target");
    let source = Database::open(&source_paths.database).unwrap();
    let target = Database::open(&target_paths.database).unwrap();
    source.save_item(&text_item("public", "public")).unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    let target_device_id = target.get_sync_device_id().unwrap();
    let target_head_key = head_object_key(&target_device_id).unwrap();
    store.puts.lock().unwrap().clear();

    let error = sync_database(
        &store,
        &target,
        &target_paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap_err();
    assert!(error.contains("namespace encryption mode does not match"));
    assert!(store.puts.lock().unwrap().is_empty());
    assert!(!store.objects.lock().unwrap().contains_key(&target_head_key));

    drop(source);
    drop(target);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn encrypted_retry_produces_one_immutable_segment_object() {
    let store = MemoryStore::default();
    let paths = temp_paths("encrypted");
    let database = Database::open(&paths.database).unwrap();
    database
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    let key = SessionKey::derive("password", REMOTE_SCOPE).unwrap();
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();
    database.save_item(&text_item("later", "later")).unwrap();
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        Some(&key),
        options(),
    )
    .unwrap();

    let device_id = database.get_sync_device_id().unwrap();
    let state = database
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let prefix = segment_prefix(&device_id, &state.epoch).unwrap();
    let segment_count = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .count();
    assert_eq!(segment_count, 1);

    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}
