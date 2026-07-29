use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};

use crate::storage::{Database, OcrRepository, SearchOperation, SearchRepository, StorageError};

use super::{
    ClipboardRepository, KindDeleteResult, KindDeleteScope, KindStorageStats,
    TextItemUpdate,
};

fn text_item(id: &str, content_hash: &str, created_at_ms: i64) -> ClipboardItem {
    ClipboardItem {
        id: id.to_owned(),
        kind: ClipboardKind::Text,
        title: format!("record-{id}"),
        text_content: Some(format!("content-{id}")),
        resource_path: None,
        preview_path: None,
        content_hash: content_hash.to_owned(),
        source_app: Some("test-suite".to_owned()),
        size_bytes: 12,
        created_at_ms,
        last_used_at_ms: None,
        is_favorite: false,
        icon_path: None,
        metadata_json: None,
    }
}

#[test]
fn saves_and_lists_items_by_recency() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("older", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("newer", "hash-2", 200))
        .unwrap();

    let items = database.list_recent(20, 0).unwrap();

    assert_eq!(database.item_count().unwrap(), 2);
    assert_eq!(items[0].id, "newer");
    assert_eq!(items[1].id, "older");
}

#[test]
fn batch_lookup_preserves_requested_relevance_order() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("first", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("second", "hash-2", 200))
        .unwrap();

    let items = database
        .get_items_by_ids(&[
            "second".to_owned(),
            "missing".to_owned(),
            "first".to_owned(),
        ])
        .unwrap();

    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );
}

#[test]
fn batch_lookup_reads_more_than_one_query_chunk() {
    let database = Database::open_in_memory().unwrap();
    let ids = (0..=500)
        .map(|index| format!("item-{index:03}"))
        .collect::<Vec<_>>();
    for (index, id) in ids.iter().enumerate() {
        database
            .save_item(&text_item(id, &format!("hash-{index}"), index as i64))
            .unwrap();
    }

    let requested_ids = ids.into_iter().rev().collect::<Vec<_>>();
    let items = database.get_items_by_ids(&requested_ids).unwrap();

    assert_eq!(items.len(), 501);
    assert_eq!(
        items.into_iter().map(|item| item.id).collect::<Vec<_>>(),
        requested_ids
    );
}

#[test]
fn lists_distinct_source_applications_for_filter_configuration() {
    let database = Database::open_in_memory().unwrap();
    let mut chatgpt = text_item("chatgpt", "hash-1", 100);
    chatgpt.source_app = Some("ChatGPT".to_owned());
    let mut browser = text_item("browser", "hash-2", 200);
    browser.source_app = Some("Browser".to_owned());
    let mut duplicate = text_item("duplicate", "hash-3", 300);
    duplicate.source_app = Some("chatgpt".to_owned());
    database.save_item(&chatgpt).unwrap();
    database.save_item(&browser).unwrap();
    database.save_item(&duplicate).unwrap();

    assert_eq!(
        database.list_source_applications().unwrap(),
        vec!["Browser", "ChatGPT"]
    );
}

#[test]
fn repeated_content_reuses_the_existing_record() {
    let database = Database::open_in_memory().unwrap();
    let mut first = text_item("original", "same-hash", 100);
    first.is_favorite = true;
    database.save_item(&first).unwrap();

    let repeated = text_item("replacement", "same-hash", 500);
    let stored_id = database.save_item(&repeated).unwrap();
    let stored = database.get_item(&stored_id).unwrap().unwrap();

    assert_eq!(stored_id, "original");
    assert_eq!(stored.created_at_ms, 500);
    assert!(stored.is_favorite);
    assert_eq!(database.item_count().unwrap(), 1);
}

