//! Pagination, counts, recycle bin and retention/capacity scenarios.

use super::*;

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
fn restore_refreshes_retention_baseline_against_cleanup() {
    let database = Database::open_in_memory().unwrap();
    let recent_time = crate::storage::repository::helpers::current_time_ms() - 1_000;
    database
        .save_item(&text_item("recent", "hash-recent", recent_time))
        .unwrap();
    // Created far inside the retention window's past.
    database
        .save_item(&text_item("ancient", "hash-ancient", 1_000))
        .unwrap();
    database.soft_delete("ancient").unwrap();
    database.restore_deleted("ancient").unwrap();

    // The restored row must survive retention cleanup instead of being
    // hard-deleted behind the user's back, and its restore moment becomes
    // the new history position.
    assert_eq!(database.delete_older_than(30).unwrap(), 0);
    let restored = database
        .get_item("ancient")
        .unwrap()
        .expect("restored record must survive retention cleanup");
    assert!(restored.created_at_ms > 1_000);
    assert_eq!(restored.last_used_at_ms, Some(restored.created_at_ms));
    assert_eq!(
        database.list_recent(1, 0, &Default::default()).unwrap()[0].id,
        "ancient"
    );

    // Batch restore carries the same guarantee.
    database
        .save_item(&text_item("ancient-2", "hash-ancient-2", 2_000))
        .unwrap();
    database.soft_delete("ancient-2").unwrap();
    database
        .restore_deleted_batch(&["ancient-2".to_owned()])
        .unwrap();
    assert_eq!(database.delete_older_than(30).unwrap(), 0);
    let batch_restored = database.get_item("ancient-2").unwrap().unwrap();
    assert_eq!(
        batch_restored.last_used_at_ms,
        Some(batch_restored.created_at_ms)
    );
    assert!(batch_restored.last_used_at_ms.unwrap() > recent_time);
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
