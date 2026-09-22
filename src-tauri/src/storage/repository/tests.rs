use crate::domain::{ClipboardItem, ClipboardKind, OcrResult, OcrStatus};

use crate::storage::{Database, OcrRepository, SearchOperation, SearchRepository, StorageError};

use super::{
    ClipboardRepository, HistoryFilter, KindDeleteResult, KindDeleteScope, KindStorageStats,
    TextItemUpdate,
};

fn text_item(id: &str, content_hash: &str, created_at_ms: i64) -> ClipboardItem {
    ClipboardItem {
        id: id.to_owned(),
        kind: ClipboardKind::Text,
        title: format!("record-{id}"),
        text_content: Some(format!("content-{id}")),
        html_content: None,
        rtf_content: None,
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
fn dedup_upsert_preserves_existing_tags() {
    let database = Database::open_in_memory().unwrap();
    let mut item = text_item("tagged", "hash-tagged", 100);
    item.metadata_json = Some(r#"{"customTitle":true}"#.to_owned());
    database.save_item(&item).unwrap();
    database.set_tags("tagged", &["work".to_owned()]).unwrap();

    // Re-save the same (kind, content_hash) with metadata that omits `tags`,
    // exactly like a re-captured image/file. The upsert must merge rather than
    // replace, or the tags silently disappear while `item_tags` keeps them.
    let mut recapture = item.clone();
    recapture.metadata_json = Some(r#"{"width":10}"#.to_owned());
    database.save_item(&recapture).unwrap();

    let stored = database.get_item("tagged").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata.get("tags"), Some(&serde_json::json!(["work"])));
    assert_eq!(metadata.get("width"), Some(&serde_json::json!(10)));
    assert_eq!(metadata.get("customTitle"), Some(&serde_json::json!(true)));
    assert_eq!(database.list_all_tags().unwrap()[0].name, "work");
}

#[test]
fn insert_populates_item_tags_from_metadata() {
    let database = Database::open_in_memory().unwrap();
    let mut item = text_item("duplicated", "hash-duplicated", 100);
    // A duplicate or import carries tags in `metadata_json` without going
    // through `set_tags`, so the derived `item_tags` index must be populated
    // on insert or tag counts and tag filtering miss the record.
    item.metadata_json = Some(r#"{"tags":["work","urgent"]}"#.to_owned());
    database.save_item(&item).unwrap();

    let tags = database.list_all_tags().unwrap();
    assert_eq!(tags.iter().find(|tag| tag.name == "work").unwrap().count, 1);
    assert_eq!(
        tags.iter().find(|tag| tag.name == "urgent").unwrap().count,
        1
    );

    let filter = HistoryFilter {
        tag: Some("work".to_owned()),
        ..HistoryFilter::default()
    };
    let recent = database.list_recent(20, 0, &filter).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, "duplicated");
}

#[test]
fn resource_reference_count_includes_multi_file_text_references() {
    let database = Database::open_in_memory().unwrap();

    // Row "single": a lone copy of /managed/b.bin as its resource.
    let mut single = text_item("single", "hash-single", 100);
    single.kind = ClipboardKind::File;
    single.text_content = None;
    single.resource_path = Some("/managed/b.bin".to_owned());
    database.save_item(&single).unwrap();

    // Row "group": a [a, b] group whose second file is the same /managed/b.bin,
    // referenced only through the ordered text_content list.
    let mut group = text_item("group", "hash-group", 200);
    group.kind = ClipboardKind::File;
    group.resource_path = Some("/managed/a.bin".to_owned());
    group.text_content = Some(serde_json::json!(["/managed/a.bin", "/managed/b.bin"]).to_string());
    database.save_item(&group).unwrap();

    // The shared path must be counted, or renaming the "single" record would
    // rename the file out from under the "group" record.
    assert_eq!(
        database
            .resource_reference_count("/managed/b.bin", "single")
            .unwrap(),
        1
    );
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

    let items = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();

    assert_eq!(database.item_count().unwrap(), 2);
    assert_eq!(items[0].id, "newer");
    assert_eq!(items[1].id, "older");
}

#[test]
fn set_last_used_records_usage_without_changing_capture_time() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("used", "hash-1", 100))
        .unwrap();
    assert_eq!(
        database.get_item("used").unwrap().unwrap().last_used_at_ms,
        None
    );

    // A re-copy freezes created_at_ms and records the reuse as usage.
    let recaptured = text_item("used", "hash-1", 500);
    database.save_item(&recaptured).unwrap();
    assert_eq!(
        database.get_item("used").unwrap().unwrap().created_at_ms,
        100
    );
    assert_eq!(
        database.get_item("used").unwrap().unwrap().last_used_at_ms,
        Some(500)
    );

    let updated = database.set_last_used("used").unwrap();
    assert!(updated);
    let loaded = database.get_item("used").unwrap().unwrap();
    assert!(loaded.last_used_at_ms.is_some());
    assert!(loaded.last_used_at_ms.unwrap() >= 500);
    // Capture time must remain untouched by the usage stamp.
    assert_eq!(loaded.created_at_ms, 100);

    // Unknown ids are a no-op.
    assert!(!database.set_last_used("missing").unwrap());
}

#[test]
fn list_recent_defaults_to_most_recently_used_with_capture_fallback() {
    let database = Database::open_in_memory().unwrap();
    // "old-used" captured long ago but used just now -> should still top the list.
    database
        .save_item(&text_item("old-used", "hash-1", 100))
        .unwrap();
    // "new-unused" just captured, never used -> falls back to created_at (top tier).
    database
        .save_item(&text_item("new-unused", "hash-2", 500))
        .unwrap();

    let before = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(before[0].id, "new-unused");
    assert_eq!(before[1].id, "old-used");

    database.set_last_used("old-used").unwrap();

    let after = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();
    // Using the older entry promotes it above the freshly captured (unused) one.
    assert_eq!(after[0].id, "old-used");
    assert_eq!(after[1].id, "new-unused");
}

#[test]
fn recapture_promotes_entry_with_older_last_used_to_top() {
    let database = Database::open_in_memory().unwrap();
    // A used entry: captured at 100, used at 150.
    let mut used = text_item("used", "hash-used", 100);
    used.last_used_at_ms = Some(150);
    database.save_item(&used).unwrap();
    // A newer unused entry captured at 200 sorts above (200 > 150).
    database
        .save_item(&text_item("newer", "hash-newer", 200))
        .unwrap();
    let before = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(before[0].id, "newer");
    assert_eq!(before[1].id, "used");

    // An external re-copy freezes created_at_ms and stamps last_used_at_ms.
    // The capture uses a fresh id, but dedup reuses the existing row id.
    let recapture = text_item("used-new-id", "hash-used", 300);
    assert_eq!(database.save_item(&recapture).unwrap(), "used");
    let stored = database.get_item("used").unwrap().unwrap();
    assert_eq!(stored.created_at_ms, 100);
    assert_eq!(stored.last_used_at_ms, Some(300));

    let after = database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(after[0].id, "used");
    assert_eq!(after[1].id, "newer");
}

#[test]
fn dedup_save_echoing_stored_timestamp_keeps_both_fields() {
    let database = Database::open_in_memory().unwrap();
    let item = text_item("echo", "hash-echo", 100);
    database.save_item(&item).unwrap();
    database.set_last_used("echo").unwrap();

    // Rename/rollback-style saves echo the stored capture time without a
    // usage stamp: the CASE must take the ELSE branch and leave both the
    // frozen capture time and the recorded usage untouched.
    database
        .save_item(&text_item("echo", "hash-echo", 100))
        .unwrap();
    let stored = database.get_item("echo").unwrap().unwrap();
    assert_eq!(stored.created_at_ms, 100);
    assert!(stored.last_used_at_ms.is_some());

    // Importing an older backup must not rewrite either field either.
    database
        .save_item(&text_item("echo", "hash-echo", 50))
        .unwrap();
    let stored = database.get_item("echo").unwrap().unwrap();
    assert_eq!(stored.created_at_ms, 100);
    assert!(stored.last_used_at_ms.is_some());
}

#[test]
fn re_copy_save_item_emits_no_outbox_traffic() {
    let database = Database::open_in_memory().unwrap();
    database.initialize_sync().unwrap();
    database
        .save_item(&text_item("recopied", "hash-recopied", 100))
        .unwrap();
    let sync_before = database.count_sync_outbox().unwrap();
    let search_before: i64 = database
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM search_outbox WHERE item_id = 'recopied'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert!(sync_before > 0);
    assert!(search_before > 0);

    // A pure re-copy freezes created_at_ms and stamps last_used_at_ms, and
    // neither outbox trigger watches the usage stamp, so the reuse must not
    // enqueue sync or search traffic. Real re-captures carry a fresh id but
    // an identical content-derived payload, so only the timestamps differ.
    let mut recapture = text_item("recopied-new-id", "hash-recopied", 200);
    recapture.title = "record-recopied".to_owned();
    recapture.text_content = Some("content-recopied".to_owned());
    database.save_item(&recapture).unwrap();

    assert_eq!(database.count_sync_outbox().unwrap(), sync_before);
    let search_after: i64 = database
        .with_connection(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM search_outbox WHERE item_id = 'recopied'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(search_after, search_before);
}

#[test]
fn list_recent_filters_by_tag_and_paginates_matching_records() {
    let database = Database::open_in_memory().unwrap();
    for (id, hash, ts) in [("a", "h-a", 100), ("b", "h-b", 200), ("c", "h-c", 300)] {
        database.save_item(&text_item(id, hash, ts)).unwrap();
    }
    database.set_tags("a", &["work".to_owned()]).unwrap();
    database.set_tags("c", &["work".to_owned()]).unwrap();
    // "b" deliberately keeps NULL metadata_json: tag filtering must skip it safely.

    let filter = HistoryFilter {
        tag: Some("work".to_owned()),
        ..Default::default()
    };
    let first = database.list_recent(1, 0, &filter).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, "c");
    let second = database.list_recent(1, 1, &filter).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].id, "a");
    assert!(database.list_recent(1, 2, &filter).unwrap().is_empty());

    let miss = HistoryFilter {
        tag: Some("nope".to_owned()),
        ..Default::default()
    };
    assert!(database.list_recent(20, 0, &miss).unwrap().is_empty());
}

