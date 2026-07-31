use super::*;
use crate::content::resource_metadata::RESOURCE_METADATA_SCHEMA_VERSION;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::{ClipboardRepository, OcrRepository};

#[cfg(test)]
mod search_pagination_tests {
    use super::*;

    fn item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: id.to_owned(),
            text_content: Some(id.to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            icon_path: None,
            size_bytes: 1,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn search_items_are_sorted_before_pagination() {
        let candidates = vec![
            item("oldest", 100),
            item("newest", 300),
            item("middle", 200),
        ];
        let rules = [SearchSortRule {
            field: SearchSortField::CreatedAt,
            direction: SearchSortDirection::Desc,
        }];

        let mut first = candidates.clone();
        apply_sort_rules(&mut first, &rules);
        let first_page: Vec<_> = first.into_iter().take(2).collect();

        let mut second = candidates.clone();
        apply_sort_rules(&mut second, &rules);
        let second_page: Vec<_> = second.into_iter().skip(2).take(2).collect();

        assert_eq!(
            first_page
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["newest", "middle"]
        );
        assert_eq!(second_page[0].id, "oldest");
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use std::path::Path;

    fn capture_state() -> CaptureState {
        CaptureState::new(
            &PrivacyManager::new(),
            vec!["IgnoredApp".to_owned()],
            100 * 1024 * 1024,
        )
    }

    #[test]
    fn pause_state_is_shared_with_capture_policy() {
        let state = capture_state();
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));

        state.set_paused(true);
        assert!(state.should_skip(Some("Notepad"), Some("ordinary text")));

        state.set_paused(false);
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));
    }

    #[test]
    fn file_copy_limit_updates_are_immediately_visible_to_capture() {
        let state = capture_state();
        assert_eq!(state.max_file_copy_size_bytes(), 100 * 1024 * 1024);

        state.set_max_file_copy_size_bytes(8 * 1024 * 1024);

        assert_eq!(state.max_file_copy_size_bytes(), 8 * 1024 * 1024);
    }

    #[test]
    fn ignored_and_password_manager_sources_are_rejected_case_insensitively() {
        let state = capture_state();

        assert!(state.should_skip(Some("ignoredapp.exe"), None));
        assert!(state.should_skip(Some(r"C:\Program Files\KeePass\KeePass.exe"), None));
        assert!(state.should_skip(Some("1PASSWORD"), None));
        assert!(!state.should_skip(Some("notepad.exe"), None));
    }

    #[test]
    fn ignored_application_updates_are_deduplicated_and_immediately_visible() {
        let state = capture_state();
        let stored = state.set_ignored_apps(vec![
            " Browser.exe ".to_owned(),
            "browser".to_owned(),
            "Terminal".to_owned(),
        ]);

        assert_eq!(stored, vec!["Browser.exe", "Terminal"]);
        assert_eq!(state.ignored_apps(), stored);
        assert!(state.should_skip(Some("browser.exe"), None));
        assert!(state.should_skip(Some("TERMINAL"), None));
    }

    #[test]
    fn sensitive_text_is_rejected_before_persistence() {
        let state = capture_state();

        assert!(state.should_skip(Some("Notepad"), Some("password=supersecret123")));
        assert!(state.should_skip(Some("Notepad"), Some("4111 1111 1111 1111")));
        assert!(!state.should_skip(Some("Notepad"), Some("meeting notes")));
    }

    #[test]
    fn foreground_source_uses_name_then_executable_fallback() {
        let named = platform::ForegroundApp {
            name: "Editor".to_owned(),
            exe_path: r"C:\Apps\editor.exe".to_owned(),
        };
        let path_only = platform::ForegroundApp {
            name: String::new(),
            exe_path: r"C:\Apps\Browser.exe".to_owned(),
        };

        assert_eq!(foreground_app_name(&named).as_deref(), Some("editor"));
        assert_eq!(foreground_app_name(&path_only).as_deref(), Some("Browser"));
    }

    #[test]
    fn self_triggered_link_write_is_skipped_before_source_metadata_can_change() {
        let text = "https://example.com";
        let database = Database::open_in_memory().unwrap();
        let original = ClipboardItem {
            id: "original".to_owned(),
            kind: ClipboardKind::Link,
            title: text.to_owned(),
            text_content: Some(text.to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: content::hash::compute_content_hash("link", text, None),
            source_app: Some("Browser".to_owned()),
            icon_path: Some("browser.png".to_owned()),
            size_bytes: text.len() as u64,
            created_at_ms: 100,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        };
        database.save_item(&original).unwrap();

        let mut guard = content::self_trigger::SelfTriggerGuard::new();
        guard.mark_clipboard_write(text);
        assert!(should_skip_self_triggered_text(
            &mut guard,
            ClipboardKind::Link,
            text,
        ));

        let stored = database.get_item("original").unwrap().unwrap();
        assert_eq!(stored.source_app, original.source_app);
        assert_eq!(stored.icon_path, original.icon_path);
        assert_eq!(stored.created_at_ms, original.created_at_ms);
    }

    #[test]
    fn self_triggered_file_writes_match_single_and_group_hashes() {
        let single_path = r"C:\Users\admin\Documents\report.txt";
        let mut single_guard = content::self_trigger::SelfTriggerGuard::new();
        single_guard.mark_clipboard_write(single_path);
        let single_hash = content::hash::compute_content_hash("file", single_path, None);
        assert!(should_skip_self_triggered_hash(
            &mut single_guard,
            &single_hash
        ));

        let paths = [
            r"C:\Users\admin\Documents\zeta.txt",
            r"C:\Users\admin\Documents\alpha.txt",
        ];
        let mut group_guard = content::self_trigger::SelfTriggerGuard::new();
        group_guard.mark_clipboard_write(&paths.join("\n"));
        let mut sorted_paths = paths.to_vec();
        sorted_paths.sort();
        let group_hash =
            content::hash::compute_content_hash("files", &sorted_paths.join("\n"), None);
        assert!(should_skip_self_triggered_hash(
            &mut group_guard,
            &group_hash
        ));
    }

    #[test]
    fn captured_files_are_copied_into_managed_storage() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-captured-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source_dir = root.join("source");
        let storage_dir = root.join("storage/files");
        std::fs::create_dir_all(&source_dir).unwrap();
        let first = source_dir.join("first.txt");
        let second = source_dir.join("second.json");
        std::fs::write(&first, b"first file").unwrap();
        std::fs::write(&second, b"{\"second\":true}").unwrap();

        let stored = store_captured_file_references(
            &[
                first.to_string_lossy().to_string(),
                second.to_string_lossy().to_string(),
            ],
            &storage_dir,
            1024,
        );

        assert_eq!(stored.len(), 2);
        assert!(stored.iter().all(|file| file.copied));
        assert!(stored
            .iter()
            .all(|file| Path::new(&file.storage_path).starts_with(&storage_dir)));
        assert_eq!(
            std::fs::read(&stored[0].storage_path).unwrap(),
            b"first file"
        );
        assert_eq!(
            Path::new(&stored[1].storage_path).extension(),
            Some(std::ffi::OsStr::new("json"))
        );

        let metadata: serde_json::Value =
            serde_json::from_str(&captured_file_metadata(&stored)).unwrap();
        assert_eq!(metadata["schemaVersion"], RESOURCE_METADATA_SCHEMA_VERSION);
        assert_eq!(metadata["files"][0]["name"], "first.txt");
        assert_eq!(metadata["files"][0]["mimeType"], "text/plain");
        assert_eq!(metadata["files"][0]["extension"], "txt");
        assert_eq!(metadata["files"][0]["sizeBytes"], 10);
        assert!(metadata["files"][0]["contentHash"].is_string());
        assert_eq!(metadata["files"][1]["copied"], true);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_captured_file_keeps_the_original_link() {
        let root = std::env::temp_dir().join(format!(
            "clipboard-captured-file-limit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source/large.bin");
        let storage_dir = root.join("storage/files");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, vec![7_u8; 32]).unwrap();

        let stored = store_captured_file_references(
            &[source.to_string_lossy().to_string()],
            &storage_dir,
            8,
        );

        assert_eq!(stored.len(), 1);
        assert!(!stored[0].copied);
        assert_eq!(stored[0].storage_path, source.to_string_lossy());
        assert_eq!(stored[0].size_bytes, 32);
        assert_eq!(stored[0].mime_type, "application/octet-stream");
        assert_eq!(stored[0].extension.as_deref(), Some("bin"));
        assert_eq!(std::fs::read_dir(&storage_dir).unwrap().count(), 0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn self_triggered_image_write_matches_normalized_capture_hash() {
        use std::io::Cursor;

        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([20, 40, 80, 255]));
        let mut stored_png = Cursor::new(Vec::new());
        image
            .write_to(&mut stored_png, image::ImageFormat::Png)
            .expect("PNG encoding should succeed");
        let mut clipboard_bmp = Cursor::new(Vec::new());
        image
            .write_to(&mut clipboard_bmp, image::ImageFormat::Bmp)
            .expect("BMP encoding should succeed");

        let path = std::env::temp_dir().join(format!(
            "clipboard-self-trigger-image-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        std::fs::write(&path, stored_png.get_ref()).expect("test image should be writable");

        let mut guard = content::self_trigger::SelfTriggerGuard::new();
        register_image_self_trigger(&mut guard, path.to_str(), None).unwrap();
        assert!(should_skip_self_triggered_media(
            &mut guard,
            "image",
            clipboard_bmp.get_ref()
        ));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn image_self_trigger_registration_falls_back_to_persisted_hash() {
        let bytes = b"legacy-image-bytes";
        let fallback_hash = content::hash::compute_media_hash("image", bytes);
        let mut guard = content::self_trigger::SelfTriggerGuard::new();

        register_image_self_trigger(
            &mut guard,
            Some("C:\\missing\\clipboard-image.png"),
            Some(&fallback_hash),
        )
        .unwrap();

        assert!(should_skip_self_triggered_hash(&mut guard, &fallback_hash));
    }

    #[test]
    fn invalid_sensitive_patterns_are_excluded_during_initialization() {
        let mut privacy = PrivacyManager::new();
        privacy.sensitive_patterns = vec![regex_lite::Regex::new("secret").unwrap()];
        let state = CaptureState::new(&privacy, Vec::new(), 100 * 1024 * 1024);

        assert_eq!(state.policy.sensitive_patterns.len(), 1);
        assert!(state.should_skip(Some("Notepad"), Some("a secret value")));
        assert!(!state.should_skip(Some("Notepad"), Some("ordinary text")));
    }

    #[test]
    fn capture_worker_stop_wakes_and_joins_thread() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let exited_for_thread = Arc::clone(&exited);
        let (stop_sender, stop_receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = stop_receiver.recv();
            exited_for_thread.store(true, Ordering::SeqCst);
        });
        let mut worker = CaptureWorker {
            stop_flag: Arc::clone(&stop_flag),
            stop_sender: Some(stop_sender),
            handle: Some(handle),
        };

        worker.stop();

        assert!(stop_flag.load(Ordering::SeqCst));
        assert!(exited.load(Ordering::SeqCst));
        assert!(worker.handle.is_none());
    }
}

#[cfg(test)]
mod title_metadata_tests {
    use super::*;

    #[test]
    fn legacy_records_infer_custom_titles_from_the_generated_title() {
        assert!(!resolve_custom_title(
            "first line\nsecond line",
            "first line\nsecond line",
            None,
        ));
        assert!(resolve_custom_title(
            "Pinned note",
            "first line\nsecond line",
            None,
        ));
    }

    #[test]
    fn explicit_custom_title_metadata_overrides_legacy_inference() {
        assert!(resolve_custom_title(
            "first line",
            "first line\nsecond line",
            Some(r#"{"customTitle":true}"#),
        ));
        assert!(!resolve_custom_title(
            "Pinned note",
            "first line\nsecond line",
            Some(r#"{"customTitle":false}"#),
        ));
    }

    #[test]
    fn setting_custom_title_metadata_preserves_existing_object_fields() {
        let metadata = set_custom_title_metadata(Some(r#"{"width":120}"#), true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(value["width"], 120);
        assert_eq!(value["customTitle"], true);
    }
}

#[cfg(test)]
mod window_position_tests {
    use super::*;

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> WindowPosition {
        WindowPosition {
            x,
            y,
            width,
            height,
        }
    }

    fn work_area(x: i32, y: i32, width: u32, height: u32) -> WindowWorkArea {
        WindowWorkArea {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn keeps_bounds_inside_the_current_work_area() {
        let saved = bounds(240, 120, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(clamp_window_position_to_work_areas(saved, &areas), saved);
    }

    #[test]
    fn clamps_an_offscreen_window_to_the_visible_edge() {
        let saved = bounds(2500, 100, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(1020, 100, 900, 700)
        );
    }

    #[test]
    fn moves_a_window_from_a_removed_monitor_to_the_nearest_remaining_area() {
        let saved = bounds(-1600, 80, 900, 700);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(0, 80, 900, 700)
        );
    }

    #[test]
    fn preserves_negative_coordinates_for_a_connected_left_monitor() {
        let saved = bounds(-1800, 120, 800, 700);
        let areas = [work_area(-1920, 0, 1920, 1040), work_area(0, 0, 1920, 1040)];

        assert_eq!(clamp_window_position_to_work_areas(saved, &areas), saved);
    }

    #[test]
    fn chooses_the_monitor_with_the_largest_visible_overlap() {
        let saved = bounds(1700, 100, 800, 700);
        let areas = [work_area(0, 0, 1920, 1040), work_area(1920, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(1920, 100, 800, 700)
        );
    }

    #[test]
    fn limits_oversized_bounds_to_the_selected_work_area() {
        let saved = bounds(-400, -300, 4000, 2400);
        let areas = [work_area(0, 0, 1920, 1040)];

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &areas),
            bounds(0, 0, 1920, 1040)
        );
    }

    #[test]
    fn applies_native_minimums_without_monitor_information() {
        let saved = bounds(20, 30, 0, 0);

        assert_eq!(
            clamp_window_position_to_work_areas(saved, &[]),
            bounds(20, 30, MAIN_WINDOW_MIN_WIDTH, MAIN_WINDOW_MIN_HEIGHT)
        );
    }
}

#[cfg(test)]
mod storage_cleanup_tests {
    use std::{fs, time::SystemTime};

    use super::*;
    use crate::domain::OcrResult;

    fn temporary_project(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "clipboard-storage-cleanup-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn temporary_storage(label: &str) -> StoragePaths {
        StoragePaths::initialize(temporary_project(label)).unwrap()
    }

    fn stored_item(
        id: &str,
        kind: ClipboardKind,
        resource_path: Option<String>,
        icon_path: Option<String>,
    ) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind,
            title: format!("record-{id}"),
            text_content: (kind == ClipboardKind::Text).then(|| format!("content-{id}")),
            html_content: None,
            resource_path,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("test-suite".to_owned()),
            icon_path,
            size_bytes: 12,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        }
    }

    #[test]
    fn cleanup_preserves_soft_deleted_resources_until_permanent_deletion() {
        let paths = temporary_storage("recycle-bin");
        let resource = paths.images.join("recoverable.png");
        let preview = paths.previews.join("recoverable-preview.png");
        fs::write(&resource, b"image-data").unwrap();
        fs::write(&preview, b"preview-data").unwrap();
        let database = Database::open_in_memory().unwrap();
        let mut item = stored_item(
            "recoverable",
            ClipboardKind::Image,
            Some(resource.to_string_lossy().into_owned()),
            None,
        );
        item.preview_path = Some(preview.to_string_lossy().into_owned());
        database.save_item(&item).unwrap();
        database.soft_delete("recoverable").unwrap();

        let first_cleanup = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(first_cleanup.removed_files, 0);
        assert!(resource.exists());
        assert!(preview.exists());
        assert!(database.restore_deleted("recoverable").unwrap());

        database.soft_delete("recoverable").unwrap();
        assert!(database.permanently_delete("recoverable").unwrap());
        let second_cleanup = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(second_cleanup.removed_files, 2);
        assert!(!resource.exists());
        assert!(!preview.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn category_delete_cleans_ocr_search_index_and_managed_resources() {
        let paths = temporary_storage("category-delete");
        let resource = paths.images.join("category.png");
        let preview = paths.previews.join("category-preview.png");
        let favorite_resource = paths.images.join("favorite.png");
        let favorite_preview = paths.previews.join("favorite-preview.png");
        let icons = paths.storage.join("icons");
        let shared_icon = icons.join("shared.png");
        fs::create_dir_all(&icons).unwrap();
        fs::write(&resource, b"image-data").unwrap();
        fs::write(&preview, b"preview-data").unwrap();
        fs::write(&favorite_resource, b"favorite-image-data").unwrap();
        fs::write(&favorite_preview, b"favorite-preview-data").unwrap();
        fs::write(&shared_icon, b"shared-icon-data").unwrap();

        let database = Database::open_in_memory().unwrap();
        let mut image = stored_item(
            "category-image",
            ClipboardKind::Image,
            Some(resource.to_string_lossy().into_owned()),
            Some("shared.png".to_owned()),
        );
        image.preview_path = Some(preview.to_string_lossy().into_owned());
        database.save_item(&image).unwrap();
        let mut favorite_image = stored_item(
            "favorite-image",
            ClipboardKind::Image,
            Some(favorite_resource.to_string_lossy().into_owned()),
            Some("shared.png".to_owned()),
        );
        favorite_image.preview_path = Some(favorite_preview.to_string_lossy().into_owned());
        favorite_image.is_favorite = true;
        database.save_item(&favorite_image).unwrap();
        database
            .save_ocr_result(&OcrResult {
                item_id: image.id.clone(),
                status: domain::OcrStatus::Completed,
                engine: "test".to_owned(),
                model_version: "1".to_owned(),
                language: Some("en".to_owned()),
                full_text: "category recognized text".to_owned(),
                blocks: Vec::new(),
                image_hash: image.content_hash.clone(),
                created_at_ms: 1,
                completed_at_ms: Some(2),
                error_message: None,
            })
            .unwrap();
        database
            .save_item(&stored_item(
                "preserved-text",
                ClipboardKind::Text,
                None,
                None,
            ))
            .unwrap();

        let search_index = SearchIndex::in_memory().unwrap();
        SearchSynchronizer::default()
            .sync_until_idle(&database, &search_index)
            .unwrap();
        assert_eq!(search_index.search("recognized", 20).unwrap().len(), 1);

        let result = permanently_delete_storage_kind_for(
            &database,
            &paths,
            &search_index,
            ClipboardKind::Image,
            None,
        )
        .unwrap();

        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.deleted_size_bytes, image.size_bytes);
        assert_eq!(result.deleted_ids, vec![image.id.clone()]);
        assert_eq!(result.removed_files, 2);
        assert_eq!(result.search_sync.as_ref().unwrap().deleted_documents, 1);
        assert!(result.warnings.is_empty());
        assert!(database.get_item(&image.id).unwrap().is_none());
        assert!(database.get_ocr_result(&image.id).unwrap().is_none());
        assert!(database.get_item(&favorite_image.id).unwrap().is_some());
        assert!(database.get_item("preserved-text").unwrap().is_some());
        assert!(search_index.search("recognized", 20).unwrap().is_empty());
        assert!(!resource.exists());
        assert!(!preview.exists());
        assert!(favorite_resource.exists());
        assert!(favorite_preview.exists());
        assert!(shared_icon.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn category_delete_rejects_stale_confirmation_statistics() {
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&stored_item("first-text", ClipboardKind::Text, None, None))
            .unwrap();
        let confirmed = database
            .kind_storage_stats(ClipboardKind::Text, STORAGE_KIND_DELETE_SCOPE)
            .unwrap();
        database
            .save_item(&stored_item("second-text", ClipboardKind::Text, None, None))
            .unwrap();

        let error = database
            .permanently_delete_by_kind_if_stats_match(
                ClipboardKind::Text,
                STORAGE_KIND_DELETE_SCOPE,
                confirmed,
            )
            .unwrap_err();

        assert!(error.to_string().contains("data changed"));
        assert_eq!(database.item_count().unwrap(), 2);
    }

    #[test]
    fn cleanup_resolves_icon_keys_and_removes_only_unreferenced_icons() {
        let paths = temporary_storage("icon-key");
        let icons = paths.storage.join("icons");
        fs::create_dir_all(&icons).unwrap();
        let referenced_icon = icons.join("notepad.png");
        let orphan_icon = icons.join("orphan.png");
        fs::write(&referenced_icon, b"referenced").unwrap();
        fs::write(&orphan_icon, b"orphan").unwrap();
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&stored_item(
                "text",
                ClipboardKind::Text,
                None,
                Some("notepad.png".to_owned()),
            ))
            .unwrap();

        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 1);
        assert!(referenced_icon.exists());
        assert!(!orphan_icon.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn scheduled_cleanup_worker_stops_and_joins_cleanly() {
        let paths = temporary_storage("worker-lifecycle");
        let database = Database::open(&paths.database).unwrap();
        let worker = CleanupWorker::start_with_interval(
            paths.project.clone(),
            database,
            paths.clone(),
            Duration::from_millis(5),
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(20));
        worker.stop();

        assert!(worker.stop_flag.load(Ordering::SeqCst));
        assert!(worker
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn scheduled_cleanup_grace_preserves_recent_orphan_files() {
        let paths = temporary_storage("orphan-grace");
        let orphan = paths.images.join("recent.png");
        fs::write(&orphan, b"recent-data").unwrap();
        let database = Database::open_in_memory().unwrap();

        let scheduled = cleanup_orphan_storage_files_with_grace(
            &database,
            &paths,
            Duration::from_secs(60 * 60),
        )
        .unwrap();
        assert_eq!(scheduled.removed_files, 0);
        assert!(orphan.exists());

        let manual = cleanup_orphan_storage_files(&database, &paths).unwrap();
        assert_eq!(manual.removed_files, 1);
        assert!(!orphan.exists());
        fs::remove_dir_all(&paths.project).unwrap();
    }

    #[test]
    fn cleanup_preserves_unowned_custom_resource_files() {
        let root = temporary_project("unowned-custom-resource");
        let project = root.join("project");
        let images = root.join("user-images");
        let files = root.join("user-files");
        fs::create_dir_all(&images).unwrap();
        fs::create_dir_all(&files).unwrap();
        let unrelated_image = images.join("family-photo.png");
        let unrelated_file = files.join("report.docx");
        fs::write(&unrelated_image, b"user image").unwrap();
        fs::write(&unrelated_file, b"user document").unwrap();

        let paths = StoragePaths::initialize_with_resource_directories(
            project,
            None,
            Some(images),
            Some(files),
        )
        .unwrap();
        assert!(!paths.image_cleanup_enabled);
        assert!(!paths.file_cleanup_enabled);

        let database = Database::open_in_memory().unwrap();
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 0);
        assert!(unrelated_image.exists());
        assert!(unrelated_file.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_removes_orphans_only_from_marked_custom_resource_roots() {
        let root = temporary_project("marked-custom-resource");
        let project = root.join("project");
        let images = root.join("managed-images");
        let files = root.join("managed-files");
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            project,
            None,
            Some(images.clone()),
            Some(files.clone()),
        )
        .unwrap();
        assert!(paths.image_cleanup_enabled);
        assert!(paths.file_cleanup_enabled);

        let orphan_image = images.join("orphan.png");
        let orphan_file = files.join("orphan.txt");
        fs::write(&orphan_image, b"orphan image").unwrap();
        fs::write(&orphan_file, b"orphan file").unwrap();

        let database = Database::open_in_memory().unwrap();
        let result = cleanup_orphan_storage_files(&database, &paths).unwrap();

        assert_eq!(result.removed_files, 2);
        assert!(!orphan_image.exists());
        assert!(!orphan_file.exists());
        assert!(images.join(storage::RESOURCE_ROOT_MARKER).exists());
        assert!(files.join(storage::RESOURCE_ROOT_MARKER).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storage_migration_rewrites_every_managed_resource_reference() {
        let project = temporary_project("path-migration");
        let old_paths = StoragePaths::initialize(project.clone()).unwrap();
        let new_paths = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(project.join("custom-data")),
        )
        .unwrap();
        let old_icons = old_paths.storage.join("icons");
        fs::create_dir_all(&old_icons).unwrap();

        let image = old_paths.images.join("image.png");
        let preview = old_paths.previews.join("image-preview.png");
        let managed_file = old_paths.files.join("document.txt");
        let icon = old_icons.join("notepad.png");
        let external_file = project.join("outside.txt");
        let search_marker = old_paths.search_index.join("migration-marker");
        for (path, contents) in [
            (&image, b"image".as_slice()),
            (&preview, b"preview".as_slice()),
            (&managed_file, b"managed".as_slice()),
            (&icon, b"icon".as_slice()),
            (&external_file, b"external".as_slice()),
            (&search_marker, b"search-index".as_slice()),
        ] {
            fs::write(path, contents).unwrap();
        }

        let database = Database::open(&old_paths.database).unwrap();
        let mut image_item = stored_item(
            "image",
            ClipboardKind::Image,
            Some(image.to_string_lossy().into_owned()),
            Some(icon.to_string_lossy().into_owned()),
        );
        image_item.preview_path = Some(preview.to_string_lossy().into_owned());
        image_item.metadata_json = Some(r#"{"width":100,"height":80}"#.to_owned());
        database.save_item(&image_item).unwrap();

        let mut file_item = stored_item(
            "file",
            ClipboardKind::File,
            Some(managed_file.to_string_lossy().into_owned()),
            None,
        );
        file_item.text_content = Some(
            serde_json::to_string(&[
                managed_file.to_string_lossy().into_owned(),
                external_file.to_string_lossy().into_owned(),
            ])
            .unwrap(),
        );
        file_item.metadata_json = Some(
            serde_json::json!({
                "files": [{
                    "path": managed_file.to_string_lossy(),
                    "originalPath": external_file.to_string_lossy(),
                    "copied": true,
                }],
            })
            .to_string(),
        );
        database.save_item(&file_item).unwrap();

        migrate_storage_data(&old_paths, &new_paths, &database).unwrap();

        let migrated = Database::open(&new_paths.database).unwrap();
        let migrated_image = migrated.get_item("image").unwrap().unwrap();
        assert_eq!(
            migrated_image.resource_path.as_deref(),
            Some(
                new_paths
                    .images
                    .join("image.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            migrated_image.preview_path.as_deref(),
            Some(
                new_paths
                    .previews
                    .join("image-preview.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            migrated_image.icon_path.as_deref(),
            Some(
                new_paths
                    .storage
                    .join("icons")
                    .join("notepad.png")
                    .to_string_lossy()
                    .as_ref()
            )
        );

        let migrated_file = migrated.get_item("file").unwrap().unwrap();
        let migrated_paths: Vec<String> =
            serde_json::from_str(migrated_file.text_content.as_deref().unwrap()).unwrap();
        assert_eq!(
            migrated_paths[0],
            new_paths.files.join("document.txt").to_string_lossy()
        );
        assert_eq!(migrated_paths[1], external_file.to_string_lossy());
        let migrated_metadata: serde_json::Value =
            serde_json::from_str(migrated_file.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            migrated_metadata["files"][0]["path"],
            new_paths
                .files
                .join("document.txt")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(
            migrated_metadata["files"][0]["originalPath"],
            external_file.to_string_lossy().as_ref()
        );
        assert!(new_paths.images.join("image.png").exists());
        assert!(new_paths.files.join("document.txt").exists());
        assert!(new_paths.storage.join("icons").join("notepad.png").exists());
        assert!(
            new_paths.search_index.exists(),
            "search index directory must exist at the target"
        );
        assert!(
            !new_paths.search_index.join("migration-marker").exists(),
            "search index is not copied — it will be rebuilt on restart"
        );

        let original = database.get_item("image").unwrap().unwrap();
        assert_eq!(original.resource_path, image_item.resource_path);
        drop(migrated);
        drop(database);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn storage_migration_skips_paths_that_already_use_the_target_layout() {
        let project = temporary_project("same-layout-migration");
        let old_paths = StoragePaths::initialize(project.clone()).unwrap();
        let database = Database::open(&old_paths.database).unwrap();
        let new_paths = StoragePaths::initialize_with_data_directory(
            project.clone(),
            Some(old_paths.storage.clone()),
        )
        .unwrap();

        assert_ne!(old_paths.data_directory, new_paths.data_directory);
        assert_eq!(old_paths.storage, new_paths.storage);
        migrate_storage_data(&old_paths, &new_paths, &database).unwrap();

        drop(database);
        fs::remove_dir_all(project).unwrap();
    }
}