#[test]
fn storage_references_include_every_path_from_multi_file_records() {
    let database = Database::open_in_memory().unwrap();
    let item = ClipboardItem {
        id: "files".to_owned(),
        kind: ClipboardKind::File,
        title: "first.txt".to_owned(),
        text_content: Some(
            serde_json::to_string(&["C:\\managed\\first.txt", "C:\\managed\\second.txt"])
                .unwrap(),
        ),
        resource_path: Some("C:\\managed\\first.txt".to_owned()),
        preview_path: None,
        content_hash: "files-hash".to_owned(),
        source_app: Some("Explorer".to_owned()),
        size_bytes: 20,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: false,
        icon_path: None,
        metadata_json: None,
    };
    database.save_item(&item).unwrap();

    let references = database.list_storage_file_references().unwrap();

    assert!(references
        .resource_paths
        .contains(&"C:\\managed\\first.txt".to_owned()));
    assert!(references
        .resource_paths
        .contains(&"C:\\managed\\second.txt".to_owned()));
}

#[test]
fn update_text_item_replaces_payload_and_preserves_record_metadata() {
    let database = Database::open_in_memory().unwrap();
    let mut original = text_item("editable", "old-hash", 100);
    original.last_used_at_ms = Some(150);
    original.is_favorite = true;
    original.icon_path = Some("icons/test.png".to_owned());
    original.metadata_json = Some(r#"{"custom":"value"}"#.to_owned());
    database.save_item(&original).unwrap();

    assert!(database
        .update_text_item(&TextItemUpdate {
            id: "editable",
            kind: ClipboardKind::Link,
            title: "updated title",
            text_content: "https://example.com",
            content_hash: "new-hash",
            size_bytes: 19,
            metadata_json: None,
        })
        .unwrap());

    let saved = database.get_item("editable").unwrap().unwrap();
    assert_eq!(saved.kind, ClipboardKind::Link);
    assert_eq!(saved.title, "updated title");
    assert_eq!(saved.text_content.as_deref(), Some("https://example.com"));
    assert_eq!(saved.content_hash, "new-hash");
    assert_eq!(saved.size_bytes, 19);
    assert_eq!(saved.source_app, original.source_app);
    assert_eq!(saved.created_at_ms, original.created_at_ms);
    assert_eq!(saved.last_used_at_ms, original.last_used_at_ms);
    assert_eq!(saved.is_favorite, original.is_favorite);
    assert_eq!(saved.icon_path, original.icon_path);
    assert_eq!(saved.metadata_json, original.metadata_json);
    assert_eq!(database.read_search_outbox(20).unwrap().len(), 2);
}

#[test]
fn update_text_item_can_replace_metadata_without_dropping_the_record() {
    let database = Database::open_in_memory().unwrap();
    let mut original = text_item("metadata-edit", "old-metadata-hash", 100);
    original.metadata_json = Some(r#"{"width":120,"custom":"value"}"#.to_owned());
    database.save_item(&original).unwrap();

    assert!(database
        .update_text_item(&TextItemUpdate {
            id: "metadata-edit",
            kind: ClipboardKind::Text,
            title: "Custom heading",
            text_content: "body text",
            content_hash: "new-metadata-hash",
            size_bytes: 9,
            metadata_json: Some(r#"{"width":120,"custom":"value","customTitle":true}"#),
        })
        .unwrap());

    let saved = database.get_item("metadata-edit").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(saved.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["width"], 120);
    assert_eq!(metadata["custom"], "value");
    assert_eq!(metadata["customTitle"], true);
}

#[test]
fn update_text_item_rejects_hash_collisions_without_partial_changes() {
    let database = Database::open_in_memory().unwrap();
    let original = text_item("original", "original-hash", 100);
    let existing = text_item("existing", "existing-hash", 200);
    database.save_item(&original).unwrap();
    database.save_item(&existing).unwrap();

    let result = database.update_text_item(&TextItemUpdate {
        id: "original",
        kind: ClipboardKind::Text,
        title: "colliding title",
        text_content: "colliding content",
        content_hash: "existing-hash",
        size_bytes: 17,
        metadata_json: None,
    });

    assert!(matches!(result, Err(StorageError::Sqlite(_))));
    assert_eq!(database.get_item("original").unwrap().unwrap(), original);
    assert_eq!(database.read_search_outbox(20).unwrap().len(), 2);
}

#[test]
fn favorite_must_be_removed_before_direct_deletion() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("item", "hash", 100)).unwrap();
    assert!(database.set_favorite("item", true).unwrap());

    assert!(matches!(
        database.delete_item("item"),
        Err(StorageError::FavoriteMustBeRemoved(id)) if id == "item"
    ));
    assert!(database.set_favorite("item", false).unwrap());
    assert!(database.delete_item("item").unwrap());
    assert_eq!(database.item_count().unwrap(), 0);
}

#[test]
fn batch_favorite_is_atomic_and_deduplicates_ids() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("first", "hash-first", 100))
        .unwrap();
    database
        .save_item(&text_item("second", "hash-second", 200))
        .unwrap();

    assert!(database
        .set_favorite_batch(
            &["first".to_owned(), "first".to_owned(), "second".to_owned()],
            true,
        )
        .unwrap());
    assert!(database.get_item("first").unwrap().unwrap().is_favorite);
    assert!(database.get_item("second").unwrap().unwrap().is_favorite);

    // A stale id must not leave a partially updated batch behind.
    assert!(!database
        .set_favorite_batch(&["first".to_owned(), "missing".to_owned()], false)
        .unwrap());
    assert!(database.get_item("first").unwrap().unwrap().is_favorite);
}

