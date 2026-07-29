use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus, OcrTextBlock};
use crate::storage::ocr_repository::*;
use crate::storage::{ClipboardRepository, Database, SearchRepository, StorageError};

fn image_item(id: &str, image_hash: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_owned(),
        kind: ClipboardKind::Image,
        title: "screenshot.png".to_owned(),
        text_content: None,
        resource_path: Some("images/screenshot.png".to_owned()),
        preview_path: Some("previews/screenshot.webp".to_owned()),
        content_hash: image_hash.to_owned(),
        source_app: Some("test-suite".to_owned()),
        size_bytes: 1024,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: false,
        icon_path: None,
        metadata_json: None,
    }
}

fn completed_result(item_id: &str, image_hash: &str) -> OcrResult {
    OcrResult {
        item_id: item_id.to_owned(),
        status: OcrStatus::Completed,
        engine: "test-engine".to_owned(),
        model_version: "1".to_owned(),
        language: Some("zh-CN".to_owned()),
        full_text: "脸皮挺脏".to_owned(),
        blocks: vec![OcrTextBlock {
            text: "脸皮挺脏".to_owned(),
            confidence: 0.98,
            left: 10,
            top: 20,
            width: 100,
            height: 24,
        }],
        image_hash: image_hash.to_owned(),
        created_at_ms: 100,
        completed_at_ms: Some(200),
        error_message: None,
    }
}

#[test]
fn stores_blocks_and_reuses_completed_results_by_hash() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database
        .save_ocr_result(&completed_result("image", "image-hash"))
        .unwrap();

    let stored = database.get_ocr_result("image").unwrap().unwrap();
    let reused = database
        .find_completed_ocr_by_hash("image-hash")
        .unwrap()
        .unwrap();

    assert_eq!(stored.full_text, "脸皮挺脏");
    assert_eq!(stored.blocks.len(), 1);
    assert_eq!(reused.item_id, "image");
}

#[test]
fn ocr_updates_are_enqueued_for_search_indexing() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database
        .save_ocr_result(&completed_result("image", "image-hash"))
        .unwrap();

    database
        .with_connection(|connection| {
            let operations: i64 = connection.query_row(
                "SELECT COUNT(*)
                 FROM search_outbox
                 WHERE item_id = 'image' AND operation = 'upsert'",
                [],
                |row| row.get(0),
            )?;

            assert_eq!(operations, 2);
            Ok(())
        })
        .unwrap();
}

#[test]
fn claims_queued_images_in_creation_order() {
    let database = Database::open_in_memory().unwrap();
    let mut later = image_item("later", "later-hash");
    later.created_at_ms = 200;
    let mut earlier = image_item("earlier", "earlier-hash");
    earlier.created_at_ms = 100;
    database.save_item(&later).unwrap();
    database.save_item(&earlier).unwrap();
    assert!(database.enqueue_ocr("later").unwrap());
    assert!(database.enqueue_ocr("earlier").unwrap());

    let claimed = database.claim_next_ocr().unwrap().unwrap();

    assert_eq!(claimed.item_id, "earlier");
    assert_eq!(
        database.get_ocr_result("earlier").unwrap().unwrap().status,
        OcrStatus::Processing
    );
}

#[test]
fn requeues_jobs_interrupted_by_shutdown() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database.enqueue_ocr("image").unwrap();
    database.claim_next_ocr().unwrap().unwrap();

    assert_eq!(database.requeue_interrupted_ocr().unwrap(), 1);
    assert_eq!(
        database.get_ocr_result("image").unwrap().unwrap().status,
        OcrStatus::Pending
    );
}

#[test]
fn persists_failure_and_allows_a_manual_retry() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database.enqueue_ocr("image").unwrap();
    database.claim_next_ocr().unwrap().unwrap();

    assert!(database
        .mark_ocr_failed("image", "decoder unavailable")
        .unwrap());
    let failed = database.get_ocr_result("image").unwrap().unwrap();
    assert_eq!(failed.status, OcrStatus::Failed);
    assert_eq!(failed.error_message.as_deref(), Some("decoder unavailable"));
    assert_eq!(database.count_failed_ocr().unwrap(), 1);
    assert!(!database.mark_ocr_failed("image", "late failure").unwrap());

    assert!(database.retry_ocr("image").unwrap());
    let pending = database.get_ocr_result("image").unwrap().unwrap();
    assert_eq!(pending.status, OcrStatus::Pending);
    assert_eq!(pending.error_message, None);
    assert_eq!(database.count_failed_ocr().unwrap(), 0);
}

