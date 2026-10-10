//! Unit tests for the icon cache, replacement validation, save-as, and shell
//! commands.

use super::clipboard::managed_source_path;
use super::icons::{app_icon_file_name, build_icon_cache};
use super::replace::{validate_replace_source, REPLACE_MAX_SOURCE_BYTES};
use super::shell::{open_external_url, openable_scheme};

use crate::commands::clipboard::ClipboardFilesCopyError;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::item_operations::{file_metadata_entries, resolve_clipboard_file_paths};
use crate::storage::{ClipboardRepository, Database, StoragePaths};

#[test]
fn app_icon_file_name_uses_icon_key_for_windows_layout() {
    assert_eq!(app_icon_file_name("Google Chrome"), "google_chrome.png");
    assert_eq!(
        app_icon_file_name("Visual Studio Code"),
        "visual_studio_code.png"
    );
    assert_eq!(app_icon_file_name("1Password"), "1password.png");
}

#[test]
fn app_icon_file_name_falls_back_to_hash_when_key_is_empty() {
    let name = app_icon_file_name("!!!");
    assert!(name.ends_with(".png"));
    assert!(name.len() > 4);
}

fn touch(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

/// Minimal 1x1 transparent PNG used to prove the decode gate accepts real
/// raster images.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn replace_source_dir(label: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("icon-replace-test-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn validate_replace_source_accepts_a_decodable_png() {
    let dir = replace_source_dir("ok");
    touch(&dir, "pick.png", TINY_PNG);
    assert!(validate_replace_source(&dir.join("pick.png")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_replace_source_rejects_non_image_extension() {
    let dir = replace_source_dir("ext");
    touch(&dir, "notes.txt", b"secret");
    let err = validate_replace_source(&dir.join("notes.txt")).unwrap_err();
    assert!(err.contains("only image files"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_replace_source_rejects_undecodable_raster_bytes() {
    let dir = replace_source_dir("bytes");
    touch(&dir, "fake.png", b"this is not png data");
    let err = validate_replace_source(&dir.join("fake.png")).unwrap_err();
    assert!(err.contains("not a decodable image"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_replace_source_rejects_missing_and_oversized_files() {
    let dir = replace_source_dir("size");
    let missing = validate_replace_source(&dir.join("gone.png")).unwrap_err();
    assert!(missing.contains("not found"));
    let big = vec![0u8; (REPLACE_MAX_SOURCE_BYTES + 1) as usize];
    touch(&dir, "big.png", &big);
    let err = validate_replace_source(&dir.join("big.png")).unwrap_err();
    assert!(err.contains("10 MiB"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_replace_source_checks_ico_magic_and_svg_prolog() {
    let dir = replace_source_dir("magic");
    // Minimal single-image ICO: reserved word, type 1, one entry.
    let mut icon = vec![0u8, 0, 1, 0, 1, 0];
    icon.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    touch(&dir, "app.ico", &icon);
    assert!(validate_replace_source(&dir.join("app.ico")).is_ok());
    touch(&dir, "fake.ico", b"MZ-binary-masquerading-as-icon");
    assert!(validate_replace_source(&dir.join("fake.ico")).is_err());

    touch(&dir, "vector.svg", b"<?xml version=\"1.0\"?><svg></svg>");
    assert!(validate_replace_source(&dir.join("vector.svg")).is_ok());
    touch(&dir, "binary.svg", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3]);
    assert!(validate_replace_source(&dir.join("binary.svg")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn save_file_test_project(label: &str) -> std::path::PathBuf {
    let project =
        std::env::temp_dir().join(format!("save-file-test-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    project
}

#[test]
fn managed_source_path_accepts_a_file_under_the_image_root() {
    let project = save_file_test_project("ok");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&paths.images, "a.png", b"img");

    let resolved =
        managed_source_path(&paths, &paths.images.join("a.png").to_string_lossy()).unwrap();

    assert_eq!(resolved.file_name().unwrap(), "a.png");
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn managed_source_path_rejects_a_file_outside_managed_roots() {
    let project = save_file_test_project("outside");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&project, "secret.txt", b"secret");

    let err =
        managed_source_path(&paths, &project.join("secret.txt").to_string_lossy()).unwrap_err();

    assert!(err.contains("outside managed storage"), "{err}");
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn managed_source_path_rejects_a_missing_file() {
    let project = save_file_test_project("missing");
    let paths = StoragePaths::initialize(project.clone()).unwrap();

    let err = managed_source_path(&paths, "does-not-exist.png").unwrap_err();

    assert!(err.contains("not found"), "{err}");
    let _ = std::fs::remove_dir_all(&project);
}

fn item(
    kind: ClipboardKind,
    resource_path: Option<String>,
    metadata_json: Option<String>,
) -> ClipboardItem {
    ClipboardItem {
        id: "id".to_string(),
        kind,
        title: "title".to_string(),
        text_content: None,
        html_content: None,
        rtf_content: None,
        resource_path,
        preview_path: None,
        content_hash: "hash".to_string(),
        source_app: None,
        icon_path: None,
        size_bytes: 0,
        created_at_ms: 0,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json,
    }
}

#[test]
fn resolve_clipboard_file_paths_accepts_an_image_resource() {
    let project = save_file_test_project("media-root");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&paths.images, "a.png", b"img");

    let resolved = resolve_clipboard_file_paths(
        &item(
            ClipboardKind::Image,
            Some(paths.images.join("a.png").to_string_lossy().to_string()),
            None,
        ),
        &paths,
        None,
    )
    .unwrap();

    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].ends_with("a.png"));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn resolve_clipboard_file_paths_prefers_metadata_storage_paths() {
    let project = save_file_test_project("file-list");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&paths.files, "a.txt", b"a");
    touch(&paths.files, "b.txt", b"b");
    let metadata = Some(
        serde_json::json!({
            "files": [
                { "storagePath": paths.files.join("a.txt").to_string_lossy().to_string() },
                { "storagePath": paths.files.join("b.txt").to_string_lossy().to_string() },
            ]
        })
        .to_string(),
    );

    let resolved =
        resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
            .unwrap();

    assert_eq!(resolved.len(), 2);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn resolve_clipboard_file_paths_restores_the_original_file_name() {
    let project = save_file_test_project("original-name");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    // The managed copy carries a hash-derived name...
    touch(
        &paths.files,
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.txt",
        b"original content",
    );
    // ...while the user's real file still exists under its original name.
    touch(&project, "my report.txt", b"original content");
    let metadata = Some(
        serde_json::json!({
            "files": [{
                "storagePath": paths.files.join("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.txt").to_string_lossy().to_string(),
                "originalPath": project.join("my report.txt").to_string_lossy().to_string(),
            }]
        })
        .to_string(),
    );

    let resolved =
        resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
            .unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].file_name().unwrap().to_string_lossy(),
        "my report.txt"
    );
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn resolve_clipboard_file_paths_falls_back_to_storage_when_original_is_gone() {
    let project = save_file_test_project("original-gone");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&paths.files, "missing-original.txt", b"content");
    let metadata = Some(
        serde_json::json!({
            "files": [{
                "storagePath": paths.files.join("missing-original.txt").to_string_lossy().to_string(),
                "originalPath": project.join("gone.txt").to_string_lossy().to_string(),
            }]
        })
        .to_string(),
    );

    let resolved =
        resolve_clipboard_file_paths(&item(ClipboardKind::File, None, metadata), &paths, None)
            .unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].file_name().unwrap().to_string_lossy(),
        "missing-original.txt"
    );
    let _ = std::fs::remove_dir_all(&project);
}

/// A full file record persisted via the repository, mirroring how the
/// capture pipeline stores one managed copy plus its original location.
fn persisted_file_item(
    id: &str,
    content_hash: &str,
    created_at_ms: i64,
    storage_path: String,
    original_path: String,
) -> ClipboardItem {
    let mut item = item(ClipboardKind::File, Some(storage_path.clone()), None);
    item.id = id.to_owned();
    item.content_hash = content_hash.to_owned();
    item.created_at_ms = created_at_ms;
    item.metadata_json = Some(
        serde_json::json!({
            "files": [{
                "name": original_path.rsplit(['/', '\\']).next().unwrap_or(""),
                "storagePath": storage_path,
                "originalPath": original_path,
            }]
        })
        .to_string(),
    );
    item
}

#[test]
fn resolve_clipboard_file_paths_inherits_the_latest_copied_name() {
    let project = save_file_test_project("latest-name");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    let database = Database::open_in_memory().unwrap();
    // The managed storage copy (hash-named) still exists...
    touch(&paths.files, "a1b2c3d4.txt", b"same content");
    let storage = paths
        .files
        .join("a1b2c3d4.txt")
        .to_string_lossy()
        .to_string();
    // ...but the first copy's original was renamed away afterwards.
    let stale_original = project.join("Old Name.txt").to_string_lossy().to_string();
    // The most recent copy of the same content lives under a new name.
    let latest_original = project
        .join("Current Name.txt")
        .to_string_lossy()
        .to_string();
    touch(&project, "Current Name.txt", b"same content");

    let old = persisted_file_item(
        "file-old",
        "hash-old",
        100,
        storage.clone(),
        stale_original.clone(),
    );
    let new = persisted_file_item(
        "file-new",
        "hash-new",
        200,
        storage.clone(),
        latest_original.clone(),
    );
    database.save_item(&old).unwrap();
    database.save_item(&new).unwrap();

    let resolved = resolve_clipboard_file_paths(&old, &paths, Some(&database)).unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].file_name().unwrap().to_string_lossy(),
        "Current Name.txt"
    );

    // Without a database the stale original falls back to the (non-existent
    // here) managed copy; here it resolves to the managed storage name.
    let fallback = resolve_clipboard_file_paths(&old, &paths, None).unwrap();
    assert_eq!(
        fallback[0].file_name().unwrap().to_string_lossy(),
        "a1b2c3d4.txt"
    );
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn resolve_clipboard_file_paths_accepts_pass_through_absolute_paths() {
    let project = save_file_test_project("passthrough");
    let paths = StoragePaths::initialize(project.clone()).unwrap();
    touch(&project, "large.bin", b"big");
    let original = project.join("large.bin").to_string_lossy().to_string();

    let resolved = resolve_clipboard_file_paths(
        &item(ClipboardKind::File, Some(original), None),
        &paths,
        None,
    )
    .unwrap();

    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].ends_with("large.bin"));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn resolve_clipboard_file_paths_rejects_text_items_and_missing_files() {
    let project = save_file_test_project("cleanup");
    let paths = StoragePaths::initialize(project.clone()).unwrap();

    assert!(matches!(
        resolve_clipboard_file_paths(&item(ClipboardKind::Text, None, None), &paths, None),
        Err(ClipboardFilesCopyError::Failed(_))
    ));
    // A record whose file vanished is the one case the frontend reports
    // differently, so the distinction has to survive resolution.
    assert!(matches!(
        resolve_clipboard_file_paths(
            &item(ClipboardKind::File, Some("gone.txt".to_string()), None),
            &paths,
            None
        ),
        Err(ClipboardFilesCopyError::ResourceMissing(_))
    ));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn file_metadata_entries_reads_storage_and_original_path_fields() {
    let metadata = Some(
        r#"{"files":[{"storagePath":"C:\\managed\\a.txt","originalPath":"C:\\docs\\a.txt"},{"path":"C:\\orig\\b.txt"},{"name":"c.txt"}]}"#
            .to_owned(),
    );

    assert_eq!(
        file_metadata_entries(&metadata).unwrap(),
        vec![
            (
                Some("C:\\managed\\a.txt".to_owned()),
                Some("C:\\docs\\a.txt".to_owned())
            ),
            (Some("C:\\orig\\b.txt".to_owned()), None),
        ]
    );
    assert!(file_metadata_entries(&None).is_none());
    assert!(file_metadata_entries(&Some("{}".to_owned())).is_none());
}

#[test]
fn open_external_url_rejects_multibyte_text_without_panicking() {
    // 9 bytes with char boundaries at 0/3/6/9: slicing at byte 7 would
    // panic, so the scheme check must use a char-boundary-safe accessor.
    assert!(open_external_url("中中中".to_owned()).is_err());
    assert!(open_external_url("javascript:alert(1)".to_owned()).is_err());
}

#[test]
fn open_external_url_scheme_allowlist() {
    // Pure check so asserting accepted schemes never launches a handler.
    assert!(openable_scheme("https://example.com"));
    assert!(openable_scheme("HTTP://EXAMPLE.COM"));
    assert!(openable_scheme("mailto:user@example.com"));
    assert!(openable_scheme("tel:+123456789"));
    assert!(!openable_scheme("javascript:alert(1)"));
    assert!(!openable_scheme("file:///etc/passwd"));
    assert!(!openable_scheme(r"C:\Windows\notepad.exe"));
    assert!(!openable_scheme("中中中"));
}

#[test]
fn merged_entries_distinguish_apps_with_icon_orphans_and_iconless_apps() {
    let dir = std::env::temp_dir().join(format!("icon-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    touch(&dir, "google_chrome.png", b"icon1");
    touch(&dir, "orphan_stray.png", b"icon2");

    let apps = vec![
        (
            "Google Chrome".to_owned(),
            Some("google_chrome.png".to_owned()),
        ),
        ("Edge".to_owned(), None),
        ("Notepad".to_owned(), None),
    ];

    let entries = build_icon_cache(&dir, apps);

    let chrome = entries
        .iter()
        .find(|e| e.display_name == "Google Chrome")
        .unwrap();
    assert_eq!(chrome.app_name.as_deref(), Some("Google Chrome"));
    assert_eq!(chrome.icon_name.as_deref(), Some("google_chrome.png"));
    assert_eq!(chrome.size_bytes, 5);
    assert_eq!(chrome.target_icon_name, "google_chrome.png");

    let edge = entries.iter().find(|e| e.display_name == "Edge").unwrap();
    assert_eq!(edge.app_name.as_deref(), Some("Edge"));
    assert_eq!(edge.icon_name, None);
    assert_eq!(edge.size_bytes, 0);
    assert_eq!(edge.first_char, "E");

    let orphan = entries.iter().find(|e| e.app_name.is_none()).unwrap();
    assert_eq!(orphan.display_name, "orphan_stray");
    assert_eq!(orphan.icon_name.as_deref(), Some("orphan_stray.png"));
    assert_eq!(orphan.size_bytes, 5);
    assert_eq!(orphan.target_icon_name, "orphan_stray.png");

    // recorded apps sort before orphan files
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].display_name, "Edge");
    assert_eq!(entries[1].display_name, "Google Chrome");
    assert_eq!(entries[2].display_name, "Notepad");
    assert_eq!(entries[3].app_name, None);

    let _ = std::fs::remove_dir_all(&dir);
}
