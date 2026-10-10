//! Rich content, batch lookup, source apps and resource references.

use super::*;

#[test]
fn html_content_round_trips_through_storage() {
    let database = Database::open_in_memory().unwrap();
    let mut item = text_item("rich", "hash-1", 100);
    item.html_content = Some("<b>bold</b>".to_owned());
    database.save_item(&item).unwrap();

    let loaded = database.get_item("rich").unwrap().unwrap();
    assert_eq!(loaded.html_content.as_deref(), Some("<b>bold</b>"));
    assert_eq!(loaded.text_content.as_deref(), Some("content-rich"));
}

#[test]
fn rtf_content_round_trips_through_storage() {
    let database = Database::open_in_memory().unwrap();
    let mut item = text_item("rich-rtf", "hash-rtf-1", 100);
    item.rtf_content = Some("{\\rtf1\\b bold}".to_owned());
    database.save_item(&item).unwrap();

    let loaded = database.get_item("rich-rtf").unwrap().unwrap();
    assert_eq!(loaded.rtf_content.as_deref(), Some("{\\rtf1\\b bold}"));
    assert_eq!(loaded.text_content.as_deref(), Some("content-rich-rtf"));
}

#[test]
fn rtf_content_survives_dedup_upsert_without_plain_text() {
    let database = Database::open_in_memory().unwrap();
    let mut first = text_item("rich-rtf", "hash-rtf-1", 100);
    first.rtf_content = Some("{\\rtf1\\b bold}".to_owned());
    database.save_item(&first).unwrap();

    // A later plain-text-only copy of the same content keeps the stored rtf.
    let second = text_item("rich-rtf", "hash-rtf-1", 200);
    database.save_item(&second).unwrap();

    let loaded = database.get_item("rich-rtf").unwrap().unwrap();
    assert_eq!(loaded.rtf_content.as_deref(), Some("{\\rtf1\\b bold}"));
}

#[test]
fn html_content_survives_dedup_upsert_without_plain_text() {
    let database = Database::open_in_memory().unwrap();
    let mut first = text_item("rich", "hash-1", 100);
    first.html_content = Some("<b>bold</b>".to_owned());
    database.save_item(&first).unwrap();

    // A later plain-text-only copy of the same content keeps the stored html.
    let second = text_item("rich", "hash-1", 200);
    database.save_item(&second).unwrap();

    let loaded = database.get_item("rich").unwrap().unwrap();
    assert_eq!(loaded.html_content.as_deref(), Some("<b>bold</b>"));
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
fn lists_source_applications_with_the_most_recent_icon() {
    let database = Database::open_in_memory().unwrap();

    // An older row carries the icon; a newer row for the same app has none.
    // The most recent row *with* an icon must still win.
    let mut older_icon = text_item("older", "hash-1", 100);
    older_icon.source_app = Some("ChatGPT".to_owned());
    older_icon.icon_path = Some("icons/chatgpt.png".to_owned());
    let mut newer_no_icon = text_item("newer", "hash-2", 200);
    newer_no_icon.source_app = Some("ChatGPT".to_owned());
    let mut browser = text_item("browser", "hash-3", 300);
    browser.source_app = Some("Browser".to_owned());

    database.save_item(&older_icon).unwrap();
    database.save_item(&newer_no_icon).unwrap();
    database.save_item(&browser).unwrap();

    let apps = database.list_source_applications_with_icons().unwrap();
    assert_eq!(
        apps,
        vec![
            ("Browser".to_owned(), None),
            ("ChatGPT".to_owned(), Some("icons/chatgpt.png".to_owned())),
        ]
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
    // The original capture time is frozen; the re-copy is recorded as usage.
    assert_eq!(stored.created_at_ms, 100);
    assert_eq!(stored.last_used_at_ms, Some(500));
    assert!(stored.is_favorite);
    assert_eq!(database.item_count().unwrap(), 1);
}

#[test]
fn re_copied_soft_deleted_content_resurrects_the_record() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("original", "same-hash", 100))
        .unwrap();

    assert!(database.soft_delete("original").unwrap());
    assert_eq!(database.item_count().unwrap(), 0);
    assert!(database
        .content_exists(ClipboardKind::Text, "same-hash")
        .unwrap());

    let repeated = text_item("replacement", "same-hash", 500);
    let stored_id = database.save_item(&repeated).unwrap();

    assert_eq!(stored_id, "original");
    assert_eq!(database.item_count().unwrap(), 1);
    let stored = database.get_item(&stored_id).unwrap().unwrap();
    // Resurrection keeps the original capture time and records the reuse.
    assert_eq!(stored.created_at_ms, 100);
    assert_eq!(stored.last_used_at_ms, Some(500));
    let listed = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "original");
}

