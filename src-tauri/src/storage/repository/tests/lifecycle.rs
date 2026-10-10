//! Text edits, favorites, soft delete and kind deletion scenarios.

use super::*;

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
fn update_text_item_clears_stale_rich_content() {
    let database = Database::open_in_memory().unwrap();
    let mut original = text_item("rich-edit", "old-rich-hash", 100);
    original.html_content = Some("<p>old html</p>".to_owned());
    original.rtf_content = Some("{\\rtf old}".to_owned());
    database.save_item(&original).unwrap();

    assert!(database
        .update_text_item(&TextItemUpdate {
            id: "rich-edit",
            kind: ClipboardKind::Text,
            title: "edited",
            text_content: "edited body",
            content_hash: "new-rich-hash",
            size_bytes: 11,
            metadata_json: None,
        })
        .unwrap());

    let saved = database.get_item("rich-edit").unwrap().unwrap();
    assert_eq!(saved.text_content.as_deref(), Some("edited body"));
    assert_eq!(saved.html_content, None);
    assert_eq!(saved.rtf_content, None);
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
    assert_eq!(
        database.get_item("original").unwrap().unwrap(),
        ClipboardItem {
            last_used_at_ms: Some(original.created_at_ms),
            ..original
        }
    );
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
fn favorite_ignores_soft_deleted_rows() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("active", "hash-active", 100))
        .unwrap();
    database
        .save_item(&text_item("deleted", "hash-deleted", 200))
        .unwrap();
    assert!(database.soft_delete("deleted").unwrap());

    // A deleted row must not become favorited: favorites are exempt from
    // recycle-bin expiry, so this would pin invisible data forever.
    assert!(!database.set_favorite("deleted", true).unwrap());
    assert!(!database
        .set_favorite_batch(&["active".to_owned(), "deleted".to_owned()], true)
        .unwrap());
    assert!(!database.get_item("deleted").unwrap().unwrap().is_favorite);
    assert!(!database.get_item("active").unwrap().unwrap().is_favorite);
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
        .list_recent(10, 0, &HistoryFilter::default())
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
        html_content: None,
        rtf_content: None,
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
