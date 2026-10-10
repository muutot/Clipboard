//! Insert/upsert/dedup, history listing and usage-cursor scenarios.

use super::*;

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
fn save_rolls_back_record_and_tags_on_derived_index_failure() {
    let database = Database::open_in_memory().unwrap();
    let existing = text_item("existing", "hash-existing", 100);
    database.save_item(&existing).unwrap();
    database.set_tags("existing", &["keep".into()]).unwrap();
    let before = database.get_item("existing").unwrap().unwrap();
    database
        .with_connection(|connection| {
            connection.execute_batch(
                "CREATE TRIGGER reject_test_tag BEFORE INSERT ON item_tags
             WHEN NEW.tag = 'reject'
             BEGIN SELECT RAISE(ABORT, 'injected tag failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();
    for mut row in [before.clone(), text_item("new", "hash-new", 200)] {
        row.title = "must roll back".into();
        row.metadata_json = Some(r#"{"tags":["partial","reject"]}"#.into());
        assert!(database.save_item(&row).is_err());
    }
    assert_eq!(database.get_item("existing").unwrap().unwrap(), before);
    assert!(database.get_item("new").unwrap().is_none());
    let tags = database.list_all_tags().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "keep");

    let mut rejected = text_item("rejected", "hash-rejected", 300);
    rejected.metadata_json = Some(r#"{"tags":["partial","reject"]}"#.into());
    let summary = database
        .save_items_transactional(&[
            ("rejected".into(), rejected),
            (
                "accepted".into(),
                text_item("accepted", "hash-accepted", 400),
            ),
        ])
        .unwrap();
    assert_eq!((summary.imported_count, summary.skipped_count), (1, 1));
    assert!(database.get_item("rejected").unwrap().is_none());
    assert!(database.get_item("accepted").unwrap().is_some());
    assert_eq!(database.list_all_tags().unwrap()[0].name, "keep");
    database
        .with_connection(|connection| {
            let leaked: i64 = connection.query_row(
                "SELECT COUNT(*) FROM item_tags WHERE tag = 'partial'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(leaked, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn bulk_import_does_not_overwrite_a_row_on_id_collision() {
    let database = Database::open_in_memory().unwrap();
    let mut victim = text_item("shared-id", "hash-victim", 100);
    victim.text_content = Some("original".to_owned());
    database.save_item(&victim).unwrap();

    // Regression: a bare `ON CONFLICT DO UPDATE` also matched the `id` primary
    // key, so an imported record that reused an existing id with a different
    // content hash rewrote and un-deleted the victim row. The bulk path only
    // guards `(kind, content_hash)`, so the id collision must surface as a skip.
    let mut forged = text_item("shared-id", "hash-forged", 500);
    forged.text_content = Some("forged".to_owned());
    let summary = database
        .save_items_transactional(&[("forged".to_owned(), forged)])
        .unwrap();

    assert_eq!(summary.imported_count, 0);
    assert_eq!(summary.skipped_count, 1);
    let stored = database.get_item("shared-id").unwrap().unwrap();
    assert_eq!(stored.content_hash, "hash-victim");
    assert_eq!(stored.text_content.as_deref(), Some("original"));
    assert!(!database
        .content_exists(ClipboardKind::Text, "hash-forged")
        .unwrap());
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
        Some(100)
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
fn list_recent_defaults_to_last_used_initialized_at_creation() {
    let database = Database::open_in_memory().unwrap();
    // "old-used" captured long ago but used just now -> should still top the list.
    database
        .save_item(&text_item("old-used", "hash-1", 100))
        .unwrap();
    // A fresh capture initializes its usage timestamp to creation time.
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
fn cursor_pagination_survives_out_of_band_reuse() {
    let database = Database::open_in_memory().unwrap();
    for index in 1..=60 {
        database
            .save_item(&text_item(
                &format!("i{index:02}"),
                &format!("hash-{index}"),
                index,
            ))
            .unwrap();
    }

    let filter = HistoryFilter::default();
    let page1 = database.list_recent(50, 0, &filter).unwrap();
    assert_eq!(page1.len(), 50);
    // Page 1 covers created 60..=11; the anchor is its last row (i11).
    let page1_ids: Vec<&str> = page1.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(page1_ids.last().copied(), Some("i11"));

    // Out-of-band reuse of a not-yet-served row between the two page
    // fetches: this is exactly the interleaving that makes OFFSET pagination
    // replay i11 and (with a removal) silently drop rows.
    database.set_last_used("i06").unwrap();

    let page2 = database
        .list_recent(
            50,
            0,
            &HistoryFilter {
                cursor: Some(cursor_of(page1.last().unwrap())),
                ..HistoryFilter::default()
            },
        )
        .unwrap();

    // The promoted i06 jumped above the cursor (the user already used it);
    // everything else below the anchor is served exactly once, in order.
    let expected: Vec<String> = [
        "i10", "i09", "i08", "i07", "i05", "i04", "i03", "i02", "i01",
    ]
    .iter()
    .map(|id| (*id).to_owned())
    .collect();
    let page2_ids: Vec<String> = page2.iter().map(|item| item.id.clone()).collect();
    assert_eq!(page2_ids, expected);
    assert!(page2_ids.iter().all(|id| !page1_ids.contains(&id.as_str())));
}

#[test]
fn cursor_pagination_breaks_usage_ties_by_id_not_creation() {
    let database = Database::open_in_memory().unwrap();
    // "a" and "b" tie on usage. ID alone breaks the tie, so "b" comes
    // first even though "a" has the newer creation time.
    let mut b = text_item("b", "hash-b", 90);
    b.last_used_at_ms = Some(100);
    database.save_item(&text_item("a", "hash-a", 100)).unwrap();
    database.save_item(&b).unwrap();

    let page1 = database
        .list_recent(1, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(page1[0].id, "b");

    let page2 = database
        .list_recent(
            10,
            0,
            &HistoryFilter {
                cursor: Some(cursor_of(&page1[0])),
                ..HistoryFilter::default()
            },
        )
        .unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].id, "a");
}

#[test]
fn list_recent_uses_stored_usage_even_when_it_predates_creation() {
    let database = Database::open_in_memory().unwrap();
    let mut older_usage = text_item("new-capture", "hash-new", 300);
    older_usage.last_used_at_ms = Some(50);
    database.save_item(&older_usage).unwrap();
    database
        .save_item(&text_item("old-capture", "hash-old", 100))
        .unwrap();

    let page = database
        .list_recent(1, 0, &HistoryFilter::default())
        .unwrap();
    assert_eq!(page[0].id, "old-capture");
    let next = database
        .list_recent(
            1,
            0,
            &HistoryFilter {
                cursor: Some(cursor_of(&page[0])),
                ..HistoryFilter::default()
            },
        )
        .unwrap();
    assert_eq!(next[0].id, "new-capture");
    assert_eq!(next[0].last_used_at_ms, Some(50));
}

#[test]
fn inserts_and_imports_initialize_missing_usage_and_preserve_explicit_usage() {
    let database = Database::open_in_memory().unwrap();
    database
        .save_item(&text_item("capture", "hash-capture", 100))
        .unwrap();
    let legacy = text_item("legacy-import", "hash-legacy", 200);
    let mut used = text_item("used-import", "hash-used", 300);
    used.last_used_at_ms = Some(50);
    database
        .save_items_transactional(&[("legacy".into(), legacy), ("used".into(), used)])
        .unwrap();

    for (id, expected) in [
        ("capture", 100),
        ("legacy-import", 200),
        ("used-import", 50),
    ] {
        assert_eq!(
            database.get_item(id).unwrap().unwrap().last_used_at_ms,
            Some(expected)
        );
    }
}

#[test]
fn history_indexes_cover_ordering_and_seek_to_usage_cursor() {
    let database = Database::open_in_memory().unwrap();
    database
        .with_connection(|connection| {
            for (favorite, index) in [
                ("", "clipboard_items_deleted_last_used_idx"),
                (
                    " AND is_favorite = 1",
                    "clipboard_items_favorite_last_used_idx",
                ),
            ] {
                for cursor in ["", " AND (last_used_at_ms, id) < (100, 'anchor')"] {
                    let sql = format!(
                        "EXPLAIN QUERY PLAN SELECT {} FROM clipboard_items
                     WHERE deleted = 0{favorite}{cursor}
                     ORDER BY last_used_at_ms DESC, id DESC LIMIT 100",
                        crate::storage::repository::ITEM_COLUMNS
                    );
                    let plan = connection
                        .prepare(&sql)?
                        .query_map([], |row| row.get::<_, String>(3))?
                        .collect::<Result<Vec<_>, _>>()?
                        .join("; ");
                    assert!(plan.contains(index), "{plan}");
                    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
                    if !cursor.is_empty() {
                        assert!(plan.contains("(last_used_at_ms,id)<(?,?)"), "{plan}");
                    }
                }
            }
            Ok(())
        })
        .unwrap();
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