#[test]
fn storage_references_include_every_path_from_multi_file_records() {
    let database = Database::open_in_memory().unwrap();
    let item = ClipboardItem {
        id: "files".to_owned(),
        kind: ClipboardKind::File,
        title: "first.txt".to_owned(),
        text_content: Some(
            serde_json::to_string(&["C:\\managed\\first.txt", "C:\\managed\\second.txt"]).unwrap(),
        ),
        html_content: None,
        rtf_content: None,
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
fn resource_reference_count_reports_shared_files_excluding_the_current_record() {
    let database = Database::open_in_memory().unwrap();
    let mut image = text_item("image-1", "hash-1", 100);
    image.kind = ClipboardKind::Image;
    image.resource_path = Some("C:\\managed\\shared.png".to_owned());
    image.preview_path = Some("C:\\managed\\shared.png".to_owned());
    database.save_item(&image).unwrap();

    // A second record sharing the same resource path (dedup / duplicate).
    let mut image2 = text_item("image-2", "hash-2", 200);
    image2.kind = ClipboardKind::Image;
    image2.resource_path = Some("C:\\managed\\shared.png".to_owned());
    database.save_item(&image2).unwrap();

    assert_eq!(
        database
            .resource_reference_count("C:\\managed\\shared.png", "image-1")
            .unwrap(),
        1
    );
    // Excluding the other owner also still finds this record referencing it.
    assert_eq!(
        database
            .resource_reference_count("C:\\managed\\shared.png", "image-2")
            .unwrap(),
        1
    );
    // A path no record references counts as zero.
    assert_eq!(
        database
            .resource_reference_count("C:\\managed\\missing.png", "image-1")
            .unwrap(),
        0
    );
}

#[test]
fn latest_file_record_referencing_storage_returns_most_recent_file_owner() {
    let database = Database::open_in_memory().unwrap();

    // Row "old": first copy of the managed file, its original later renamed.
    let mut old = text_item("file-old", "hash-old", 100);
    old.kind = ClipboardKind::File;
    old.text_content = None;
    old.resource_path = Some("C:\\managed\\a1b2c3.txt".to_owned());
    old.metadata_json = Some(
        serde_json::json!({
            "files": [{
                "name": "Draft.txt",
                "storagePath": "C:\\managed\\a1b2c3.txt",
                "originalPath": "C:\\originals\\Draft.txt"
            }]
        })
        .to_string(),
    );
    database.save_item(&old).unwrap();

    // Row "new": the same content re-copied after the rename, under a new id.
    let mut new = text_item("file-new", "hash-new", 200);
    new.kind = ClipboardKind::File;
    new.text_content = None;
    new.resource_path = Some("C:\\managed\\a1b2c3.txt".to_owned());
    new.metadata_json = Some(
        serde_json::json!({
            "files": [{
                "name": "Final.txt",
                "storagePath": "C:\\managed\\a1b2c3.txt",
                "originalPath": "C:\\originals\\Final.txt"
            }]
        })
        .to_string(),
    );
    database.save_item(&new).unwrap();

    // The "old" record must inherit the freshest owner's original path.
    let inherited = database
        .latest_file_record_referencing_storage("C:\\managed\\a1b2c3.txt", "file-old")
        .unwrap()
        .unwrap();
    assert_eq!(inherited.id, "file-new");
    let rediscovered = database
        .latest_file_record_referencing_storage("C:\\managed\\a1b2c3.txt", "file-new")
        .unwrap()
        .unwrap();
    assert_eq!(rediscovered.id, "file-old");

    // Unknown storage paths yield no record.
    assert!(database
        .latest_file_record_referencing_storage("C:\\managed\\missing.txt", "file-old")
        .unwrap()
        .is_none());
    // Non-file records must never satisfy the lookup, even when their metadata
    // references the same managed path.
    let storage = "C:\\managed\\text-only.txt";
    let mut text_row = text_item("text-row", "hash-text", 300);
    text_row.metadata_json = Some(
        serde_json::json!({
            "files": [{
                "storagePath": storage,
                "originalPath": "C:\\originals\\Text.txt"
            }]
        })
        .to_string(),
    );
    database.save_item(&text_row).unwrap();
    assert!(database
        .latest_file_record_referencing_storage(storage, "text-scan")
        .unwrap()
        .is_none());
}
