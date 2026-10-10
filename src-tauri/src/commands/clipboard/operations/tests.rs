//! Unit tests for the clipboard command helpers, moved verbatim with the
//! command surface.

use super::{
    apply_sort_rules, cmp_by_field, duplicate_clipboard_item_record, generated_clipboard_title,
    is_owned_rename_source, metadata_custom_title, record_item_usage, rename_item_record,
    resolve_custom_title, rewrite_stored_resource_paths, sanitize_file_stem,
    save_clipboard_item_as_new_record, set_custom_title_metadata,
};
use crate::commands::clipboard::types::{
    SearchResultCache, SearchSortDirection, SearchSortField, SearchSortRule,
};
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::ClipboardRepository;
use crate::storage::{Database, StoragePaths};

fn item(id: &str, title: &str) -> ClipboardItem {
    ClipboardItem {
        id: id.to_owned(),
        kind: ClipboardKind::Text,
        title: title.to_owned(),
        text_content: None,
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: format!("hash-{id}"),
        source_app: None,
        icon_path: None,
        size_bytes: 0,
        created_at_ms: 0,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: None,
    }
}

#[test]
fn clearing_history_announces_only_rows_soft_deleted_by_its_commit() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&item("active", "active")).unwrap();
    database
        .save_item(&item("already-deleted", "deleted"))
        .unwrap();
    database.soft_delete("already-deleted").unwrap();
    let mut favorite = item("favorite", "favorite");
    favorite.is_favorite = true;
    database.save_item(&favorite).unwrap();
    let count = super::clear_non_favorite_records(&database, |payload| {
        assert_eq!(payload.deleted_ids, vec!["active"]);
        assert!(payload.removed_ids.is_empty());
        assert_eq!(database.list_deleted(10, 0).unwrap().len(), 2);
        assert!(database.get_item("favorite").unwrap().unwrap().is_favorite);
    })
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        super::clear_non_favorite_records(&database, |_| {
            panic!("an unchanged history must not notify");
        })
        .unwrap(),
        0
    );
    database
        .with_connection(|connection| {
            connection.execute_batch("DROP TABLE clipboard_items")?;
            Ok(())
        })
        .unwrap();
    assert!(super::clear_non_favorite_records(&database, |_| {
        panic!("a failed clear must not notify");
    })
    .is_err());
}

#[test]
fn hard_delete_announces_only_a_committed_removal() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&item("remove", "remove")).unwrap();
    let mut favorite = item("favorite", "favorite");
    favorite.is_favorite = true;
    database.save_item(&favorite).unwrap();
    let mut notifications = Vec::new();
    assert!(
        super::delete_clipboard_item_record(&database, "remove", |payload| {
            assert!(database.get_item("remove").unwrap().is_none());
            assert!(payload.deleted_ids.is_empty());
            assert!(payload.items.is_empty());
            notifications.push(payload.removed_ids);
        })
        .unwrap()
    );
    assert_eq!(notifications, vec![vec!["remove"]]);
    assert!(
        !super::delete_clipboard_item_record(&database, "remove", |_| {
            panic!("missing row must not announce removal")
        })
        .unwrap()
    );
    assert!(
        super::delete_clipboard_item_record(&database, "favorite", |_| {
            panic!("failed deletion must not announce removal")
        })
        .is_err()
    );
    assert!(database.get_item("favorite").unwrap().is_some());
}

#[test]
fn duplicate_namespaces_hash_and_stays_unique_when_called_twice() {
    let database = Database::open_in_memory().unwrap();
    let mut source = item("src", "title");
    source.content_hash = "abc".to_owned();
    source.text_content = Some("hello".to_owned());
    database.save_item(&source).unwrap();

    let first = duplicate_clipboard_item_record(&database, "src").unwrap();
    let second = duplicate_clipboard_item_record(&database, "src").unwrap();
    assert_eq!(first.last_used_at_ms, Some(first.created_at_ms));
    assert_eq!(second.last_used_at_ms, Some(second.created_at_ms));

    assert_ne!(first.id, second.id);
    assert_ne!(first.content_hash, second.content_hash);
    assert_ne!(first.content_hash, "abc");
    assert_eq!(
        database.get_item("src").unwrap().unwrap().content_hash,
        "abc"
    );
    assert!(database.get_item(&first.id).unwrap().is_some());
    assert!(database.get_item(&second.id).unwrap().is_some());
}