#[test]
fn batch_soft_delete_protects_favorites_without_partial_changes() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("regular", "hash-regular", 100))
        .unwrap();
    database
        .save_item(&text_item("favorite", "hash-favorite", 200))
        .unwrap();
    database.set_favorite("favorite", true).unwrap();

    let result = database.soft_delete_batch(&["regular".to_owned(), "favorite".to_owned()]);
    assert!(matches!(
        result,
        Err(StorageError::FavoriteMustBeRemoved(id)) if id == "favorite"
    ));
    assert!(database.get_item("regular").unwrap().is_some());
    assert!(database.get_item("favorite").unwrap().is_some());

    assert!(database
        .soft_delete_batch(&["regular".to_owned(), "regular".to_owned()])
        .unwrap());
    assert!(database.get_item("regular").unwrap().is_some());
    assert!(!database
        .list_recent(10, 0)
        .unwrap()
        .iter()
        .any(|item| item.id == "regular"));
}

#[test]
fn batch_lookup_excludes_soft_deleted_items() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("active", "hash-active", 100))
        .unwrap();
    database
        .save_item(&text_item("deleted", "hash-deleted", 200))
        .unwrap();
    database.soft_delete("deleted").unwrap();

    let ids = vec!["deleted".to_owned(), "active".to_owned()];
    let items = database.get_items_by_ids(&ids).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["active"]
    );
}

#[test]
fn favorite_survives_unfavorited_history_cleanup() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("favorite", "favorite-hash", 100))
        .unwrap();
    database
        .save_item(&text_item("regular", "regular-hash", 200))
        .unwrap();
    database.set_favorite("favorite", true).unwrap();

    database
        .with_connection(|connection| {
            connection.execute("DELETE FROM clipboard_items WHERE is_favorite = 0", [])?;
            Ok(())
        })
        .unwrap();

    let stored = database.get_item("favorite").unwrap().unwrap();
    assert!(stored.is_favorite);
    assert_eq!(stored.title, "record-favorite");
    assert!(database.get_item("regular").unwrap().is_none());
    assert_eq!(database.item_count().unwrap(), 1);

    database
        .with_connection(|connection| {
            let last_operation: String = connection.query_row(
                "SELECT operation FROM search_outbox ORDER BY sequence DESC LIMIT 1",
                [],
                |row| row.get(0),
            )?;

            assert_eq!(last_operation, "delete");
            Ok(())
        })
        .unwrap();
}