#[test]
fn list_recent_applies_kind_favorite_source_and_date_filters() {
    let database = Database::open_in_memory().unwrap();
    let mut link = text_item("link-1", "hash-1", 100);
    link.kind = ClipboardKind::Link;
    database.save_item(&link).unwrap();
    let mut image = text_item("image-1", "hash-2", 200);
    image.kind = ClipboardKind::Image;
    image.source_app = Some("other-app".to_owned());
    database.save_item(&image).unwrap();
    let mut fav = text_item("fav-1", "hash-3", 300);
    fav.is_favorite = true;
    database.save_item(&fav).unwrap();

    let kind_filter = HistoryFilter {
        kind: Some(ClipboardKind::Image),
        ..Default::default()
    };
    let items = database.list_recent(20, 0, &kind_filter).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["image-1"]
    );

    let favorite_filter = HistoryFilter {
        favorite_only: true,
        ..Default::default()
    };
    let items = database.list_recent(20, 0, &favorite_filter).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["fav-1"]
    );

    let source_filter = HistoryFilter {
        source_app: Some("other-app".to_owned()),
        ..Default::default()
    };
    let items = database.list_recent(20, 0, &source_filter).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["image-1"]
    );

    let date_filter = HistoryFilter {
        date_from_ms: Some(150),
        date_to_ms: Some(250),
        ..Default::default()
    };
    let items = database.list_recent(20, 0, &date_filter).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["image-1"]
    );

    let combined = HistoryFilter {
        kind: Some(ClipboardKind::Text),
        date_from_ms: Some(150),
        ..Default::default()
    };
    let items = database.list_recent(20, 0, &combined).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["fav-1"]
    );
}