#[test]
fn save_as_new_persists_unchanged_content_without_hash_collision() {
    let database = Database::open_in_memory().unwrap();
    let mut source = item("src", "title");
    source.content_hash = "real-hash".to_owned();
    source.text_content = Some("same".to_owned());
    database.save_item(&source).unwrap();

    let created = save_clipboard_item_as_new_record(&database, "src", "title2", "same").unwrap();
    assert_eq!(created.last_used_at_ms, Some(created.created_at_ms));

    assert_ne!(created.id, "src");
    assert_eq!(created.text_content.as_deref(), Some("same"));
    assert_ne!(created.content_hash, "real-hash");
    assert_eq!(
        database.get_item("src").unwrap().unwrap().content_hash,
        "real-hash"
    );
    assert!(database.get_item(&created.id).unwrap().is_some());
}

#[test]
fn save_as_new_uses_real_hash_for_fresh_content() {
    let database = Database::open_in_memory().unwrap();
    let mut source = item("src", "title");
    source.content_hash = "real-hash".to_owned();
    source.text_content = Some("old".to_owned());
    database.save_item(&source).unwrap();

    let created =
        save_clipboard_item_as_new_record(&database, "src", "title", "brand new").unwrap();
    let expected = crate::content::hash::compute_content_hash("text", "brand new", None);
    assert_eq!(created.content_hash, expected);
}

#[test]
fn save_as_new_rejects_empty_text_and_media_kinds() {
    let database = Database::open_in_memory().unwrap();
    let mut source = item("src", "title");
    source.text_content = Some("body".to_owned());
    database.save_item(&source).unwrap();
    assert!(save_clipboard_item_as_new_record(&database, "src", "t", "   ").is_err());

    let mut media = item("img", "shot");
    media.kind = ClipboardKind::Image;
    database.save_item(&media).unwrap();
    assert!(save_clipboard_item_as_new_record(&database, "img", "t", "text").is_err());
}

#[test]
fn replaces_path_separators_and_windows_specials() {
    // Separators become underscores, so the name can never traverse out
    // of the managed directory; leading dots are harmless (same dir).
    assert_eq!(sanitize_file_stem("..\\..\\evil"), ".._.._evil");
    assert_eq!(
        sanitize_file_stem("a/b:c*d?e\"f<g>h|i"),
        "a_b_c_d_e_f_g_h_i"
    );
}

#[test]
fn traversal_only_names_collapse_to_empty() {
    assert_eq!(sanitize_file_stem(".."), "");
    assert_eq!(sanitize_file_stem("."), "");
    assert_eq!(sanitize_file_stem("..."), "");
}

#[test]
fn keeps_unicode_display_names() {
    assert_eq!(sanitize_file_stem("截图 2026"), "截图 2026");
    assert_eq!(sanitize_file_stem("report-v2_final"), "report-v2_final");
}

#[test]
fn rejects_windows_reserved_device_names() {
    assert_eq!(sanitize_file_stem("CON"), "_");
    assert_eq!(sanitize_file_stem("com1"), "_");
    assert_eq!(sanitize_file_stem("LPT4"), "_");
    assert_eq!(sanitize_file_stem("combo"), "combo");
}

