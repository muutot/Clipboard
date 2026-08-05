#[cfg(test)]
mod integration_tests {
    use clipboard_desktop_lib::domain::{
        ClipboardItem, ClipboardKind, OcrResult, OcrStatus, OcrTextBlock,
    };
    use clipboard_desktop_lib::search::{SearchIndex, SearchSynchronizer};
    use clipboard_desktop_lib::storage::{ClipboardRepository, Database, OcrRepository};

    fn text_item(id: &str, content: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: format!("title-{id}"),
            text_content: Some(content.to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            icon_path: None,
            size_bytes: content.len() as u64,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    fn image_item(id: &str, image_hash: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Image,
            title: format!("screenshot-{id}.png"),
            text_content: None,
            html_content: None,
            resource_path: Some(format!("images/{id}.png")),
            preview_path: None,
            content_hash: image_hash.to_owned(),
            source_app: Some("test-suite".to_owned()),
            icon_path: None,
            size_bytes: 1024,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    fn completed_ocr_result(item_id: &str, full_text: &str) -> OcrResult {
        OcrResult {
            item_id: item_id.to_owned(),
            status: OcrStatus::Completed,
            engine: "test-engine".to_owned(),
            model_version: "1".to_owned(),
            language: Some("zh-CN".to_owned()),
            full_text: full_text.to_owned(),
            blocks: vec![OcrTextBlock {
                text: full_text.to_owned(),
                confidence: 0.95,
                left: 0,
                top: 0,
                width: 100,
                height: 20,
            }],
            image_hash: format!("hash-{item_id}"),
            created_at_ms: 100,
            completed_at_ms: Some(200),
            error_message: None,
        }
    }

    // ── Full pipeline: insert → search → verify ──

    #[test]
    fn full_pipeline_insert_search_verify() {
        let database = Database::open_in_memory().unwrap();
        let index = SearchIndex::in_memory().unwrap();

        database
            .save_item(&text_item("a", "这是一条中文记录", 100))
            .unwrap();
        database
            .save_item(&text_item("b", "this is an english record", 200))
            .unwrap();

        let summary = SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();
        assert_eq!(summary.upserted_documents, 2);

        let chinese_hits = index.search("中文", 20).unwrap();
        assert_eq!(chinese_hits.len(), 1);
        assert_eq!(chinese_hits[0].item_id, "a");

        let english_hits = index.search("english", 20).unwrap();
        assert_eq!(english_hits.len(), 1);
        assert_eq!(english_hits[0].item_id, "b");

        let all_hits = index.search("记录", 20).unwrap();
        assert!(!all_hits.is_empty());
        let record_ids: Vec<&str> = all_hits.iter().map(|h| h.item_id.as_str()).collect();
        assert!(record_ids.contains(&"a"));

        let items = database
            .get_items_by_ids(&["b".to_owned(), "a".to_owned()])
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "b");
        assert_eq!(items[1].id, "a");
    }

    // ── OCR pipeline: insert image → enqueue → claim → save → search hits ──

    #[test]
    fn ocr_pipeline_image_to_searchable_text() {
        let database = Database::open_in_memory().unwrap();
        let index = SearchIndex::in_memory().unwrap();

        database
            .save_item(&image_item("img", "img-hash", 100))
            .unwrap();
        assert!(database.enqueue_ocr("img").unwrap());

        let claimed = database.claim_next_ocr().unwrap().unwrap();
        assert_eq!(claimed.item_id, "img");

        database
            .save_ocr_result(&completed_ocr_result("img", "图片中的文字内容"))
            .unwrap();

        SearchSynchronizer::default()
            .sync_until_idle(&database, &index)
            .unwrap();

        let hits = index.search("图片 文字", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item_id, "img");
        assert_eq!(hits[0].kind, "image");

        let non_hits = index.search("不存在的内容", 20).unwrap();
        assert!(non_hits.is_empty());
    }

    // ── History cleanup: soft delete → permanent cleanup → verify ──

    #[test]
    fn history_cleanup_soft_delete_then_permanent_cleanup() {
        let database = Database::open_in_memory().unwrap();

        let now_ms = 1_000_000_000_000i64;
        let day_ms = 86_400_000i64;

        database
            .save_item(&text_item("keep", "kept content", now_ms))
            .unwrap();
        database
            .save_item(&text_item(
                "delete",
                "deleted content",
                now_ms - 30 * day_ms,
            ))
            .unwrap();

        assert!(database.soft_delete("delete").unwrap());

        let _deleted = database.permanently_delete_expired(7).unwrap();

        let remaining = database.item_count().unwrap();
        assert!(remaining >= 1);
        assert!(database.get_item("keep").unwrap().is_some());
    }

    // ── API contract: item count with soft-deleted ──

    #[test]
    fn item_count_excludes_soft_deleted_items() {
        let database = Database::open_in_memory().unwrap();

        database.save_item(&text_item("a", "active", 100)).unwrap();
        database
            .save_item(&text_item("b", "to-delete", 200))
            .unwrap();

        assert_eq!(database.item_count().unwrap(), 2);

        database.soft_delete("b").unwrap();
        assert_eq!(database.item_count().unwrap(), 1);
    }

    // ── Pagination edge cases ──

    #[test]
    fn pagination_with_empty_database() {
        let database = Database::open_in_memory().unwrap();
        let items = database
            .list_recent(
                100,
                0,
                &clipboard_desktop_lib::storage::HistoryFilter::default(),
            )
            .unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn pagination_beyond_bounds_returns_empty() {
        let database = Database::open_in_memory().unwrap();
        database.save_item(&text_item("a", "content", 100)).unwrap();

        let items = database
            .list_recent(
                100,
                1000,
                &clipboard_desktop_lib::storage::HistoryFilter::default(),
            )
            .unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn pagination_returns_partial_page_at_end() {
        let database = Database::open_in_memory().unwrap();
        for i in 0..5 {
            database
                .save_item(&text_item(&format!("item-{i}"), "content", i * 100))
                .unwrap();
        }

        let items = database
            .list_recent(
                3,
                3,
                &clipboard_desktop_lib::storage::HistoryFilter::default(),
            )
            .unwrap();
        assert_eq!(items.len(), 2);
    }

    // ── Search validation ──

    #[test]
    fn search_index_validate_returns_true_for_valid_index() {
        let index = SearchIndex::in_memory().unwrap();
        assert!(index.validate());
    }
}
