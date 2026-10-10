//! Import size/version limits, search filtering and summary bounds.

use super::*;

/// An import may not hand the replication version clock a saturated value.
///
/// `created_at_ms` is copied straight into `modified_at_ms` by the insert
/// trigger, and SQLite promotes an overflowing `+ 1` in the update trigger to
/// REAL. A REAL version is unreadable as `i64` by
/// `sync_state::winning_local_version`, so a single poisoned row aborted every
/// later sync apply — a permanent, invisible failure for the user. The import
/// funnel now rejects a timestamp that is negative or implausibly far ahead, and
/// the triggers saturate so a value that is already at the ceiling can still
/// never change type.
#[test]
fn an_import_cannot_saturate_the_sync_version_clock() {
    let database = Database::open_in_memory().unwrap();
    enable_sync_versioning(&database);

    let summary = database
        .save_items_transactional(&[
            (
                "saturated.csv".to_owned(),
                text_item("poison", "hash-poison", i64::MAX),
            ),
            ("negative.json".to_owned(), text_item("neg", "hash-neg", -1)),
            (
                "valid.csv".to_owned(),
                text_item("valid", "hash-valid", current_time_ms() - 60_000),
            ),
        ])
        .unwrap();

    assert_eq!(summary.imported_count, 1, "only the sane row may import");
    assert_eq!(summary.skipped_count, 2);
    assert!(
        summary
            .errors
            .iter()
            .any(|e| e.contains("saturated.csv") && e.contains("too far in the future")),
        "the saturated row must be reported: {:?}",
        summary.errors
    );
    assert!(
        summary
            .errors
            .iter()
            .any(|e| e.contains("negative.json") && e.contains("negative")),
        "the negative row must be reported: {:?}",
        summary.errors
    );
    assert_eq!(database.item_count().unwrap(), 1);

    // The surviving row is a normal version, not a poisoned one.
    let version = read_version(&database, "valid").expect("a normal version reads as i64");
    assert!(version < i64::MAX / 2, "unexpected version {version}");
}

/// The saturation must hold even for a row that reached the ceiling before this
/// fix existed: a watched-column update may advance the version, but it may never
/// change its SQLite type.
#[test]
fn a_saturated_version_never_becomes_a_real_column() {
    let database = Database::open_in_memory().unwrap();
    enable_sync_versioning(&database);

    // Plant the poisoned row directly, as an older build would have written it.
    database
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO clipboard_items (id, kind, title, content_hash, size_bytes, created_at_ms, modified_at_ms)
                 VALUES ('legacy', 'text', 'legacy', 'hash-legacy', 12, 9223372036854775807, 9223372036854775807)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        column_type(&database, "legacy", "modified_at_ms"),
        "integer"
    );

    // `is_favorite` is a watched column, so the update trigger fires.
    database.set_favorite("legacy", true).unwrap();

    assert_eq!(
        column_type(&database, "legacy", "modified_at_ms"),
        "integer",
        "the version must saturate, not overflow into REAL"
    );
    let version = read_version(&database, "legacy").expect("the version must stay readable as i64");
    assert_eq!(version, i64::MAX, "saturation pins the value at i64::MAX");
}

/// The capture limit only ever applies to what the clipboard produced, so an
/// import was a way around it and straight into the search index.
#[test]
fn an_import_cannot_exceed_the_capture_size_limit() {
    let database = Database::open_in_memory().unwrap();
    let oversized = "a".repeat(10_000_001);
    let mut item = text_item("huge", "hash-huge", current_time_ms());
    item.text_content = Some(oversized);
    let mut titled = text_item("titled", "hash-titled", current_time_ms());
    titled.title = "t".repeat(4_097);

    let summary = database
        .save_items_transactional(&[
            ("huge.json".to_owned(), item),
            ("titled.json".to_owned(), titled),
        ])
        .unwrap();

    assert_eq!(summary.imported_count, 0);
    assert_eq!(summary.skipped_count, 2);
    assert!(
        summary.errors.iter().any(|e| e.contains("text_content")),
        "the oversized body must be reported: {:?}",
        summary.errors
    );
    assert!(
        summary.errors.iter().any(|e| e.contains("title")),
        "the oversized title must be reported: {:?}",
        summary.errors
    );
    assert_eq!(database.item_count().unwrap(), 0);
}

#[test]
fn search_filter_ids_applies_every_axis_without_loading_payloads() {
    let db = Database::open_in_memory().unwrap();
    for i in 0..7 {
        let id = format!("filter-{i}");
        let mut item = text_item(&id, &id, 100);
        item.is_favorite = i != 1;
        item.source_app = Some(if i == 2 { "Other" } else { "Editor" }.into());
        if i == 3 {
            item.kind = ClipboardKind::Link;
        }
        if i == 4 {
            item.created_at_ms = 200;
        }
        db.save_item(&item).unwrap();
        if i != 5 {
            db.set_tags(&id, &["work".into()]).unwrap();
        }
        if i == 6 {
            db.set_favorite(&id, false).unwrap();
            db.soft_delete(&id).unwrap();
        }
    }
    let filter = HistoryFilter {
        kind: Some(ClipboardKind::Text),
        favorite_only: true,
        tag: Some("work".into()),
        source_app: Some("Editor".into()),
        date_from_ms: Some(100),
        date_to_ms: Some(100),
        cursor: None,
    };
    assert_eq!(db.search_filter_ids(&filter).unwrap(), vec!["filter-0"]);
    assert_eq!(db.list_recent(10, 0, &filter).unwrap()[0].id, "filter-0");
}

#[test]
fn list_summaries_bound_unicode_bodies_and_full_reads_preserve_payloads() {
    let db = Database::open_in_memory().unwrap();
    let text = "长".repeat(100_000);
    let mut item = text_item("large", "large-hash", 100);
    item.text_content = Some(text.clone());
    item.html_content = Some(format!("<pre>{text}</pre>"));
    item.rtf_content = Some(text.clone());
    db.save_item(&item).unwrap();
    let page = db
        .list_summaries(1, 0, &HistoryFilter::default(), false)
        .unwrap();
    assert_eq!(page[0].text_content.as_ref().unwrap().chars().count(), 2048);
    assert_eq!(page[0].html_content.as_deref(), Some(""));
    assert_eq!(page[0].rtf_content.as_deref(), Some(""));
    assert_eq!(
        db.get_summary_items_by_ids(&["large".into()]).unwrap(),
        page
    );
    let full = db.get_item("large").unwrap().unwrap();
    assert_eq!(full.text_content.as_deref(), Some(text.as_str()));
    assert_eq!(full.html_content, item.html_content);
    assert_eq!(full.rtf_content, item.rtf_content);
    db.soft_delete("large").unwrap();
    assert!(db
        .list_summaries(10, 0, &HistoryFilter::default(), false)
        .unwrap()
        .is_empty());
    assert_eq!(
        db.list_summaries(10, 0, &HistoryFilter::default(), true)
            .unwrap(),
        page
    );
}
