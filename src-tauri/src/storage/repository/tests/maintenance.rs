//! Concurrency and expired-purge maintenance scenarios.

use super::*;

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

/// Capacity enforcement reads a count and then deletes down to it, so both must
/// share one write transaction.
///
/// The scheduled cleanup worker and the manual cleanup command hold separate
/// `Database` handles, each with its own connection. As two autocommit
/// statements, both read the same pre-trim count and each delete the full
/// excess, so the library loses roughly twice the configured eviction whenever
/// the two overlap.
#[test]
fn concurrent_capacity_enforcement_does_not_over_evict() {
    let path = std::env::temp_dir().join(format!(
        "clipboard-capacity-race-{}-{:016x}.sqlite3",
        std::process::id(),
        rand::random::<u64>()
    ));
    let _ = std::fs::remove_file(&path);

    // Two independent handles on the same file, exactly like the cleanup worker
    // and the cleanup command.
    let worker = std::sync::Arc::new(Database::open(&path).unwrap());
    let command = std::sync::Arc::new(Database::open(&path).unwrap());

    for index in 0..600 {
        worker
            .save_item(&text_item(
                &format!("capacity-{index:04}"),
                &format!("hash-{index:04}"),
                index,
            ))
            .unwrap();
    }
    assert_eq!(worker.item_count().unwrap(), 600);

    // Below the current count, so both runners see a real excess.
    const MAX_ITEMS: u64 = 100;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let runners: Vec<_> = [worker.clone(), command.clone()]
        .into_iter()
        .map(|database| {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                database.enforce_capacity_limit(MAX_ITEMS)
            })
        })
        .collect();

    let evicted: u64 = runners
        .into_iter()
        .map(|runner| runner.join().unwrap().unwrap())
        .sum();

    // The 500 rows above the cap must go and no more. Without the shared
    // transaction each runner derives the same 500-row excess and ~500 extra
    // non-favorite records disappear.
    assert_eq!(
        evicted, 500,
        "only the rows above the cap may be evicted, got {evicted}"
    );
    assert_eq!(
        worker.item_count().unwrap(),
        MAX_ITEMS,
        "the library must settle exactly at the configured cap"
    );

    drop(worker);
    drop(command);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}
