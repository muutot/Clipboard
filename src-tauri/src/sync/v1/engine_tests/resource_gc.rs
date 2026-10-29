use super::*;
use std::sync::Arc;

fn setup_image(label: &str) -> (MemoryStore, StoragePaths, Database, String) {
    let store = MemoryStore::default();
    let paths = temp_paths(label);
    let database = Database::open(&paths.database).unwrap();
    let image_path = paths.images.join("resource-gc-source.png");
    fs::write(&image_path, b"resource-gc-image-bytes").unwrap();
    database
        .save_item(&image_item("image", &image_path))
        .unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let resource_key = store
        .objects
        .lock()
        .unwrap()
        .keys()
        .find(|key| key.starts_with("v1/resources/image/"))
        .cloned()
        .expect("the bootstrap must upload the image resource");
    (store, paths, database, resource_key)
}

fn insert_orphan(store: &MemoryStore) -> String {
    let key = resource_object_key(ResourceCategory::Image, &"b".repeat(64), "png").unwrap();
    store
        .put(
            &key,
            b"unreferenced-resource".to_vec(),
            PutCondition::Unconditional,
        )
        .unwrap();
    key
}

/// Stamps the live resources with a fresh store timestamp and the orphan with
/// one past the grace window: the GC anchors its cutoff to the store's own
/// newest stamp, so one shared fake stamp cannot model both at once.
fn stamp_orphan_past_grace(store: &MemoryStore, orphan: &str) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    *store.list_modified_ms.lock().unwrap() = Some(now_ms);
    store
        .modified_ms_overrides
        .lock()
        .unwrap()
        .insert(orphan.to_owned(), now_ms - 8 * 24 * 60 * 60 * 1000);
}

fn compact(
    store: &MemoryStore,
    database: &Database,
    paths: &StoragePaths,
    mode: ResourceGcMode,
) -> SyncEngineResult {
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
    let mut result = SyncEngineResult::default();
    maybe_compact(
        store,
        database,
        paths,
        REMOTE_SCOPE,
        &database.get_sync_device_id().unwrap(),
        &state,
        None,
        options().resource_limits,
        mode,
        &mut result,
    )
    .unwrap();
    result
}