#[test]
fn set_tags_replaces_and_removes_tags_in_metadata_json() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();

    assert!(database
        .set_tags("item-1", &["work".to_owned(), " urgent ".to_owned()])
        .unwrap());
    let stored = database.get_item("item-1").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["work", "urgent"]));

    // Empty list removes the tags key entirely.
    assert!(database.set_tags("item-1", &[]).unwrap());
    let stored = database.get_item("item-1").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata_json.as_deref().unwrap()).unwrap();
    assert!(metadata.get("tags").is_none());

    // Missing record is a no-op returning false.
    assert!(!database.set_tags("missing", &["x".to_owned()]).unwrap());
}

#[test]
fn list_all_tags_returns_distinct_names_with_counts_and_colors() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("item-2", "hash-2", 200))
        .unwrap();
    database
        .set_tags("item-1", &["work".to_owned(), "urgent".to_owned()])
        .unwrap();
    database.set_tags("item-2", &["work".to_owned()]).unwrap();

    assert!(database.set_tag_color("work", "#ff0000").unwrap());

    let tags = database.list_all_tags().unwrap();
    let work = tags.iter().find(|tag| tag.name == "work").unwrap();
    assert_eq!(work.count, 2);
    assert_eq!(work.color, "#ff0000");
    let urgent = tags.iter().find(|tag| tag.name == "urgent").unwrap();
    assert_eq!(urgent.count, 1);
    assert_eq!(urgent.color, "");
    assert_eq!(tags.len(), 2);
}

