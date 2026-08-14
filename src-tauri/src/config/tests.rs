use std::{fs, path::PathBuf, str::FromStr, time::SystemTime};

use serde_json::{json, Value};

use crate::config::ConfigStore;

#[test]
fn creates_the_single_project_configuration_file() {
    let project = temporary_test_directory("default");

    let store = ConfigStore::load(&project).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

    assert_eq!(store.path(), project.join("conf/conf.json"));
    assert_eq!(saved["storage"]["dataDirectory"], Value::Null);
    assert_eq!(saved["storage"]["maxFileCopySizeBytes"], 104_857_600);
    assert_eq!(saved["storage"]["maxScreenshotSizeBytes"], 52_428_800);
    assert_eq!(saved["history"]["maxItems"], 10_000);
    assert_eq!(saved["history"]["retentionDays"], 30);
    assert_eq!(saved["history"]["favoritesExempt"], true);
    assert_eq!(saved["history"]["recycleBinDays"], 7);
    assert_eq!(saved["privacy"]["localOnly"], true);
    assert_eq!(saved["privacy"]["captureSensitiveSources"], false);
    assert_eq!(
        saved["privacy"]["sensitivePatterns"]
            .as_array()
            .map(|patterns| patterns.len()),
        Some(7)
    );
    assert_eq!(saved["privacy"]["paused"], false);
    assert_eq!(saved["permissions"]["allowNetworkAccess"], false);
    assert_eq!(saved["window"]["launchAtStartup"], false);
    assert_eq!(saved["window"]["closeToTray"], true);
    assert_eq!(saved["window"]["singleInstance"], true);
    assert_eq!(saved["general"]["language"], "zh-CN");
    assert_eq!(saved["general"]["fontSizes"]["base"], 14);
    assert_eq!(saved["general"]["fontSizes"]["secondary"], 11);
    assert_eq!(saved["general"]["display"]["showSecondaryText"], true);
    assert_eq!(saved["general"]["display"]["maxTextLines"], 3);
    assert_eq!(saved["general"]["windowTransparency"], 95);
    assert_eq!(saved["general"]["windowEffect"], "off");
    assert_eq!(saved["general"]["windowOpacityAffectsText"], false);
    assert_eq!(saved["general"]["compactMode"], false);
    assert_eq!(saved["general"]["showToastNotifications"], true);
    assert_eq!(saved["general"]["viewerBackdropOpacity"], 92);
    assert_eq!(saved["general"]["searchSuggestionMode"], "off");
    assert_eq!(saved["general"]["searchHistoryEnabled"], false);
    assert_eq!(saved["general"]["showSettingsCloseButton"], true);
    assert_eq!(saved["general"]["pageSizeLimit"], 500);
    assert_eq!(saved["general"]["searchPageSizeLimit"], 500);
    assert_eq!(saved["general"]["maxTextCaptureBytes"], 500_000);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_general_settings_as_one_configuration_group() {
    let project = temporary_test_directory("general");
    let mut store = ConfigStore::load(&project).unwrap();
    let mut settings = store.general_settings().clone();
    settings.language = "en".to_owned();
    settings.font_sizes.base = 17;
    settings.display.show_secondary_text = false;
    settings.display.max_text_lines = 7;
    settings.compact_mode = true;
    settings.show_toast_notifications = false;
    settings.theme = "light".to_owned();
    settings.image_fullscreen_mode = "desktop".to_owned();
    settings.viewer_backdrop_opacity = 64;
    settings.search_suggestion_mode = "inline".to_owned();
    settings.search_history_enabled = true;
    settings.show_settings_close_button = false;
    settings.window_effect = "acrylic".to_owned();
    settings.window_opacity_affects_text = true;
    settings.search_index_sync_mode = "background".to_owned();
    settings.max_text_capture_bytes = 2_000_000;

    store.set_general_settings(settings.clone()).unwrap();

    assert!(store.has_general_settings());
    assert_eq!(store.general_settings(), &settings);
    let reloaded = ConfigStore::load(&project).unwrap();
    assert!(reloaded.has_general_settings());
    assert_eq!(reloaded.general_settings(), &settings);

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["general"]["language"], "en");
    assert_eq!(saved["general"]["fontSizes"]["base"], 17);
    assert_eq!(saved["general"]["display"]["showSecondaryText"], false);
    assert_eq!(saved["general"]["display"]["maxTextLines"], 7);
    assert_eq!(saved["general"]["compactMode"], true);
    assert_eq!(saved["general"]["showToastNotifications"], false);
    assert_eq!(saved["general"]["windowEffect"], "acrylic");
    assert_eq!(saved["general"]["theme"], "light");
    assert_eq!(saved["general"]["imageFullscreenMode"], "desktop");
    assert_eq!(saved["general"]["viewerBackdropOpacity"], 64);
    assert_eq!(saved["general"]["searchSuggestionMode"], "inline");
    assert_eq!(saved["general"]["searchHistoryEnabled"], true);
    assert_eq!(saved["general"]["showSettingsCloseButton"], false);
    assert_eq!(saved["general"]["searchIndexSyncMode"], "background");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn existing_general_settings_default_search_preferences_to_disabled() {
    let project = temporary_test_directory("search-preferences-defaults");
    let config_directory = project.join("conf");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        serde_json::to_vec_pretty(&json!({
            "general": {
                "language": "en"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = ConfigStore::load(&project).unwrap();
    assert_eq!(store.general_settings().search_suggestion_mode, "off");
    assert!(!store.general_settings().search_history_enabled);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn existing_general_settings_default_to_visible_settings_close_button() {
    let project = temporary_test_directory("settings-close-button-default");
    let config_directory = project.join("conf");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        serde_json::to_vec_pretty(&json!({
            "general": {
                "language": "en"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let store = ConfigStore::load(&project).unwrap();
    assert!(store.general_settings().show_settings_close_button);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_ocr_runtime_settings_as_one_configuration_group() {
    let project = temporary_test_directory("ocr-settings");
    let mut store = ConfigStore::load(&project).unwrap();

    store
        .set_ocr_settings("ppocr".to_owned(), "medium".to_owned(), 0.4, 0.7, 2.0)
        .unwrap();

    let reloaded = ConfigStore::load(&project).unwrap();
    assert_eq!(reloaded.ocr_engine(), "ppocr");
    assert_eq!(reloaded.ppocr_model_variant(), "medium");
    assert_eq!(reloaded.det_score_threshold(), 0.4);
    assert_eq!(reloaded.det_box_threshold(), 0.7);
    assert_eq!(reloaded.det_unclip_ratio(), 2.0);

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["ocr"]["engine"], "ppocr");
    assert_eq!(saved["ocr"]["ppocrModelVariant"], "medium");
    assert_eq!(saved["ocr"]["detScoreThreshold"], json!(0.4_f32));
    assert_eq!(saved["ocr"]["detBoxThreshold"], json!(0.7_f32));
    assert_eq!(saved["ocr"]["detUnclipRatio"], json!(2.0_f32));

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn old_configuration_without_general_settings_stays_compatible_until_migrated() {
    let project = temporary_test_directory("legacy-general");
    let config_directory = project.join("conf");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        serde_json::to_vec_pretty(&json!({
            "storage": { "copySizeLimitMb": 128 },
            "window": { "useSystemTitlebar": false }
        }))
        .unwrap(),
    )
    .unwrap();

    let mut store = ConfigStore::load(&project).unwrap();
    assert!(!store.has_general_settings());
    assert_eq!(store.general_settings().language, "zh-CN");

    store.set_max_items(123).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert!(saved.get("general").is_none());
    assert_eq!(saved["history"]["maxItems"], 123);

    let mut settings = store.general_settings().clone();
    settings.language = "en".to_owned();
    store.set_general_settings(settings).unwrap();
    assert!(store.has_general_settings());
    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["general"]["language"], "en");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn preserves_other_settings_when_the_storage_directory_changes() {
    let project = temporary_test_directory("preserve");
    let config_directory = project.join("conf");
    let custom_data = project.join("custom-data");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        serde_json::to_vec_pretty(&json!({
            "storage": {
                "dataDirectory": null,
                "copySizeLimitMb": 128
            },
            "window": {
                "useSystemTitlebar": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let mut store = ConfigStore::load(&project).unwrap();

    store
        .set_storage_directory(Some(custom_data.clone()))
        .unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

    assert_eq!(store.storage_directory(), Some(custom_data.as_path()));
    assert_eq!(saved["storage"]["copySizeLimitMb"], 128);
    assert_eq!(saved["window"]["useSystemTitlebar"], false);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_independent_resource_storage_paths() {
    let project = temporary_test_directory("resource-paths");
    let image_path = project.join("screenshots");
    let file_path = project.join("managed-files");
    let mut store = ConfigStore::load(&project).unwrap();

    store
        .set_resource_storage_paths(Some(image_path.clone()), Some(file_path.clone()))
        .unwrap();

    assert_eq!(store.image_storage_path(), Some(image_path.as_path()));
    assert_eq!(store.file_storage_path(), Some(file_path.as_path()));
    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(
        saved["storage"]["imageStoragePath"],
        image_path.to_string_lossy().to_string()
    );
    assert_eq!(
        saved["storage"]["fileStoragePath"],
        file_path.to_string_lossy().to_string()
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_relative_or_shared_resource_storage_paths() {
    let project = temporary_test_directory("invalid-resource-paths");
    let mut store = ConfigStore::load(&project).unwrap();
    let shared = project.join("resources");

    assert!(store
        .set_resource_storage_paths(Some(PathBuf::from("relative")), None)
        .is_err());
    assert!(store
        .set_resource_storage_paths(Some(shared.clone()), Some(shared))
        .is_err());

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn normalizes_and_persists_ignored_applications() {
    let project = temporary_test_directory("ignored-apps");
    let mut store = ConfigStore::load(&project).unwrap();

    let saved = store
        .set_ignored_applications(vec![
            " KeePass ".to_owned(),
            "keepass".to_owned(),
            "Bitwarden".to_owned(),
            "".to_owned(),
        ])
        .unwrap();
    let persisted: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();

    assert_eq!(saved, vec!["Bitwarden", "KeePass"]);
    assert_eq!(store.ignored_applications(), saved);
    assert_eq!(
        persisted["privacy"]["ignoredApplications"],
        json!(["Bitwarden", "KeePass"])
    );
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_window_position() {
    let project = temporary_test_directory("window-pos");
    let mut store = ConfigStore::load(&project).unwrap();

    store.set_window_position(100, 200, 800, 600).unwrap();

    assert_eq!(store.window_position(), Some((100, 200, 800, 600)));

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["window"]["x"], 100);
    assert_eq!(saved["window"]["y"], 200);
    assert_eq!(saved["window"]["width"], 800);
    assert_eq!(saved["window"]["height"], 600);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_privacy_settings() {
    let project = temporary_test_directory("privacy");
    let mut store = ConfigStore::load(&project).unwrap();

    assert!(!store.privacy_paused());

    store.set_privacy_paused(true).unwrap();
    assert!(store.privacy_paused());

    store
        .set_privacy_master_password_hash(Some("hash123".to_owned()))
        .unwrap();
    assert_eq!(store.privacy_master_password_hash(), Some("hash123"));

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["privacy"]["paused"], true);
    assert_eq!(saved["privacy"]["masterPasswordHash"], "hash123");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_privacy_flags_and_sensitive_patterns() {
    let project = temporary_test_directory("privacy-patterns");
    let mut store = ConfigStore::load(&project).unwrap();

    assert!(store.privacy_local_only());
    assert!(!store.privacy_capture_sensitive_sources());
    assert_eq!(store.sensitive_patterns().len(), 7);

    store.set_privacy_local_only(false).unwrap();
    store.set_privacy_capture_sensitive_sources(true).unwrap();
    let stored = store
        .set_sensitive_patterns(vec![
            "  \\bpassword\\s*[=:]\\s*\\S+  ".to_owned(),
            "".to_owned(),
            "   ".to_owned(),
            "\\bpassword\\s*[=:]\\s*\\S+".to_owned(),
            "internal-id-\\d+".to_owned(),
        ])
        .unwrap();

    // Trimming, blank dropping and deduplication all apply; order is kept.
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0], r"\bpassword\s*[=:]\s*\S+");
    assert_eq!(stored[1], r"internal-id-\d+");

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["privacy"]["localOnly"], false);
    assert_eq!(saved["privacy"]["captureSensitiveSources"], true);
    assert_eq!(
        saved["privacy"]["sensitivePatterns"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn legacy_config_with_removed_telemetry_flag_still_loads() {
    let project = temporary_test_directory("privacy-legacy");
    let config_directory = project.join("conf");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        r#"{"privacy":{"telemetryEnabled":true,"localOnly":false}}"#,
    )
    .unwrap();

    let store = ConfigStore::load(&project).unwrap();
    assert!(!store.privacy_local_only());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn persists_export_settings() {
    let project = temporary_test_directory("export");
    let mut store = ConfigStore::load(&project).unwrap();

    assert!(store.schedule_auto_export().is_none());

    store
        .set_schedule_auto_export(Some("0 */6 * * *".to_owned()))
        .unwrap();
    assert_eq!(store.schedule_auto_export(), Some("0 */6 * * *"));

    store.set_schedule_auto_export(None).unwrap();
    assert!(store.schedule_auto_export().is_none());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn sync_settings_use_documented_defaults() {
    let project = temporary_test_directory("sync-defaults");
    let store = ConfigStore::load(&project).unwrap();
    let sync = store.sync_config();

    assert!(!sync.auto_sync);
    assert_eq!(sync.auto_sync_interval_secs, 300);
    assert_eq!(sync.remote_path.as_deref(), Some("clipboard-sync"));
    assert_eq!(sync.s3_region.as_deref(), Some("us-east-1"));
    assert_eq!(sync.segment_max_entries, 512);
    assert_eq!(sync.max_sync_image_bytes, 5_242_880);
    assert_eq!(sync.max_sync_file_bytes, 10_485_760);

    let saved: Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(saved["sync"]["autoSyncIntervalSecs"], 300);
    assert_eq!(saved["sync"]["segmentMaxEntries"], 512);
    assert_eq!(saved["sync"]["maxSyncImageBytes"], 5_242_880);
    assert_eq!(saved["sync"]["maxSyncFileBytes"], 10_485_760);
    assert!(saved["sync"].get("maxRemoteOplogFiles").is_none());
    assert!(saved["sync"].get("oplogRolloverEntries").is_none());
    assert!(saved["sync"].get("oplogRolloverSizeBytes").is_none());

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn partial_sync_settings_use_current_defaults() {
    let project = temporary_test_directory("partial-sync-defaults");
    let config_directory = project.join("conf");
    fs::create_dir_all(&config_directory).unwrap();
    fs::write(
        config_directory.join("conf.json"),
        serde_json::to_vec_pretty(&json!({
            "sync": {
                "provider": "off"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let sync = ConfigStore::load(&project).unwrap().sync_config();
    assert_eq!(sync.auto_sync_interval_secs, 300);
    assert_eq!(sync.remote_path.as_deref(), Some("clipboard-sync"));
    assert_eq!(sync.s3_region.as_deref(), Some("us-east-1"));
    assert_eq!(sync.segment_max_entries, 512);
    assert_eq!(sync.max_sync_image_bytes, 5_242_880);
    assert_eq!(sync.max_sync_file_bytes, 10_485_760);

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn search_index_sync_mode_defaults_to_lazy_and_round_trips() {
    let project = temporary_test_directory("search-sync-mode");
    let mut store = ConfigStore::load(&project).unwrap();

    // Default is lazy (current behavior) for existing/legacy configurations.
    assert_eq!(
        store.search_index_sync_mode(),
        crate::config::SearchIndexSyncMode::Lazy
    );

    let mut settings = store.general_settings().clone();
    settings.search_index_sync_mode = "background".to_owned();
    store.set_general_settings(settings.clone()).unwrap();
    assert_eq!(
        store.search_index_sync_mode(),
        crate::config::SearchIndexSyncMode::Background
    );

    let reloaded = ConfigStore::load(&project).unwrap();
    assert_eq!(
        reloaded.search_index_sync_mode(),
        crate::config::SearchIndexSyncMode::Background
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn search_index_sync_mode_parses_unknown_values_as_lazy() {
    use crate::config::SearchIndexSyncMode;
    assert_eq!(
        SearchIndexSyncMode::from_str("lazy"),
        Ok(SearchIndexSyncMode::Lazy)
    );
    assert_eq!(
        SearchIndexSyncMode::from_str("background"),
        Ok(SearchIndexSyncMode::Background)
    );
    assert_eq!(
        SearchIndexSyncMode::from_str("anything-else"),
        Ok(SearchIndexSyncMode::Lazy)
    );
    assert_eq!(SearchIndexSyncMode::Lazy.as_str(), "lazy");
    assert_eq!(SearchIndexSyncMode::Background.as_str(), "background");
}

#[test]
fn update_source_defaults_to_gitcode_and_round_trips() {
    let project = temporary_test_directory("update-source");
    let mut store = ConfigStore::load(&project).unwrap();

    // Default is gitcode (current behavior) for fresh/legacy configurations.
    assert_eq!(store.update_source(), crate::config::UpdateSource::Gitcode);

    let mut settings = store.general_settings().clone();
    settings.update_source = "github".to_owned();
    store.set_general_settings(settings.clone()).unwrap();
    assert_eq!(store.update_source(), crate::config::UpdateSource::Github);

    let reloaded = ConfigStore::load(&project).unwrap();
    assert_eq!(
        reloaded.update_source(),
        crate::config::UpdateSource::Github
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn update_source_parses_unknown_values_as_gitcode() {
    use crate::config::UpdateSource;
    assert_eq!(UpdateSource::from_str("github"), Ok(UpdateSource::Github));
    assert_eq!(UpdateSource::from_str("gitcode"), Ok(UpdateSource::Gitcode));
    assert_eq!(
        UpdateSource::from_str("anything-else"),
        Ok(UpdateSource::Gitcode)
    );
    assert_eq!(UpdateSource::Github.as_str(), "github");
    assert_eq!(UpdateSource::Gitcode.as_str(), "gitcode");
}

fn temporary_test_directory(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    std::env::temp_dir().join(format!(
        "clipboard-config-{label}-{}-{unique}",
        std::process::id()
    ))
}
