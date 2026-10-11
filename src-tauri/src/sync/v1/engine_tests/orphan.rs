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
