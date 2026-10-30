//! Orphaned upload adoption and leftover cleanup scenarios.

use super::*;

#[test]
fn migrated_database_and_remaining_old_instance_keep_both_publications_discoverable() {
    use std::sync::Arc;
    let store = Arc::new(MemoryStore::default());
    let paths = temp_paths("migration-sync-identity");
    let peer_paths = temp_paths("migration-sync-identity-peer");
    let migrating = Database::open(&paths.database).unwrap();
    migrating
        .save_item(&text_item("initial", "initial"))
        .unwrap();
    sync_database(&*store, &migrating, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let older_instance = Database::open(&paths.database).unwrap();
    let original_device = older_instance.get_sync_device_id().unwrap();
    let target_paths = StoragePaths::initialize_with_resource_directories_for_configuration(
        paths.project.clone(),
        Some(paths.project.join("new-data")),
        None,
        None,
    )
    .unwrap();
    let reservation = migrating.begin_storage_migration().unwrap();
    crate::commands::config::storage::migrate_storage_data(&paths, &target_paths, &migrating)
        .unwrap();
    reservation.keep_until_restart();
    drop(migrating); // The migrating app restarts; an older instance remains alive.
    let target = Arc::new(Database::open(&target_paths.database).unwrap());
    let target_device = target.get_sync_device_id().unwrap();
    assert_ne!(target_device, original_device);
    assert_eq!(
        older_instance.get_sync_device_id().unwrap(),
        original_device
    );
    older_instance
        .save_item(&text_item("old-instance", "old-instance"))
        .unwrap();
    let hook_store = store.clone();
    let hook_target = target.clone();
    let hook_paths = target_paths.clone();
    *store.after_put.lock().unwrap() = Some((
        "/segments/".into(),
        Box::new(move || {
            hook_target
                .save_item(&text_item("new-instance", "new-instance"))
                .unwrap();
            sync_database(
                &*hook_store,
                &hook_target,
                &hook_paths,
                REMOTE_SCOPE,
                None,
                options(),
            )
            .unwrap();
        }),
    ));
    sync_database(
        &*store,
        &older_instance,
        &paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let old_device = older_instance.get_sync_device_id().unwrap();
    let head = decode_device_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(&head_object_key(&old_device).unwrap())
            .unwrap(),
        None,
    )
    .unwrap();
    let referenced_segment_exists = head
        .last_segment_key
        .as_ref()
        .is_some_and(|key| store.objects.lock().unwrap().contains_key(key));
    assert_eq!(older_instance.count_sync_outbox().unwrap(), 0);
    assert_eq!(target.count_sync_outbox().unwrap(), 0);
    // An idle retry must not hide the fact that the successful head references
    // a segment another clone discarded as an unrelated orphan.
    sync_database(
        &*store,
        &older_instance,
        &paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();
    let pulled = sync_database(&*store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    let old_present = peer.get_item("old-instance").unwrap().is_some();
    let new_present = peer.get_item("new-instance").unwrap().is_some();
    drop(peer);
    drop(target);
    let reopened = Database::open(&target_paths.database).unwrap();
    assert_eq!(reopened.get_sync_device_id().unwrap(), target_device);
    drop(reopened);
    drop(older_instance);
    fs::remove_dir_all(paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
    assert!(
        referenced_segment_exists,
        "the migrated clone deleted an in-flight segment still referenced by a successful head"
    );
    assert_eq!(pulled.failed_peers, 0);
    assert!(
        old_present && new_present,
        "both independently successful publications must reach a fresh peer"
    );
}

#[test]
fn overlapping_database_connections_cannot_regress_the_published_head() {
    use std::sync::Arc;
    let store = Arc::new(MemoryStore::default());
    let paths = temp_paths("overlapping-sync-source");
    let peer_paths = temp_paths("overlapping-sync-peer");
    let source = Database::open(&paths.database).unwrap();
    let other = Arc::new(Database::open(&paths.database).unwrap());
    source.save_item(&text_item("initial", "initial")).unwrap();
    sync_database(&*store, &source, &paths, REMOTE_SCOPE, None, options()).unwrap();
    source.save_item(&text_item("first", "first")).unwrap();
    let other_result = Arc::new(Mutex::new(None));
    let hook_result = other_result.clone();
    let hook_other = other.clone();
    let hook_store = store.clone();
    let hook_paths = paths.clone();
    *store.after_put.lock().unwrap() = Some((
        "/segments/".into(),
        Box::new(move || {
            hook_other
                .save_item(&text_item("second", "second"))
                .unwrap();
            *hook_result.lock().unwrap() = Some(sync_database(
                &*hook_store,
                &hook_other,
                &hook_paths,
                REMOTE_SCOPE,
                None,
                options(),
            ));
        }),
    ));
    let first_result = sync_database(&*store, &source, &paths, REMOTE_SCOPE, None, options());
    let overlap = other_result.lock().unwrap().take().unwrap();
    if let Err(error) = overlap {
        assert!(
            error.contains("already in progress"),
            "unexpected overlap failure: {error}"
        );
        assert!(first_result.is_ok());
        // The second caller retries after the first run releases its lease.
        sync_database(&*store, &other, &paths, REMOTE_SCOPE, None, options()).unwrap();
    }
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let device = source.get_sync_device_id().unwrap();
    let head = decode_device_head(
        store
            .objects
            .lock()
            .unwrap()
            .get(&head_object_key(&device).unwrap())
            .unwrap(),
        None,
    )
    .unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();
    sync_database(&*store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    let second_present = peer.get_item("second").unwrap().is_some();
    drop(peer);
    drop(source);
    drop(other);
    fs::remove_dir_all(paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
    assert_eq!(
        head.published_sequence, state.published_sequence,
        "a late earlier run must not overwrite a successful newer head"
    );
    assert!(
        second_present,
        "the successful second publication must remain discoverable"
    );
}

#[test]
fn restored_same_item_edit_is_not_acknowledged_by_an_old_orphan() {
    assert_restored_edit_survives_interrupted_publication(false, None);
}

#[test]
fn unavailable_sync_ownership_fails_before_remote_io_or_sync_initialization() {
    let store = MemoryStore::default();
    let paths = temp_paths("unavailable-sync-lock");
    let database = Database::open(&paths.database).unwrap();
    let lock = paths
        .database
        .with_file_name("clipboard.sqlite3.sync-run.lock");
    fs::create_dir(&lock).unwrap();
    assert!(sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).is_err());
    assert!(store.puts.lock().unwrap().is_empty());
    assert!(store.gets.lock().unwrap().is_empty());
    assert!(store.lists.lock().unwrap().is_empty());
    assert!(
        database.initialize_sync().unwrap(),
        "failed ownership must precede sync initialization"
    );
    fs::remove_dir(lock).unwrap();
    sync_database(&store, &database, &paths, REMOTE_SCOPE, None, options()).unwrap();
    drop(database);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn restored_same_item_edit_is_not_acknowledged_by_an_old_published_head() {
    assert_restored_edit_survives_interrupted_publication(true, None);
}

#[test]
fn encrypted_restored_same_item_edits_survive_both_interruption_windows() {
    let key = SessionKey::derive("fixture password", REMOTE_SCOPE).unwrap();
    for publish_head in [false, true] {
        assert_restored_edit_survives_interrupted_publication(publish_head, Some(&key));
    }
}

fn assert_restored_edit_survives_interrupted_publication(
    publish_head: bool,
    session_key: Option<&SessionKey>,
) {
    use crate::storage::TextItemUpdate;
    let store = MemoryStore::default();
    let paths = temp_paths("same-item-restored-source");
    let peer_paths = temp_paths("same-item-restored-peer");
    let source = Database::open(&paths.database).unwrap();
    source.save_item(&text_item("editable", "initial")).unwrap();
    sync_database(
        &store,
        &source,
        &paths,
        REMOTE_SCOPE,
        session_key,
        options(),
    )
    .unwrap();
    let backup = paths.project.join("backup.sqlite3");
    source.vacuum_into(&backup).unwrap();
    let edit = |database: &Database, text: &str, hash: &str| {
        assert!(database
            .update_text_item(&TextItemUpdate {
                id: "editable",
                kind: ClipboardKind::Text,
                title: text,
                text_content: text,
                content_hash: hash,
                size_bytes: text.len() as u64,
                metadata_json: None,
            })
            .unwrap());
    };
    edit(&source, "interrupted remote edit", "remote-edit-hash");
    let device = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .unwrap();
    let segment = Segment {
        device_id: device.clone(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let encoded = encode_segment(&segment, session_key).unwrap();
    let segment_key = segment_object_key(
        &device,
        &state.epoch,
        segment.first_sequence,
        segment.last_sequence,
        &encoded.sha256,
    )
    .unwrap();
    store
        .put(&segment_key, encoded.bytes, PutCondition::IfAbsent)
        .unwrap();
    if publish_head {
        let mut remote = state.clone();
        remote.published_sequence = segment.last_sequence;
        remote.last_segment_key = Some(segment_key.clone());
        let encoded =
            encode_device_head(&remote.device_head(&device).unwrap(), session_key).unwrap();
        store
            .put(
                &head_object_key(&device).unwrap(),
                encoded.bytes,
                PutCondition::Unconditional,
            )
            .unwrap();
    }
    drop(source);
    let restored = Database::open(&backup).unwrap();
    edit(
        &restored,
        "new local edit after restore",
        "restored-edit-hash",
    );
    let restored_batch = restored
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .unwrap();
    assert_eq!(restored_batch.first_sequence, segment.first_sequence);
    assert_eq!(restored_batch.last_sequence, segment.last_sequence);
    assert_eq!(
        restored_batch.mutations.upserts[0].item.id,
        segment.mutations.upserts[0].item.id
    );
    assert_ne!(restored_batch.mutations, segment.mutations);
    sync_database(
        &store,
        &restored,
        &paths,
        REMOTE_SCOPE,
        session_key,
        options(),
    )
    .unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();
    sync_database(
        &store,
        &peer,
        &peer_paths,
        REMOTE_SCOPE,
        session_key,
        options(),
    )
    .unwrap();
    let local_text = restored.get_item("editable").unwrap().unwrap().text_content;
    let peer_text = peer.get_item("editable").unwrap().unwrap().text_content;
    let pending = restored.count_sync_outbox().unwrap();
    drop(restored);
    drop(peer);
    fs::remove_dir_all(paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
    assert_eq!(local_text.as_deref(), Some("new local edit after restore"));
    assert_eq!(
        peer_text, local_text,
        "acknowledging an old segment must not drop an unrelated edit of the same item ID"
    );
    assert_eq!(pending, 0);
}

#[test]
fn interrupted_media_segment_remains_adoptable_with_canonical_resource_comparison() {
    use clipboard_sync::cancellation::CancellationToken;
    let encrypted_key = SessionKey::derive("fixture password", REMOTE_SCOPE).unwrap();
    for key in [None, Some(&encrypted_key)] {
        let store = MemoryStore::default();
        let paths = temp_paths("media-orphan-source");
        let peer_paths = temp_paths("media-orphan-peer");
        let source = Database::open(&paths.database).unwrap();
        source.save_item(&text_item("initial", "initial")).unwrap();
        sync_database(&store, &source, &paths, REMOTE_SCOPE, key, options()).unwrap();
        let previous = source
            .get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap();
        let image = paths.images.join("fixture.png");
        fs::write(&image, b"fixture image").unwrap();
        source.save_item(&image_item("media", &image)).unwrap();
        let token = CancellationToken::default();
        *store.cancel_after_key_fragment.lock().unwrap() =
            Some(("/segments/".into(), token.clone()));
        assert_eq!(
            engine::sync_database_cancellable(
                &store,
                &source,
                &engine_paths(&paths),
                REMOTE_SCOPE,
                key,
                options(),
                &token,
            )
            .unwrap_err(),
            "sync cancelled"
        );
        *store.cancel_after_key_fragment.lock().unwrap() = None;
        let orphan = store
            .objects
            .lock()
            .unwrap()
            .keys()
            .find(|key| key.contains("/segments/"))
            .unwrap()
            .clone();
        sync_database(&store, &source, &paths, REMOTE_SCOPE, key, options()).unwrap();
        let recovered = source
            .get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap();
        assert_eq!(recovered.epoch, previous.epoch);
        assert_eq!(recovered.last_segment_key.as_deref(), Some(orphan.as_str()));
        assert!(store.objects.lock().unwrap().contains_key(&orphan));
        assert_eq!(source.count_sync_outbox().unwrap(), 0);
        let peer = Database::open(&peer_paths.database).unwrap();
        sync_database(&store, &peer, &peer_paths, REMOTE_SCOPE, key, options()).unwrap();
        assert!(peer.get_item("media").unwrap().is_some());
        assert_eq!(
            peer.get_sync_resource_refs(REMOTE_SCOPE, "media").unwrap(),
            source
                .get_sync_resource_refs(REMOTE_SCOPE, "media")
                .unwrap()
        );
        drop(source);
        drop(peer);
        fs::remove_dir_all(paths.project).unwrap();
        fs::remove_dir_all(peer_paths.project).unwrap();
    }
}

#[test]
fn interrupted_segment_upload_orphan_is_adopted_without_breaking_peers() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("orphan-source");
    let peer_paths = temp_paths("orphan-peer");
    let source = Database::open(&source_paths.database).unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();

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
    source
        .save_item(&text_item("published", "published"))
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

    // Simulate a crash after `put_immutable` but before `publish_head`:
    // the next outbox row is uploaded as a segment object while the head
    // and the outbox commit stay behind.
    source
        .save_item(&text_item("orphaned", "orphaned"))
        .unwrap();
    let device_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let expected_first = state.published_sequence + 1;
    let batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 16)
        .unwrap()
        .unwrap();
    assert_eq!(batch.first_sequence, expected_first);
    let segment = Segment {
        device_id: device_id.clone(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let encoded = encode_segment(&segment, None).unwrap();
    let orphan_key = segment_object_key(
        &device_id,
        &state.epoch,
        segment.first_sequence,
        segment.last_sequence,
        &encoded.sha256,
    )
    .unwrap();
    store
        .put(&orphan_key, encoded.bytes, PutCondition::IfAbsent)
        .unwrap();

    // The retry finds more outbox rows than the orphan covers; adoption
    // must drain the outbox through the orphan instead of re-encoding a
    // segment whose range overlaps it.
    source.save_item(&text_item("live", "live")).unwrap();
    let healed = sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    assert!(source.get_item("live").unwrap().is_some());
    assert!(store.objects.lock().unwrap().contains_key(&orphan_key));

    // A fresh peer converges through the repaired contiguous chain.
    sync_database(&store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    for id in ["initial", "published", "orphaned", "live"] {
        assert!(peer.get_item(id).unwrap().is_some(), "missing item {id}");
    }
    assert!(healed.uploaded_entries >= 2);

    drop(source);
    drop(peer);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
}

#[test]
fn published_head_without_local_commit_is_adopted_in_place() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("head-commit-crash-source");
    let peer_paths = temp_paths("head-commit-crash-peer");
    let source = Database::open(&source_paths.database).unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();

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
    source
        .save_item(&text_item("published", "published"))
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

    // Simulate a crash between `publish_head` and the local publication
    // commit: the remote head already points at the uploaded segment while
    // the local publication state and the outbox commit stay behind.
    source
        .save_item(&text_item("uncommitted", "uncommitted"))
        .unwrap();
    let device_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 16)
        .unwrap()
        .unwrap();
    assert_eq!(batch.first_sequence, state.published_sequence + 1);
    let segment = Segment {
        device_id: device_id.clone(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let encoded = encode_segment(&segment, None).unwrap();
    let segment_key = segment_object_key(
        &device_id,
        &state.epoch,
        segment.first_sequence,
        segment.last_sequence,
        &encoded.sha256,
    )
    .unwrap();
    store
        .put(&segment_key, encoded.bytes, PutCondition::IfAbsent)
        .unwrap();
    let mut crashed_state = state.clone();
    crashed_state.published_sequence = segment.last_sequence;
    crashed_state.last_segment_key = Some(segment_key.clone());
    crashed_state.updated_at_ms = 1;
    let head = crashed_state.device_head(&device_id).unwrap();
    let head_encoded = encode_device_head(&head, None).unwrap();
    let head_key = head_object_key(&device_id).unwrap();
    store
        .put(&head_key, head_encoded.bytes, PutCondition::Unconditional)
        .unwrap();

    // Recovery must adopt the remote head in the same epoch instead of
    // re-pulling our own segment and rotating the epoch, which would force
    // every peer into a full re-bootstrap.
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        None,
        options(),
    )
    .unwrap();
    let recovered = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    assert_eq!(recovered.epoch, state.epoch);
    assert_eq!(recovered.published_sequence, segment.last_sequence);
    assert_eq!(
        recovered.last_segment_key.as_deref(),
        Some(segment_key.as_str())
    );
    assert!(source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .is_none());
    assert!(source.get_item("uncommitted").unwrap().is_some());

    // A fresh peer converges through the adopted chain.
    sync_database(&store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    for id in ["initial", "published", "uncommitted"] {
        assert!(peer.get_item(id).unwrap().is_some(), "missing item {id}");
    }

    drop(source);
    drop(peer);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
}

#[test]
fn partially_overlapping_orphan_leftover_is_deleted_during_adoption() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("orphan-leftover-source");
    let peer_paths = temp_paths("orphan-leftover-peer");
    let source = Database::open(&source_paths.database).unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();

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
    source
        .save_item(&text_item("published", "published"))
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

    // First interrupted publish leaves the shorter orphan [2..2].
    source
        .save_item(&text_item("orphaned", "orphaned"))
        .unwrap();
    let device_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let expected_first = state.published_sequence + 1;
    let batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 16)
        .unwrap()
        .unwrap();
    assert_eq!(batch.first_sequence, expected_first);
    let orphan_segment = Segment {
        device_id: device_id.clone(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let orphan_encoded = encode_segment(&orphan_segment, None).unwrap();
    let orphan_key = segment_object_key(
        &device_id,
        &state.epoch,
        orphan_segment.first_sequence,
        orphan_segment.last_sequence,
        &orphan_encoded.sha256,
    )
    .unwrap();
    store
        .put(&orphan_key, orphan_encoded.bytes, PutCondition::IfAbsent)
        .unwrap();

    // A legacy retry then uploaded a longer overlapping segment [2..3]
    // that can never continue the chain once [2..2] is adopted.
    source.save_item(&text_item("live", "live")).unwrap();
    let wider_batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 16)
        .unwrap()
        .unwrap();
    assert_eq!(wider_batch.first_sequence, expected_first);
    assert!(wider_batch.last_sequence > orphan_segment.last_sequence);
    let wider_segment = Segment {
        device_id: device_id.clone(),
        epoch: state.epoch.clone(),
        first_sequence: wider_batch.first_sequence,
        last_sequence: wider_batch.last_sequence,
        mutations: wider_batch.mutations,
    };
    let wider_encoded = encode_segment(&wider_segment, None).unwrap();
    let wider_key = segment_object_key(
        &device_id,
        &state.epoch,
        wider_segment.first_sequence,
        wider_segment.last_sequence,
        &wider_encoded.sha256,
    )
    .unwrap();
    store
        .put(&wider_key, wider_encoded.bytes, PutCondition::IfAbsent)
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
    assert!(source.get_item("live").unwrap().is_some());
    assert!(store.objects.lock().unwrap().contains_key(&orphan_key));
    assert!(!store.objects.lock().unwrap().contains_key(&wider_key));

    sync_database(&store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    for id in ["initial", "published", "orphaned", "live"] {
        assert!(peer.get_item(id).unwrap().is_some(), "missing item {id}");
    }

    drop(source);
    drop(peer);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
}

/// An orphan may only be adopted when it encodes the rows that are still
/// pending.
///
/// The adoption commit deletes every outbox row up to the segment's last
/// sequence, so adopting an orphan that encodes *other* items silently drops
/// the local changes instead of publishing them. That is exactly the shape a
/// database restored from an older backup produces: the outbox sequence is an
/// AUTOINCREMENT counter, so the restored database fills the same sequence
/// range with unrelated rows.
#[test]
fn orphan_that_encodes_other_items_is_not_adopted() {
    let store = MemoryStore::default();
    let source_paths = temp_paths("orphan-restored-source");
    let peer_paths = temp_paths("orphan-restored-peer");
    let source = Database::open(&source_paths.database).unwrap();
    let peer = Database::open(&peer_paths.database).unwrap();

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

    // An interrupted publish leaves an orphan segment for the next outbox row.
    source
        .save_item(&text_item("orphaned", "orphaned"))
        .unwrap();
    let device_id = source.get_sync_device_id().unwrap();
    let state = source
        .get_or_create_sync_remote_state(REMOTE_SCOPE)
        .unwrap();
    let expected_first = state.published_sequence + 1;
    let batch = source
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 16)
        .unwrap()
        .unwrap();
    let orphan_segment = Segment {
        device_id: device_id.clone(),
        epoch: state.epoch.clone(),
        first_sequence: batch.first_sequence,
        last_sequence: batch.last_sequence,
        mutations: batch.mutations,
    };
    let orphan_encoded = encode_segment(&orphan_segment, None).unwrap();
    let orphan_key = segment_object_key(
        &device_id,
        &state.epoch,
        orphan_segment.first_sequence,
        orphan_segment.last_sequence,
        &orphan_encoded.sha256,
    )
    .unwrap();
    store
        .put(&orphan_key, orphan_encoded.bytes, PutCondition::IfAbsent)
        .unwrap();

    // The database is rolled back: the pending row now points at a different
    // item, while the orphan still encodes the old one.
    source
        .save_item(&text_item("restored", "restored"))
        .unwrap();
    source
        .with_connection(|connection| {
            let sequence = i64::try_from(expected_first).unwrap();
            connection.execute(
                "UPDATE sync_outbox SET item_id = 'restored' WHERE sequence = ?1",
                [sequence],
            )?;
            connection.execute("DELETE FROM sync_outbox WHERE sequence > ?1", [sequence])?;
            Ok(())
        })
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

    // The stale orphan is dropped and the live outbox is published instead.
    assert!(
        !store.objects.lock().unwrap().contains_key(&orphan_key),
        "an orphan that encodes other items must not be adopted"
    );
    assert_eq!(source.count_sync_outbox().unwrap(), 0);

    sync_database(&store, &peer, &peer_paths, REMOTE_SCOPE, None, options()).unwrap();
    assert!(
        peer.get_item("restored").unwrap().is_some(),
        "the pending change must reach the peer"
    );
    assert!(
        peer.get_item("orphaned").unwrap().is_none(),
        "the stale orphan content must not be published"
    );

    drop(source);
    drop(peer);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(peer_paths.project).unwrap();
}
