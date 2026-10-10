//! Outbox coalescing, snapshot visiting and lazily folded packs.

use super::*;

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
