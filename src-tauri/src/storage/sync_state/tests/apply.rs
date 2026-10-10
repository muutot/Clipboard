//! Remote snapshot/checkpoint application and rollback scenarios.

use super::*;

#[test]
fn remote_snapshot_apply_is_atomic_idempotent_and_does_not_echo() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let writer = RecordVersion {
        modified_at_ms: 500,
        writer_device_id: REMOTE_DEVICE.to_string(),
    };
    let mut remote = replicated("remote", "hash-remote", "remote", writer);
    remote.item.metadata_json = Some(r#"{"tags":["shared","work"]}"#.to_string());
    let mutations = MutationBatch {
        upserts: vec![remote],
        tombstones: Vec::new(),
    };

    assert_eq!(
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
            .unwrap(),
        1
    );
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(
        database.get_item("remote").unwrap().unwrap().title,
        "remote"
    );
    let tags = database.list_all_tags().unwrap();
    assert_eq!(
        tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>(),
        vec!["shared", "work"]
    );
    assert_eq!(
        database
            .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
            .unwrap()
            .unwrap(),
        cursor(0, None)
    );
    assert_eq!(
        database
            .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"c".repeat(64), &mutations)
            .unwrap(),
        0
    );
}

#[test]
fn record_writer_breaks_equal_timestamp_ties_deterministically() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let lower_writer = "00000000-0000-4000-8000-000000000001";
    let higher_writer = "ffffffff-ffff-4fff-bfff-ffffffffffff";
    let initial = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie",
            "lower",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: lower_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"d".repeat(64), &initial)
        .unwrap();

    let winner = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie-winner",
            "higher",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: higher_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-1")), &winner)
        .unwrap();
    assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");

    let loser = MutationBatch {
        upserts: vec![replicated(
            "tie",
            "hash-tie-loser",
            "lower-again",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: lower_writer.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    assert_eq!(
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-2")), &loser,)
            .unwrap(),
        0
    );
    assert_eq!(database.get_item("tie").unwrap().unwrap().title, "higher");
}

#[test]
fn checkpoint_apply_updates_mutations_cursors_and_generation_atomically() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let second_device = "33333333-3333-4333-8333-333333333333";
    let second_epoch = "44444444-4444-4444-8444-444444444444";
    let first_key =
        crate::sync::v1::segment_object_key(REMOTE_DEVICE, REMOTE_EPOCH, 1, 2, &"a".repeat(64))
            .unwrap();
    let cursors = vec![
        cursor(2, Some(&first_key)),
        DeviceCursor {
            device_id: second_device.to_string(),
            epoch: second_epoch.to_string(),
            sequence: 0,
            last_segment_key: None,
        },
    ];
    let mutations = MutationBatch {
        upserts: vec![replicated(
            "checkpoint-item",
            "hash-checkpoint",
            "checkpoint",
            RecordVersion {
                modified_at_ms: 900,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };

    assert_eq!(
        database
            .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
            .unwrap(),
        1
    );
    assert!(database.get_item("checkpoint-item").unwrap().is_some());
    assert_eq!(database.list_sync_cursors(REMOTE_SCOPE).unwrap(), cursors);
    assert_eq!(
        database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
        cursors
    );
    assert_eq!(
        database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
        Some((1, "b".repeat(64)))
    );
    assert_eq!(database.count_sync_outbox().unwrap(), 0);
    assert_eq!(
        database
            .apply_sync_checkpoint(REMOTE_SCOPE, 1, &"b".repeat(64), &cursors, &mutations)
            .unwrap(),
        0
    );
}

#[test]
fn checkpoint_apply_does_not_regress_cursors_ahead_of_the_frozen_vector() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mutations = MutationBatch {
        upserts: vec![replicated(
            "checkpoint-item",
            "hash-checkpoint",
            "checkpoint",
            RecordVersion {
                modified_at_ms: 900,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    let first_key =
        crate::sync::v1::segment_object_key(REMOTE_DEVICE, REMOTE_EPOCH, 1, 2, &"a".repeat(64))
            .unwrap();

    // Generation 1 freezes the device at sequence 2.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            1,
            &"b".repeat(64),
            &[cursor(2, Some(&first_key))],
            &mutations,
        )
        .unwrap();
    // Generation 2 publishes a newer vector (sequence 5) that the pull
    // path applies.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"e".repeat(64),
            &[cursor(5, None)],
            &mutations,
        )
        .unwrap();
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![cursor(5, None)]
    );

    // Re-applying the generation-2 checkpoint must not reset the cursor
    // back to its stale in-vector value: the segments between 2 and 5
    // were already applied, so a regression would force a full re-pull.
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"e".repeat(64),
            &[cursor(2, Some(&first_key))],
            &mutations,
        )
        .unwrap();
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![cursor(5, None)]
    );
}

#[test]
fn invalid_checkpoint_rolls_back_rows_cursors_and_generation() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let existing = cursor(1, None);
    let initial = MutationBatch {
        upserts: vec![replicated(
            "existing-checkpoint",
            "hash-existing-checkpoint",
            "existing",
            RecordVersion {
                modified_at_ms: 500,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            1,
            &"c".repeat(64),
            std::slice::from_ref(&existing),
            &initial,
        )
        .unwrap();
    let invalid = MutationBatch {
        upserts: vec![ReplicatedItem {
            item: item("broken-checkpoint", "hash-broken-checkpoint", "broken").into(),
            version: RecordVersion {
                modified_at_ms: 600,
                writer_device_id: "not-a-uuid".to_string(),
            },
        }],
        tombstones: Vec::new(),
    };

    assert!(database
        .apply_sync_checkpoint(
            REMOTE_SCOPE,
            2,
            &"d".repeat(64),
            &[DeviceCursor {
                device_id: "33333333-3333-4333-8333-333333333333".to_string(),
                epoch: "44444444-4444-4444-8444-444444444444".to_string(),
                sequence: 0,
                last_segment_key: None,
            }],
            &invalid,
        )
        .is_err());
    assert!(database.get_item("broken-checkpoint").unwrap().is_none());
    assert_eq!(
        database.list_sync_cursors(REMOTE_SCOPE).unwrap(),
        vec![existing.clone()]
    );
    assert_eq!(
        database.get_sync_checkpoint_cursors(REMOTE_SCOPE).unwrap(),
        vec![existing]
    );
    assert_eq!(
        database.get_sync_checkpoint_state(REMOTE_SCOPE).unwrap(),
        Some((1, "c".repeat(64)))
    );
}

#[test]
fn tombstone_blocks_an_older_later_segment_from_resurrecting_a_row() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let initial = MutationBatch {
        upserts: vec![replicated(
            "victim",
            "hash-victim",
            "live",
            RecordVersion {
                modified_at_ms: 100,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"e".repeat(64), &initial)
        .unwrap();
    let deletion = MutationBatch {
        upserts: Vec::new(),
        tombstones: vec![Tombstone {
            item_id: "victim".to_string(),
            kind: ClipboardKind::Text.into(),
            content_hash: "hash-victim".to_string(),
            deleted_at_ms: 200,
            version: RecordVersion {
                modified_at_ms: 200,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        }],
    };
    database
        .apply_sync_segment(REMOTE_SCOPE, &cursor(1, Some("segment-delete")), &deletion)
        .unwrap();
    let stale = MutationBatch {
        upserts: vec![replicated(
            "victim",
            "hash-victim",
            "stale",
            RecordVersion {
                modified_at_ms: 150,
                writer_device_id: REMOTE_DEVICE.to_string(),
            },
        )],
        tombstones: Vec::new(),
    };
    assert_eq!(
        database
            .apply_sync_segment(REMOTE_SCOPE, &cursor(2, Some("segment-stale")), &stale,)
            .unwrap(),
        0
    );
    assert!(database
        .export_sync_snapshot()
        .unwrap()
        .mutations
        .upserts
        .is_empty());
    assert_eq!(
        database
            .export_sync_snapshot()
            .unwrap()
            .mutations
            .tombstones
            .len(),
        1
    );
}

#[test]
fn failed_remote_batch_rolls_back_rows_suppression_and_cursor() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    let mut image = item("local-image", "same-image-hash", "image");
    image.kind = ClipboardKind::Image;
    database.save_item(&image).unwrap();
    database.acknowledge_sync_outbox(1).unwrap();

    let first = replicated(
        "would-be-inserted",
        "unique-text-hash",
        "first",
        RecordVersion {
            modified_at_ms: 300,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    let mut collision = replicated(
        "different-image-id",
        "same-image-hash",
        "collision",
        RecordVersion {
            modified_at_ms: 300,
            writer_device_id: REMOTE_DEVICE.to_string(),
        },
    );
    collision.item.kind = ClipboardKind::Image.into();
    let failed = MutationBatch {
        upserts: vec![first, collision],
        tombstones: Vec::new(),
    };
    assert!(database
        .apply_sync_snapshot(REMOTE_SCOPE, &cursor(0, None), &"f".repeat(64), &failed)
        .is_err());
    assert!(database.get_item("would-be-inserted").unwrap().is_none());
    assert!(database
        .get_sync_cursor(REMOTE_SCOPE, REMOTE_DEVICE)
        .unwrap()
        .is_none());

    database
        .save_item(&item("after-failure", "hash-after", "after"))
        .unwrap();
    assert_eq!(database.count_sync_outbox().unwrap(), 1);
}
