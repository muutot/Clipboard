//! Orphaned upload adoption and leftover cleanup scenarios.

use super::*;

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