#[test]
fn rename_tag_rewrites_items_and_migrates_color() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();
    database
        .save_item(&text_item("item-2", "hash-2", 200))
        .unwrap();
    database
        .set_tags("item-1", &["work".to_owned(), "urgent".to_owned()])
        .unwrap();
    database.set_tags("item-2", &["work".to_owned()]).unwrap();
    assert!(database.set_tag_color("work", "#00ff00").unwrap());

    let updated = database.rename_tag("work", "jobs").unwrap();
    assert_eq!(updated, 2);

    let item1 = database.get_item("item-1").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(item1.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["jobs", "urgent"]));
    let item2 = database.get_item("item-2").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(item2.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["jobs"]));

    let tags = database.list_all_tags().unwrap();
    let jobs = tags.iter().find(|tag| tag.name == "jobs").unwrap();
    assert_eq!(jobs.count, 2);
    assert_eq!(jobs.color, "#00ff00");
    assert!(tags.iter().all(|tag| tag.name != "work"));
}

#[test]
fn rename_tag_keeps_the_destination_registry_color() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();
    database
        .set_tags("item-1", &["work".to_owned(), "urgent".to_owned()])
        .unwrap();
    assert!(database.set_tag_color("work", "#0000ff").unwrap());
    assert!(database.set_tag_color("urgent", "#ff0000").unwrap());

    database.rename_tag("work", "urgent").unwrap();

    let tags = database.list_all_tags().unwrap();
    let urgent = tags.iter().find(|tag| tag.name == "urgent").unwrap();
    assert_eq!(urgent.color, "#ff0000");
    assert!(tags.iter().all(|tag| tag.name != "work"));
}

#[test]
fn delete_tag_removes_it_from_items_and_registry() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();
    database
        .set_tags("item-1", &["work".to_owned(), "urgent".to_owned()])
        .unwrap();
    assert!(database.set_tag_color("urgent", "#0000ff").unwrap());

    let removed = database.delete_tag("urgent").unwrap();
    assert_eq!(removed, 1);

    let item1 = database.get_item("item-1").unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(item1.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["work"]));

    let tags = database.list_all_tags().unwrap();
    assert!(tags.iter().all(|tag| tag.name != "urgent"));
}

#[test]
fn tag_color_validation_accepts_hex_and_rejects_invalid() {
    let database = Database::open_in_memory().unwrap();
    assert!(database.set_tag_color("work", "#a1b2c3").unwrap());
    assert!(database.set_tag_color("work", "").unwrap());
    assert!(!database.set_tag_color("work", "red").unwrap());
    assert!(!database.set_tag_color("", "#000000").unwrap());
}