#[test]
fn regenerates_an_image_and_invalidates_same_hash_results() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("first", "shared-hash"))
        .unwrap();
    database
        .save_item(&image_item("second", "different-hash"))
        .unwrap();

    database
        .save_ocr_result(&completed_result("first", "shared-hash"))
        .unwrap();
    database
        .save_ocr_result(&completed_result("second", "shared-hash"))
        .unwrap();
    let existing_events = database.read_search_outbox(100).unwrap();
    database
        .acknowledge_search_outbox(existing_events.last().unwrap().sequence)
        .unwrap();

    assert!(database.regenerate_ocr("first").unwrap());

    let first = database.get_ocr_result("first").unwrap().unwrap();
    let second = database.get_ocr_result("second").unwrap().unwrap();
    assert_eq!(first.status, OcrStatus::Pending);
    assert_eq!(second.status, OcrStatus::Pending);
    assert!(first.full_text.is_empty());
    assert!(second.full_text.is_empty());
    assert!(first.created_at_ms <= second.created_at_ms);

    let regeneration_events = database.read_search_outbox(100).unwrap();
    assert!(regeneration_events
        .iter()
        .any(|event| event.item_id == "first"));
    assert!(regeneration_events
        .iter()
        .any(|event| event.item_id == "second"));
    assert!(!database
        .get_search_document("first")
        .unwrap()
        .unwrap()
        .content
        .contains("鑴哥毊鎸鸿剰"));
    assert!(!database
        .get_search_document("second")
        .unwrap()
        .unwrap()
        .content
        .contains("鑴哥毊鎸鸿剰"));
}

#[test]
fn regeneration_rejects_an_image_hash_that_is_processing() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database.enqueue_ocr("image").unwrap();
    database.claim_next_ocr().unwrap().unwrap();

    assert!(matches!(
        database.regenerate_ocr("image"),
        Err(StorageError::OcrRegenerationInProgress(item_id)) if item_id == "image"
    ));
}

#[test]
fn requeue_clears_stale_processing_metadata_and_is_reentrant() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("image", "image-hash"))
        .unwrap();
    database.enqueue_ocr("image").unwrap();
    database.claim_next_ocr().unwrap().unwrap();
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE ocr_results
                 SET error_message = 'interrupted', completed_at_ms = 123
                 WHERE item_id = 'image'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    assert_eq!(database.requeue_interrupted_ocr().unwrap(), 1);
    let pending = database.get_ocr_result("image").unwrap().unwrap();
    assert_eq!(pending.status, OcrStatus::Pending);
    assert_eq!(pending.error_message, None);
    assert_eq!(pending.completed_at_ms, None);

    assert_eq!(database.claim_next_ocr().unwrap().unwrap().item_id, "image");
    assert!(database.claim_next_ocr().unwrap().is_none());
}

#[test]
fn failed_transition_is_a_noop_for_pending_and_completed_tasks() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&image_item("pending", "pending-hash"))
        .unwrap();
    database.enqueue_ocr("pending").unwrap();
    assert!(!database
        .mark_ocr_failed("pending", "should not claim")
        .unwrap());
    assert_eq!(
        database.get_ocr_result("pending").unwrap().unwrap().status,
        OcrStatus::Pending
    );

    database
        .save_item(&image_item("completed", "completed-hash"))
        .unwrap();
    let completed = completed_result("completed", "completed-hash");
    database.save_ocr_result(&completed).unwrap();
    assert!(!database
        .mark_ocr_failed("completed", "should not overwrite")
        .unwrap());
    assert_eq!(
        database
            .get_ocr_result("completed")
            .unwrap()
            .unwrap()
            .status,
        OcrStatus::Completed
    );
}