#[test]
fn reference_scan_retains_old_resources_that_a_peer_may_reuse() {
    let (store, paths, database, resource_key) = setup_image("resource-gc-scan");
    let orphan = insert_orphan(&store);
    stamp_orphan_past_grace(&store, &orphan);
    store.deleted.lock().unwrap().clear();

    compact(&store, &database, &paths, ResourceGcMode::ReferenceScan);
    assert!(!store.deleted.lock().unwrap().contains(&orphan));
    let objects = store.objects.lock().unwrap();
    assert!(objects.contains_key(&orphan));
    assert!(objects.contains_key(&resource_key));

    drop(objects);
    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn manifest_mode_uses_the_checkpoint_manifest_without_reading_the_pack() {
    let (store, paths, database, resource_key) = setup_image("resource-gc-manifest");
    database
        .save_item(&text_item("manifest-later", "manifest-later"))
        .unwrap();
    let mut manifest_options = options();
    manifest_options.resource_gc_mode = ResourceGcMode::Manifest;
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        None,
        manifest_options,
    )
    .unwrap();
    let orphan = insert_orphan(&store);
    stamp_orphan_past_grace(&store, &orphan);
    store.deleted.lock().unwrap().clear();
    store.gets.lock().unwrap().clear();

    compact(&store, &database, &paths, ResourceGcMode::Manifest);

    assert!(!store.deleted.lock().unwrap().contains(&orphan));
    let objects = store.objects.lock().unwrap();
    assert!(objects.contains_key(&orphan));
    assert!(objects.contains_key(&resource_key));
    assert!(objects
        .keys()
        .any(|key| key.starts_with("v1/resource-refs/")));
    drop(objects);
    let gets = store.gets.lock().unwrap();
    assert!(gets.iter().any(|key| key.starts_with("v1/resource-refs/")));

    drop(gets);
    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn fresh_resources_are_retained_without_relying_on_timestamps() {
    let (store, paths, database, resource_key) = setup_image("resource-gc-grace");
    let orphan = insert_orphan(&store);
    *store.list_modified_ms.lock().unwrap() = Some(chrono::Utc::now().timestamp_millis());
    store.deleted.lock().unwrap().clear();

    compact(&store, &database, &paths, ResourceGcMode::ReferenceScan);

    assert!(!store.deleted.lock().unwrap().contains(&orphan));
    let objects = store.objects.lock().unwrap();
    assert!(objects.contains_key(&orphan));
    assert!(objects.contains_key(&resource_key));

    drop(objects);
    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn manifest_mode_falls_back_to_reference_scan_when_sidecars_are_missing() {
    let (store, paths, database, resource_key) = setup_image("resource-gc-manifest-fallback");
    database
        .save_item(&text_item("fallback-later", "fallback-later"))
        .unwrap();
    let mut manifest_options = options();
    manifest_options.resource_gc_mode = ResourceGcMode::Manifest;
    sync_database(
        &store,
        &database,
        &paths,
        REMOTE_SCOPE,
        None,
        manifest_options,
    )
    .unwrap();
    store
        .objects
        .lock()
        .unwrap()
        .retain(|key, _| !key.starts_with("v1/resource-refs/"));
    let orphan = insert_orphan(&store);
    stamp_orphan_past_grace(&store, &orphan);
    store.deleted.lock().unwrap().clear();

    compact(&store, &database, &paths, ResourceGcMode::Manifest);

    assert!(!store.deleted.lock().unwrap().contains(&orphan));
    let objects = store.objects.lock().unwrap();
    assert!(objects.contains_key(&orphan));
    assert!(objects.contains_key(&resource_key));

    drop(objects);
    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn concurrent_publication_keeps_a_reused_old_resource_downloadable() {
    for mode in [ResourceGcMode::ReferenceScan, ResourceGcMode::Manifest] {
        let (store, paths, database, _) = setup_image("resource-gc-reuse");
        let store = Arc::new(store);
        let bytes = b"old resource reused by a new peer";
        let resource_key = resource_object_key(
            ResourceCategory::Image,
            &hex::encode(Sha256::digest(bytes)),
            "png",
        )
        .unwrap();
        store
            .put(&resource_key, bytes.to_vec(), PutCondition::IfAbsent)
            .unwrap();
        stamp_orphan_past_grace(&store, &resource_key);

        let peer_paths = Arc::new(temp_paths("resource-gc-reuse-peer"));
        let peer_database = Arc::new(Database::open(&peer_paths.database).unwrap());
        let peer_image = peer_paths.images.join("reused.png");
        fs::write(&peer_image, bytes).unwrap();
        peer_database
            .save_item(&image_item("peer-reused-image", &peer_image))
            .unwrap();
        let publishing_store = Arc::clone(&store);
        let publishing_paths = Arc::clone(&peer_paths);
        let publishing_database = Arc::clone(&peer_database);
        *store.after_list.lock().unwrap() = Some((
            "v1/segments/".to_owned(),
            Box::new(move || {
                sync_database(
                    publishing_store.as_ref(),
                    publishing_database.as_ref(),
                    publishing_paths.as_ref(),
                    REMOTE_SCOPE,
                    None,
                    options(),
                )
                .unwrap();
            }),
        ));

        // Publish after the last pack listing in the reference scan, using
        // HEAD to reuse the old object instead of refreshing its timestamp.
        compact(store.as_ref(), &database, &paths, mode);
        assert!(store.after_list.lock().unwrap().is_none());
        let downloaded = clipboard_sync::v1::resources::materialize_resource(
            store.as_ref(),
            &resource_key,
            &paths.storage.join("resource-gc-download"),
            1024,
            None,
        )
        .unwrap();
        assert_eq!(fs::read(downloaded.path).unwrap(), bytes);
        assert!(!store.deleted.lock().unwrap().contains(&resource_key));

        drop(peer_database);
        fs::remove_dir_all(&peer_paths.project).unwrap();
        drop(database);
        fs::remove_dir_all(paths.project).unwrap();
    }
}
