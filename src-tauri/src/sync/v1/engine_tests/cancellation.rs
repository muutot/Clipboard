//! Cancellation rollback and retryability scenarios.

use super::*;

#[test]
fn cancelled_publication_keeps_heads_and_outbox_retryable() {
    use clipboard_sync::cancellation::CancellationToken;
    let store = MemoryStore::default();
    let paths = temp_paths("cancel-publication");
    let db = Database::open(&paths.database).unwrap();
    db.save_item(&text_item("initial", "initial")).unwrap();
    let token = CancellationToken::default();
    *store.cancel_after_key_fragment.lock().unwrap() = Some(("/snapshots/".into(), token.clone()));
    let result = engine::sync_database_cancellable(
        &store,
        &db,
        &engine_paths(&paths),
        REMOTE_SCOPE,
        None,
        options(),
        &token,
    );
    assert_eq!(result.unwrap_err(), "sync cancelled");
    assert!(
        !db.get_or_create_sync_remote_state(REMOTE_SCOPE)
            .unwrap()
            .initialized
    );
    assert!(store
        .objects
        .lock()
        .unwrap()
        .keys()
        .all(|key| !key.starts_with(HEADS_PREFIX)));
    *store.cancel_after_key_fragment.lock().unwrap() = None;
    sync_database(&store, &db, &paths, REMOTE_SCOPE, None, options()).unwrap();
    let id = db.get_sync_device_id().unwrap();
    let head_key = head_object_key(&id).unwrap();
    let previous = store
        .objects
        .lock()
        .unwrap()
        .get(&head_key)
        .unwrap()
        .clone();
    db.save_item(&text_item("next", "next")).unwrap();
    let token = CancellationToken::default();
    *store.cancel_after_key_fragment.lock().unwrap() = Some(("/segments/".into(), token.clone()));
    assert_eq!(
        engine::sync_database_cancellable(
            &store,
            &db,
            &engine_paths(&paths),
            REMOTE_SCOPE,
            None,
            options(),
            &token
        )
        .unwrap_err(),
        "sync cancelled"
    );
    assert_eq!(
        store.objects.lock().unwrap().get(&head_key).unwrap(),
        &previous
    );
    assert!(db
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .is_some());
    *store.cancel_after_key_fragment.lock().unwrap() = None;
    sync_database(&store, &db, &paths, REMOTE_SCOPE, None, options()).unwrap();
    assert!(db
        .get_sync_outbox_batch_for_scope(REMOTE_SCOPE, 100)
        .unwrap()
        .is_none());
    drop(db);
    fs::remove_dir_all(paths.project).unwrap();
}

#[test]
fn cancellation_between_snapshot_batches_rolls_back_rows_and_cursor() {
    for checkpoint in [false, true] {
        use clipboard_sync::cancellation::{checked_batches, CancellationToken};
        let db = Database::open_in_memory().unwrap();
        db.initialize_sync().unwrap();
        let token = CancellationToken::default();
        let cursor = DeviceCursor {
            device_id: uuid::Uuid::new_v4().to_string(),
            epoch: uuid::Uuid::new_v4().to_string(),
            sequence: 2,
            last_segment_key: None,
        };
        let mut calls = 0;
        let result = token.run(|| {
            let mut batches = checked_batches(std::iter::from_fn(|| {
                calls += 1;
                if calls == 2 {
                    token.cancel();
                    return None;
                }
                Some(Ok((
                    MutationBatch {
                        upserts: vec![replicated_text("first", "first", 1, &cursor.device_id)],
                        tombstones: vec![],
                    },
                    Default::default(),
                )))
            }));
            if checkpoint {
                clipboard_sync::v1::SyncRepository::apply_sync_checkpoint_batches(
                    &db,
                    REMOTE_SCOPE,
                    1,
                    &"a".repeat(64),
                    std::slice::from_ref(&cursor),
                    &mut batches,
                )
            } else {
                clipboard_sync::v1::SyncRepository::apply_sync_snapshot_batches(
                    &db,
                    REMOTE_SCOPE,
                    &cursor,
                    &"a".repeat(64),
                    &mut batches,
                )
            }
        });
        assert!(result.unwrap_err().contains("sync cancelled"));
        assert!(db.get_item("first").unwrap().is_none());
        assert!(db
            .get_sync_cursor(REMOTE_SCOPE, &cursor.device_id)
            .unwrap()
            .is_none());
        assert!(db
            .get_sync_checkpoint_state(REMOTE_SCOPE)
            .unwrap()
            .is_none());
    }
}