#[test]
fn kind_deletion_scope_controls_favorites_and_recycle_bin_records() {
    let database = Database::open_in_memory().unwrap();
    let mut active = text_item("active", "hash-active", 100);
    active.size_bytes = 10;
    let mut favorite = text_item("favorite", "hash-favorite", 200);
    favorite.size_bytes = 20;
    favorite.is_favorite = true;
    let mut recycled = text_item("recycled", "hash-recycled", 300);
    recycled.size_bytes = 30;
    let mut link = text_item("link", "hash-link", 400);
    link.kind = ClipboardKind::Link;
    link.size_bytes = 40;
    database.save_item(&active).unwrap();
    database.save_item(&favorite).unwrap();
    database.save_item(&recycled).unwrap();
    database.save_item(&link).unwrap();
    database.soft_delete("recycled").unwrap();

    assert_eq!(
        database
            .kind_storage_stats(
                ClipboardKind::Text,
                KindDeleteScope {
                    include_favorites: false,
                    include_deleted: false,
                },
            )
            .unwrap(),
        KindStorageStats {
            item_count: 1,
            size_bytes: 10,
        }
    );
    assert_eq!(
        database
            .kind_storage_stats(
                ClipboardKind::Text,
                KindDeleteScope {
                    include_favorites: true,
                    include_deleted: false,
                },
            )
            .unwrap(),
        KindStorageStats {
            item_count: 2,
            size_bytes: 30,
        }
    );
    assert_eq!(
        database
            .kind_storage_stats(
                ClipboardKind::Text,
                KindDeleteScope {
                    include_favorites: false,
                    include_deleted: true,
                },
            )
            .unwrap(),
        KindStorageStats {
            item_count: 2,
            size_bytes: 40,
        }
    );
    assert_eq!(
        database
            .kind_storage_stats(ClipboardKind::Text, KindDeleteScope::all())
            .unwrap(),
        KindStorageStats {
            item_count: 3,
            size_bytes: 60,
        }
    );

    let deleted = database
        .permanently_delete_by_kind(
            ClipboardKind::Text,
            KindDeleteScope {
                include_favorites: false,
                include_deleted: true,
            },
        )
        .unwrap();

    assert_eq!(
        deleted,
        KindDeleteResult {
            stats: KindStorageStats {
                item_count: 2,
                size_bytes: 40,
            },
            deleted_ids: vec!["active".to_owned(), "recycled".to_owned()],
        }
    );
    assert!(database.get_item("active").unwrap().is_none());
    assert!(database.get_item("recycled").unwrap().is_none());
    assert!(database.get_item("favorite").unwrap().is_some());
    assert!(database.get_item("link").unwrap().is_some());
}

#[test]
fn kind_deletion_cascades_ocr_queues_search_deletes_and_drops_references() {
    let database = Database::open_in_memory().unwrap();
    let image = ClipboardItem {
        id: "image".to_owned(),
        kind: ClipboardKind::Image,
        title: "captured image".to_owned(),
        text_content: None,
        resource_path: Some("C:\\managed\\image.png".to_owned()),
        preview_path: Some("C:\\managed\\preview.jpg".to_owned()),
        content_hash: "image-hash".to_owned(),
        source_app: Some("test-suite".to_owned()),
        size_bytes: 25,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: true,
        icon_path: None,
        metadata_json: None,
    };
    database.save_item(&image).unwrap();
    database
        .save_ocr_result(&OcrResult {
            item_id: image.id.clone(),
            status: OcrStatus::Completed,
            engine: "test".to_owned(),
            model_version: "1".to_owned(),
            language: Some("en".to_owned()),
            full_text: "recognized".to_owned(),
            blocks: Vec::new(),
            image_hash: image.content_hash.clone(),
            created_at_ms: 100,
            completed_at_ms: Some(200),
            error_message: None,
        })
        .unwrap();
    database
        .with_connection(|connection| {
            connection.execute("DELETE FROM search_outbox", [])?;
            Ok(())
        })
        .unwrap();

    let deleted = database
        .permanently_delete_by_kind(ClipboardKind::Image, KindDeleteScope::all())
        .unwrap();

    assert_eq!(
        deleted,
        KindDeleteResult {
            stats: KindStorageStats {
                item_count: 1,
                size_bytes: 25,
            },
            deleted_ids: vec!["image".to_owned()],
        }
    );
    assert!(database.get_item("image").unwrap().is_none());
    assert!(database.get_ocr_result("image").unwrap().is_none());

    let events = database.read_search_outbox(20).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].item_id, "image");
    assert_eq!(events[0].operation, SearchOperation::Delete);
    assert_eq!(
        database.list_storage_file_references().unwrap(),
        Default::default()
    );
}

