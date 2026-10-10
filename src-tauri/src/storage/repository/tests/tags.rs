//! Tag registry scenarios plus historical auto-tag application.

use super::*;

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
fn auto_tag_history_preview_is_read_only_and_apply_is_additive_and_idempotent() {
    let db = Database::open_in_memory().unwrap();
    for id in ["a", "b", "deleted"] {
        let mut row = text_item(id, id, 1);
        row.source_app = Some(if id == "b" { "Other" } else { "Editor" }.into());
        db.save_item(&row).unwrap();
    }
    db.soft_delete("deleted").unwrap();
    db.set_tags("a", &["manual".into()]).unwrap();
    let rules = crate::tags::compile_auto_tag_rules(&[crate::config::AutoTagRule {
        tag: "work".into(),
        source_app: "editor".into(),
        ..Default::default()
    }]);
    let preview = db.auto_tag_history(&rules, false).unwrap();
    assert_eq!(preview.changed_count, 1);
    assert_eq!(preview.samples[0].id, "a");
    assert!(!db
        .get_item("a")
        .unwrap()
        .unwrap()
        .metadata_json
        .unwrap()
        .contains("work"));
    assert_eq!(db.auto_tag_history(&rules, true).unwrap().changed_count, 1);
    let metadata: serde_json::Value =
        serde_json::from_str(&db.get_item("a").unwrap().unwrap().metadata_json.unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["manual", "work"]));
    assert_eq!(db.auto_tag_history(&rules, true).unwrap().changed_count, 0);
    assert!(!db.add_tags("a", &["work".into()]).unwrap());
}

#[test]
fn historical_auto_tag_failure_rolls_back_every_record() {
    let db = Database::open_in_memory().unwrap();
    for id in ["a", "b"] {
        db.save_item(&text_item(id, id, 1)).unwrap();
    }
    db.with_connection(|connection| {
        connection.execute_batch(
            "CREATE TRIGGER reject_b BEFORE UPDATE OF metadata_json ON clipboard_items
          WHEN NEW.id = 'b' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )?;
        Ok(())
    })
    .unwrap();
    let rules = crate::tags::compile_auto_tag_rules(&[crate::config::AutoTagRule {
        pattern: "content".into(),
        tag: "auto".into(),
        ..Default::default()
    }]);
    assert!(db.auto_tag_history(&rules, true).is_err());
    assert!(db.list_all_tags().unwrap().is_empty());
    assert!(db.get_item("a").unwrap().unwrap().metadata_json.is_none());
}

#[test]
fn auto_tag_snapshot_releases_gui_connection_and_preserves_concurrent_manual_tags() {
    use crate::storage::AutoTagPhase;
    let root = std::env::temp_dir().join(format!("clipboard-auto-tags-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let db = Database::open(root.join("history.db")).unwrap();
    for id in ["a", "b"] {
        db.save_item(&text_item(id, id, 1)).unwrap();
    }
    let rules = crate::tags::compile_auto_tag_rules(&[crate::config::AutoTagRule {
        pattern: "content".into(),
        tag: "auto".into(),
        ..Default::default()
    }]);
    let mut inserted = false;
    let mut tagged = false;
    let result = db
        .auto_tag_history_with_progress(&rules, true, |progress| {
            if progress.phase == AutoTagPhase::Scanning && progress.completed == 1 && !inserted {
                // This would deadlock on the former shared-connection scan. The existing snapshot
                // must also remain stable while another connection adds matching content.
                db.save_item(&text_item("new", "new", 2))?;
                inserted = true;
            }
            if progress.phase == AutoTagPhase::Applying && progress.completed == 0 && !tagged {
                db.set_tags("a", &["manual".into()])?;
                tagged = true;
            }
            Ok(())
        })
        .unwrap();
    assert!(inserted && tagged);
    assert_eq!(result.matched_count, 2);
    assert_eq!(result.changed_count, 2);
    assert!(db.get_item("new").unwrap().unwrap().metadata_json.is_none());
    let metadata: serde_json::Value =
        serde_json::from_str(&db.get_item("a").unwrap().unwrap().metadata_json.unwrap()).unwrap();
    assert_eq!(metadata["tags"], serde_json::json!(["manual", "auto"]));
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn auto_tag_cancellation_at_final_apply_row_rolls_back_and_allows_retry() {
    use crate::storage::AutoTagPhase;
    let db = Database::open_in_memory().unwrap();
    for id in ["a", "b"] {
        db.save_item(&text_item(id, id, 1)).unwrap();
    }
    let rules = crate::tags::compile_auto_tag_rules(&[crate::config::AutoTagRule {
        pattern: "content".into(),
        tag: "auto".into(),
        ..Default::default()
    }]);
    let result = db.auto_tag_history_with_progress(&rules, true, |progress| {
        if progress.phase == AutoTagPhase::Applying && progress.completed == progress.total {
            return Err(StorageError::Io(std::io::Error::other("cancelled")));
        }
        Ok(())
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(db.list_all_tags().unwrap().is_empty());
    assert_eq!(db.auto_tag_history(&rules, true).unwrap().changed_count, 2);
}

#[test]
fn auto_tag_changed_matching_inputs_abort_application_without_partial_tags() {
    use crate::storage::AutoTagPhase;
    let root =
        std::env::temp_dir().join(format!("clipboard-auto-tags-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let db = Database::open(root.join("history.db")).unwrap();
    for id in ["a", "b"] {
        db.save_item(&text_item(id, id, 1)).unwrap();
    }
    let rules = crate::tags::compile_auto_tag_rules(&[crate::config::AutoTagRule {
        pattern: "content".into(),
        tag: "auto".into(),
        ..Default::default()
    }]);
    let result = db.auto_tag_history_with_progress(&rules, true, |progress| {
        if progress.phase == AutoTagPhase::Applying && progress.completed == 0 {
            db.with_connection(|conn| {
                conn.execute(
                    "UPDATE clipboard_items SET source_app = 'changed' WHERE id = 'b'",
                    [],
                )?;
                Ok(())
            })?;
        }
        Ok(())
    });
    assert!(result.unwrap_err().to_string().contains("history changed"));
    assert!(db.list_all_tags().unwrap().is_empty());
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