#[test]
fn rename_rewrites_managed_paths_in_metadata_and_file_list() {
    let mut record = item("file-1", "old");
    record.kind = ClipboardKind::File;
    record.resource_path = Some("/store/files/old.txt".to_owned());
    record.preview_path = Some("/store/files/old.txt".to_owned());
    record.metadata_json = Some(
        r#"{"resourcePath":"/store/files/old.txt","files":[{"storagePath":"/store/files/old.txt"},{"storagePath":"/store/files/other.txt"}]}"#
            .to_owned(),
    );
    record.text_content = Some(r#"["/store/files/old.txt","/store/files/other.txt"]"#.to_owned());

    rewrite_stored_resource_paths(&mut record, "/store/files/old.txt", "/store/files/new.txt");

    let metadata: serde_json::Value =
        serde_json::from_str(record.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["resourcePath"], "/store/files/new.txt");
    assert_eq!(metadata["files"][0]["storagePath"], "/store/files/new.txt");
    assert_eq!(
        metadata["files"][1]["storagePath"],
        "/store/files/other.txt"
    );
    assert_eq!(
        record.text_content.as_deref(),
        Some(r#"["/store/files/new.txt","/store/files/other.txt"]"#)
    );
}

#[test]
fn trims_trailing_dots_and_spaces() {
    assert_eq!(sanitize_file_stem("name. "), "name");
}

#[test]
fn rename_keeps_generated_thumbnail_preview_path() {
    let project = std::env::temp_dir().join(format!(
        "clipboard-rename-preview-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let paths = crate::storage::StoragePaths::initialize(project.clone()).unwrap();
    let database = Database::open(&paths.database).unwrap();

    let images_dir = paths.images.clone();
    std::fs::create_dir_all(&images_dir).unwrap();
    let old_path = images_dir.join("old.png");
    std::fs::write(&old_path, b"png").unwrap();

    // A generated thumbnail (preview_path != resource_path) must survive
    // the rename: overwriting it would drop the preview link and orphan
    // the file under previews/.
    let mut record = item("img-1", "old");
    record.kind = ClipboardKind::Image;
    record.resource_path = Some(old_path.display().to_string());
    record.preview_path = Some("/store/previews/thumb.jpg".to_owned());
    database.save_item(&record).unwrap();

    let renamed =
        rename_item_record(&database, &paths, "img-1".to_owned(), "new".to_owned()).unwrap();
    let new_path = images_dir.join("new.png");
    assert_eq!(
        renamed.resource_path.as_deref(),
        Some(new_path.to_str().unwrap())
    );
    assert_eq!(
        renamed.preview_path.as_deref(),
        Some("/store/previews/thumb.jpg")
    );
    assert!(new_path.exists());
    assert!(!old_path.exists());

    // A fallback preview (thumbnail not yet generated) follows the file.
    let mut fallback = item("img-2", "old2");
    fallback.kind = ClipboardKind::Image;
    let old2 = images_dir.join("old2.png");
    std::fs::write(&old2, b"png").unwrap();
    fallback.resource_path = Some(old2.display().to_string());
    fallback.preview_path = fallback.resource_path.clone();
    database.save_item(&fallback).unwrap();
    let renamed2 =
        rename_item_record(&database, &paths, "img-2".to_owned(), "new2".to_owned()).unwrap();
    let new2 = images_dir.join("new2.png");
    assert_eq!(
        renamed2.preview_path.as_deref(),
        Some(new2.to_str().unwrap())
    );

    drop(database);
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
fn rename_preserves_external_directories_unclaimed_and_shared_files() {
    let project =
        std::env::temp_dir().join(format!("clipboard-rename-owned-{}", uuid::Uuid::new_v4()));
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let external = project.join("external.txt");
    std::fs::write(&external, b"original bytes").unwrap();
    let directory = paths.files.join("folder");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("child"), b"keep").unwrap();
    let shared = paths.files.join("shared.txt");
    std::fs::write(&shared, b"shared bytes").unwrap();
    let mut other = item("other", "other");
    other.resource_path = Some(shared.display().to_string());
    database.save_item(&other).unwrap();
    let custom = project.with_extension("custom");
    let custom_paths = StoragePaths::initialize_with_resource_directories(
        project.clone(),
        None,
        None,
        Some(custom.clone()),
    )
    .unwrap();
    let unclaimed = custom.join("unclaimed.txt");
    std::fs::write(&unclaimed, b"custom bytes").unwrap();
    for (id, source, roots) in [
        ("external", &external, &paths),
        ("directory", &directory, &paths),
        ("shared", &shared, &paths),
        ("unclaimed", &unclaimed, &custom_paths),
    ] {
        let mut record = item(id, id);
        record.kind = ClipboardKind::File;
        record.resource_path = Some(source.display().to_string());
        // Metadata cannot grant ownership of a pass-through path.
        record.metadata_json = Some(r#"{"copied":true}"#.into());
        database.save_item(&record).unwrap();
        let renamed =
            rename_item_record(&database, roots, id.into(), "display only".into()).unwrap();
        assert_eq!(renamed.title, "display only");
        assert_eq!(renamed.resource_path, record.resource_path);
        assert!(source.exists());
    }
    assert_eq!(std::fs::read(&external).unwrap(), b"original bytes");
    assert_eq!(std::fs::read(directory.join("child")).unwrap(), b"keep");
    // A claimed custom root works, but revoking its marker takes effect immediately.
    custom_paths
        .claim_resource_root(crate::storage::ResourceRootRole::File)
        .unwrap();
    let claimed = StoragePaths::initialize_with_resource_directories(
        project.clone(),
        None,
        None,
        Some(custom.clone()),
    )
    .unwrap();
    assert!(is_owned_rename_source(&claimed, &unclaimed));
    claimed
        .remove_resource_root_marker(crate::storage::ResourceRootRole::File)
        .unwrap();
    assert!(!is_owned_rename_source(&claimed, &unclaimed));
    std::fs::remove_dir_all(project).unwrap();
    std::fs::remove_dir_all(custom).unwrap();
}

fn rule(field: SearchSortField, direction: SearchSortDirection) -> SearchSortRule {
    SearchSortRule { field, direction }
}

#[test]
fn cmp_by_field_orders_descending_by_default_for_recency_fields() {
    let mut older = item("a", "a");
    older.created_at_ms = 100;
    let mut newer = item("b", "b");
    newer.created_at_ms = 200;

    assert_eq!(
        cmp_by_field(&newer, &older, SearchSortField::CreatedAt),
        std::cmp::Ordering::Less
    );

    // Usage is initialized at persistence, not derived during sorting.
    newer.last_used_at_ms = Some(50);
    older.last_used_at_ms = Some(100);
    older.created_at_ms = 300;
    assert_eq!(
        cmp_by_field(&older, &newer, SearchSortField::LastUsedAt),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        cmp_by_field(&newer, &older, SearchSortField::LastUsedAt),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn cmp_by_field_uses_last_used_without_comparing_creation() {
    let mut recaptured = item("a", "a");
    recaptured.created_at_ms = 300;
    recaptured.last_used_at_ms = Some(150);
    let mut other = item("b", "b");
    other.created_at_ms = 200;
    other.last_used_at_ms = Some(200);

    assert_eq!(
        cmp_by_field(&recaptured, &other, SearchSortField::LastUsedAt),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        cmp_by_field(&other, &recaptured, SearchSortField::LastUsedAt),
        std::cmp::Ordering::Less
    );
}

#[test]
fn cmp_by_field_covers_title_size_kind_and_favorite() {
    let mut small = item("s", "beta");
    small.size_bytes = 10;
    let mut big = item("b", "alpha");
    big.size_bytes = 500;
    big.is_favorite = true;
    big.kind = ClipboardKind::Image;

    assert_eq!(
        cmp_by_field(&small, &big, SearchSortField::Title),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        cmp_by_field(&big, &small, SearchSortField::Size),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        cmp_by_field(&big, &small, SearchSortField::Kind),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        cmp_by_field(&big, &small, SearchSortField::Favorite),
        std::cmp::Ordering::Less
    );
}

#[test]
fn title_asc_sorts_lexicographically_after_the_direction_fix() {
    let alpha = item("a", "Alpha");
    let beta = item("b", "Beta");

    // Asc must be A→Z now that every field shares a descending base.
    let mut items = vec![beta.clone(), alpha.clone()];
    apply_sort_rules(
        &mut items,
        &[rule(SearchSortField::Title, SearchSortDirection::Asc)],
    );
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );

    // Desc is Z→A.
    let mut reversed = vec![alpha, beta];
    apply_sort_rules(
        &mut reversed,
        &[rule(SearchSortField::Title, SearchSortDirection::Desc)],
    );
    assert_eq!(
        reversed
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
}

#[test]
fn apply_sort_rules_sorts_multi_key_with_direction() {
    let mut first = item("1", "b");
    first.created_at_ms = 100;
    first.is_favorite = false;
    let mut second = item("2", "a");
    second.created_at_ms = 100;
    second.is_favorite = true;
    let mut third = item("3", "c");
    third.created_at_ms = 300;

    let mut items = vec![first, second, third];
    apply_sort_rules(
        &mut items,
        &[rule(SearchSortField::CreatedAt, SearchSortDirection::Asc)],
    );
    // Ascending recency = oldest entries first.
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );

    // Favorite desc wins over the title asc tiebreak for items 1 and 2.
    apply_sort_rules(
        &mut items,
        &[
            rule(SearchSortField::Favorite, SearchSortDirection::Desc),
            rule(SearchSortField::Title, SearchSortDirection::Asc),
        ],
    );
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["2", "1", "3"]
    );
}

#[test]
fn apply_sort_rules_keeps_order_without_rules() {
    let mut items = vec![item("x", "x"), item("a", "a")];
    apply_sort_rules(&mut items, &[]);
    assert_eq!(items[0].id, "x");
}

/// Module scenario for the usage-stamp staleness: the default history
/// sort is `lastUsedAt desc`, a usage stamp writes no `search_outbox`
/// event, and `SearchResultCache` therefore outlives the re-ranking the
/// stamp should cause. After pasting an entry out of history, the next
/// cached page must no longer serve the pre-usage order.
#[test]
fn usage_stamp_invalidates_the_cached_last_used_page() {
    let database = Database::open_in_memory().unwrap();
    let mut older = item("older", "record-older");
    older.text_content = Some("content-older".to_owned());
    older.created_at_ms = 100;
    let mut newer = item("newer", "record-newer");
    newer.text_content = Some("content-newer".to_owned());
    newer.created_at_ms = 200;
    database.save_item(&older).unwrap();
    database.save_item(&newer).unwrap();

    // Simulate the search pipeline for the default sort: fetch, sort,
    // cache the full sorted result.
    let rules = vec![SearchSortRule {
        field: SearchSortField::LastUsedAt,
        direction: SearchSortDirection::Desc,
    }];
    let ids = vec!["older".to_owned(), "newer".to_owned()];
    let mut sorted = database.get_items_by_ids(&ids).unwrap();
    apply_sort_rules(&mut sorted, &rules);
    assert_eq!(
        sorted
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["newer", "older"]
    );

    let cache = SearchResultCache::new();
    cache.set(
        cache.write_token(),
        String::new(),
        rules.clone(),
        100,
        sorted,
        2,
        false,
    );
    assert_eq!(cache.get("", &rules, 100, 0, 10).unwrap().0[0].id, "newer");

    // The user pastes the older entry out of history; the usage entry
    // point must drop the cached page since no outbox event fires.
    let updated = record_item_usage(&database, &cache, "older").unwrap();
    assert!(updated);

    // The cached page must be gone now; while it survives, scrolling
    // serves the stale pre-usage order even though the entry has been
    // re-ranked in the database.
    assert!(cache.get("", &rules, 100, 0, 10).is_none());

    // A cache-miss re-query re-sorts with the used entry on top.
    let mut resorted = database.get_items_by_ids(&ids).unwrap();
    apply_sort_rules(&mut resorted, &rules);
    assert_eq!(
        resorted
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["older", "newer"]
    );
}

#[test]
fn changing_the_candidate_cap_invalidates_sorted_results_in_both_directions() {
    let cache = SearchResultCache::new();
    cache.set(
        cache.write_token(),
        "query".into(),
        vec![],
        500,
        vec![item("low-relevance", "first-after-custom-sort")],
        600,
        true,
    );
    assert!(cache.get("query", &[], 500, 0, 100).is_some());
    // Prefix slicing cannot shrink a relevance-capped result after custom
    // sorting: this first row may not be among the top 100 candidates.
    assert!(cache.get("query", &[], 100, 0, 100).is_none());
    assert!(cache.get("query", &[], 1000, 0, 100).is_none());
}

#[test]
fn oversized_search_results_are_returnable_but_not_retained() {
    let cache = SearchResultCache::new();
    let mut huge = item("large", "large");
    huge.metadata_json = Some("x".repeat(16 * 1024 * 1024));
    cache.set(
        cache.write_token(),
        "large".into(),
        vec![],
        100,
        vec![huge],
        1,
        false,
    );
    assert!(cache.get("large", &[], 100, 0, 100).is_none());
}

#[test]
fn usage_stamp_prevents_inflight_search_from_repopulating_stale_cache() {
    let database = Database::open_in_memory().unwrap();
    database.save_item(&item("record", "title")).unwrap();
    let cache = SearchResultCache::new();
    let token = cache.write_token();
    let snapshot = database.get_items_by_ids(&["record".to_owned()]).unwrap();
    assert!(record_item_usage(&database, &cache, "record").unwrap());
    // This is the completion of a search that read before the usage stamp.
    cache.set(
        token,
        "title".to_owned(),
        Vec::new(),
        100,
        snapshot,
        1,
        false,
    );
    assert!(cache.get("title", &[], 100, 0, 10).is_none());
    let fresh = database.get_items_by_ids(&["record".to_owned()]).unwrap();
    cache.set(
        cache.write_token(),
        "title".to_owned(),
        Vec::new(),
        100,
        fresh,
        1,
        false,
    );
    assert!(cache.get("title", &[], 100, 0, 10).is_some());
}

/// Module scenario for tie ordering: the search pipeline feeds items in
/// Tantivy relevance order and the reference strategy documents that
/// order as the fallback for equal sort keys. With `sort_unstable_by`
/// the tie groups of a larger input are permuted, so the documented
/// fallback only holds deterministically under a stable sort.
#[test]
fn apply_sort_rules_keeps_incoming_order_for_tied_keys() {
    // 160 items in 40 ascending size groups of four. Sorting by size
    // desc must fully reverse the group order while every tie group
    // keeps its incoming (relevance) sequence.
    let mut items: Vec<ClipboardItem> = (0..160)
        .map(|index| {
            let mut entry = item(&format!("id-{index:03}"), "record");
            entry.size_bytes = (index / 4) as u64;
            entry
        })
        .collect();
    apply_sort_rules(
        &mut items,
        &[rule(SearchSortField::Size, SearchSortDirection::Desc)],
    );

    let expected: Vec<String> = (0..40)
        .rev()
        .flat_map(|group| (0..4).map(move |offset| format!("id-{:03}", group * 4 + offset)))
        .collect();
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn generated_clipboard_title_truncates_by_chars_not_bytes() {
    let ascii = "k".repeat(500);
    assert_eq!(generated_clipboard_title(&ascii).len(), 200);

    // 300 CJK characters (900 bytes): byte slicing would panic or split a
    // code point; char-based truncation yields exactly 200 chars.
    let cjk: String = "剪".repeat(300);
    let title = generated_clipboard_title(&cjk);
    assert_eq!(title.chars().count(), 200);
}

#[test]
fn metadata_custom_title_reads_only_boolean_flags() {
    assert_eq!(metadata_custom_title(None), None);
    assert_eq!(metadata_custom_title(Some("not-json")), None);
    assert_eq!(
        metadata_custom_title(Some(r#"{"customTitle":true}"#)),
        Some(true)
    );
    assert_eq!(
        metadata_custom_title(Some(r#"{"customTitle":false}"#)),
        Some(false)
    );
    // Non-boolean values must not be coerced into a decision.
    assert_eq!(
        metadata_custom_title(Some(r#"{"customTitle":"yes"}"#)),
        None
    );
}

#[test]
fn resolve_custom_title_prefers_metadata_then_compares_titles() {
    assert!(resolve_custom_title(
        "My title",
        "body",
        Some(r#"{"customTitle":true}"#)
    ));
    assert!(!resolve_custom_title(
        "My title",
        "body",
        Some(r#"{"customTitle":false}"#)
    ));

    // Without metadata the generated title decides.
    let body = "hello";
    assert!(!resolve_custom_title(body, body, None));
    assert!(resolve_custom_title("edited", body, None));
}

#[test]
fn set_custom_title_metadata_preserves_existing_fields() {
    let updated = set_custom_title_metadata(Some(r#"{"tags":["work"],"n":1}"#), true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(value["customTitle"], serde_json::Value::Bool(true));
    assert_eq!(value["tags"], serde_json::json!(["work"]));
    assert_eq!(value["n"], serde_json::json!(1));

    // A non-object payload is replaced rather than corrupted.
    let replaced = set_custom_title_metadata(Some("[1,2]"), false).unwrap();
    let replaced_value: serde_json::Value = serde_json::from_str(&replaced).unwrap();
    assert_eq!(
        replaced_value["customTitle"],
        serde_json::Value::Bool(false)
    );

    assert!(set_custom_title_metadata(None, true).is_ok());
}