#[test]
fn tags_are_folded_into_search_document_content() {
    use crate::storage::SearchRepository;
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("item-1", "hash-1", 100))
        .unwrap();
    database
        .set_tags("item-1", &["project-x".to_owned(), "urgent".to_owned()])
        .unwrap();

    let document = database.get_search_document("item-1").unwrap().unwrap();
    assert!(document.content.contains("project-x"));
    assert!(document.content.contains("urgent"));
    assert!(document.content.contains("content-item-1"));
}

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

// ── Task 4: Pagination edge cases ──

#[test]
fn pagination_empty_database_returns_empty() {
    let database = Database::open_in_memory().unwrap();
    let items = database
        .list_recent(100, 0, &HistoryFilter::default())
        .unwrap();
    assert!(items.is_empty());
}

#[test]
fn pagination_single_page_returns_all_items() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();
    database.save_item(&text_item("b", "hash-b", 200)).unwrap();
    database.save_item(&text_item("c", "hash-c", 300)).unwrap();

    let items = database
        .list_recent(50, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(items.len(), 3);
}

#[test]
fn pagination_beyond_bounds_returns_empty() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();

    let items = database
        .list_recent(100, 1000, &HistoryFilter::default())
        .unwrap();
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

    let items = database
        .list_recent(3, 3, &HistoryFilter::default())
        .unwrap();
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

    let items = database
        .list_recent(100, 0, &HistoryFilter::default())
        .unwrap();
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
fn capacity_counts_only_evictable_records() {
    let database = Database::open_in_memory().unwrap();
    let mut favorite = text_item("favorite", "hash-fav", 100);
    favorite.is_favorite = true;
    database.save_item(&favorite).unwrap();
    database
        .save_item(&text_item("plain", "hash-plain", 200))
        .unwrap();

    // `item_count` still reports every active row for stats surfaces…
    assert_eq!(database.item_count().unwrap(), 2);
    // …while capacity accounting sees only the row cleanup may delete.
    assert_eq!(database.evictable_item_count().unwrap(), 1);

    // An all-favorite library over the limit evicts nothing instead of
    // reporting a phantom excess on every cleanup tick.
    database
        .save_item(&text_item("plain-2", "hash-plain-2", 300))
        .unwrap();
    let mut all_favorite = text_item("favorite-2", "hash-fav-2", 400);
    all_favorite.is_favorite = true;
    database.save_item(&all_favorite).unwrap();
    database
        .set_favorite_batch(&["plain".to_owned(), "plain-2".to_owned()], true)
        .unwrap();
    assert_eq!(database.evictable_item_count().unwrap(), 0);
    assert_eq!(database.enforce_capacity_limit(1).unwrap(), 0);
    assert_eq!(database.item_count().unwrap(), 4);
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
    assert!(database
        .list_recent(20, 0, &HistoryFilter::default())
        .unwrap()
        .is_empty());
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

#[test]
fn expired_purge_covers_legacy_rows_without_a_delete_timestamp() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("legacy", "hash-legacy", 100))
        .unwrap();
    assert!(database.soft_delete("legacy").unwrap());
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE clipboard_items SET deleted_at_ms = NULL WHERE id = 'legacy'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    assert_eq!(database.permanently_delete_expired(30).unwrap(), 1);
    assert!(database.get_item("legacy").unwrap().is_none());
}

#[test]
fn expired_purge_keeps_favorited_recycle_bin_records() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("kept", "hash-kept", 100))
        .unwrap();
    assert!(database.soft_delete("kept").unwrap());
    // Favoriting inside the bin is refused and deleting a favorite is
    // rejected, so a favorited recycle-bin row can only be legacy
    // (grandfathered before the refuse rule). NULL the timestamp like a
    // legacy row so the record is actually expiry-eligible and the
    // favorite guard in the purge is exercised.
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE clipboard_items SET is_favorite = 1, deleted_at_ms = NULL WHERE id = 'kept'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    assert_eq!(database.permanently_delete_expired(30).unwrap(), 0);
    assert!(database.get_item("kept").unwrap().is_some());
}