// ── Task 4: Pagination edge cases ──

#[test]
fn pagination_empty_database_returns_empty() {
    let database = Database::open_in_memory().unwrap();
    let items = database.list_recent(100, 0).unwrap();
    assert!(items.is_empty());
}

#[test]
fn pagination_single_page_returns_all_items() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();
    database.save_item(&text_item("b", "hash-b", 200)).unwrap();
    database.save_item(&text_item("c", "hash-c", 300)).unwrap();

    let items = database.list_recent(50, 0).unwrap();
    assert_eq!(items.len(), 3);
}

#[test]
fn pagination_beyond_bounds_returns_empty() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();

    let items = database.list_recent(100, 1000).unwrap();
    assert!(items.is_empty());
}

#[test]
fn pagination_returns_partial_page_at_end() {
    let database = Database::open_in_memory().unwrap();
    for i in 0..5 {
        database
            .save_item(&text_item(
                &format!("item-{i}"),
                &format!("hash-{i}"),
                i * 100,
            ))
            .unwrap();
    }

    let items = database.list_recent(3, 3).unwrap();
    assert_eq!(items.len(), 2);
}

#[test]
fn pagination_limit_is_respected() {
    let database = Database::open_in_memory().unwrap();
    for i in 0..600 {
        database
            .save_item(&text_item(
                &format!("item-{i}"),
                &format!("hash-{i}"),
                i as i64 * 100,
            ))
            .unwrap();
    }

    let items = database.list_recent(100, 0).unwrap();
    assert_eq!(items.len(), 100);
}

// ── Task 4: Item count with soft-deleted items ──

#[test]
fn item_count_excludes_soft_deleted_items() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("active", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("deleted", "hash-2", 200))
        .unwrap();

    assert_eq!(database.item_count().unwrap(), 2);

    database.soft_delete("deleted").unwrap();
    assert_eq!(database.item_count().unwrap(), 1);
    assert!(database.get_item("active").unwrap().is_some());
}

#[test]
fn item_count_includes_restored_items() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("restored", "hash-1", 100))
        .unwrap();

    database.soft_delete("restored").unwrap();
    assert_eq!(database.item_count().unwrap(), 0);

    database.restore_deleted("restored").unwrap();
    assert_eq!(database.item_count().unwrap(), 1);
}

#[test]
fn deleted_records_are_listed_in_recency_order() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("older", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("newer", "hash-2", 200))
        .unwrap();
    database.soft_delete("older").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    database.soft_delete("newer").unwrap();

    let deleted = database.list_deleted(20, 0).unwrap();
    assert_eq!(
        deleted
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["newer", "older"]
    );
    assert!(database.list_recent(20, 0).unwrap().is_empty());
}

#[test]
fn batch_restore_and_permanent_delete_are_atomic() {
    let database = Database::open_in_memory().unwrap();
    for id in ["one", "two", "three"] {
        database
            .save_item(&text_item(id, &format!("hash-{id}"), 100))
            .unwrap();
        database.soft_delete(id).unwrap();
    }

    assert!(!database
        .restore_deleted_batch(&["one".to_owned(), "missing".to_owned()])
        .unwrap());
    assert_eq!(database.list_deleted(20, 0).unwrap().len(), 3);

    assert!(database
        .restore_deleted_batch(&["one".to_owned(), "two".to_owned()])
        .unwrap());
    assert_eq!(database.list_deleted(20, 0).unwrap().len(), 1);

    assert!(!database
        .permanently_delete_batch(&["three".to_owned(), "missing".to_owned()])
        .unwrap());
    assert!(database.get_item("three").unwrap().is_some());
    assert!(database.permanently_delete("three").unwrap());
    assert!(database.get_item("three").unwrap().is_none());
}

// ── Task 4: Concurrent read/write with multiple connections ──

#[test]
fn concurrent_read_does_not_block_writes() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();

    let read_result = database.get_item("a");
    database.save_item(&text_item("b", "hash-b", 200)).unwrap();

    assert!(read_result.is_ok());
    assert_eq!(database.item_count().unwrap(), 2);
}
