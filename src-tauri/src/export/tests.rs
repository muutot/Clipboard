//! Export/import unit tests, moved verbatim with the export module.

use super::*;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::storage::ClipboardRepository;

fn sample_items() -> Vec<ClipboardItem> {
    vec![
        ClipboardItem {
            id: "item-1".to_owned(),
            kind: ClipboardKind::Text,
            title: "Hello".to_owned(),
            text_content: Some("Hello, world!".to_owned()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "abc".to_owned(),
            source_app: Some("Notepad".to_owned()),
            size_bytes: 13,
            created_at_ms: 1000,
            last_used_at_ms: None,
            is_favorite: true,
            icon_path: None,
            metadata_json: None,
        },
        ClipboardItem {
            id: "item-2".to_owned(),
            kind: ClipboardKind::Link,
            title: "Example".to_owned(),
            text_content: Some("https://example.com".to_owned()),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "def".to_owned(),
            source_app: Some("Chrome".to_owned()),
            size_bytes: 19,
            created_at_ms: 2000,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        },
    ]
}

#[test]
fn exports_to_json_format() {
    let items = sample_items();
    let options = ExportOptions {
        format: ExportFormat::Json,
        include_favorites: true,
        date_from_ms: None,
        date_to_ms: None,
        content_types: vec!["text".to_owned(), "link".to_owned()],
    };

    let result = export_items(&items, &options).unwrap();
    assert!(result.contains("item-1"));
    assert!(result.contains("item-2"));
    assert!(result.contains("Hello, world!"));
}

#[test]
fn exports_to_csv_format() {
    let items = sample_items();
    let options = ExportOptions {
        format: ExportFormat::Csv,
        include_favorites: true,
        date_from_ms: None,
        date_to_ms: None,
        content_types: vec!["text".to_owned(), "link".to_owned()],
    };

    let result = export_items(&items, &options).unwrap();
    assert!(result.starts_with("id,kind,title,text_content"));
    assert!(result.contains("item-1"));
    assert!(result.contains("item-2"));
}

#[test]
fn exports_to_plain_text_format() {
    let items = sample_items();
    let options = ExportOptions {
        format: ExportFormat::PlainText,
        include_favorites: true,
        date_from_ms: None,
        date_to_ms: None,
        content_types: vec!["text".to_owned(), "link".to_owned()],
    };

    let result = export_items(&items, &options).unwrap();
    assert!(result.contains("Hello, world!"));
    assert!(result.contains("https://example.com"));
}

#[test]
fn imports_from_json_into_database() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    let json = serde_json::to_string(&sample_items()).unwrap();

    let summary = import_from_json(&json, &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    assert_eq!(summary.skipped_count, 0);
    assert!(summary.errors.is_empty());
}

#[test]
fn reimporting_same_json_skips_without_rewriting_stored_rows() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    let json = serde_json::to_string(&sample_items()).unwrap();

    let first = import_from_json(&json, &database).unwrap();
    assert_eq!(first.imported_count, 2);
    assert_eq!(first.skipped_count, 0);

    // A crafted re-import carries the same identities but hostile content:
    // it must be skipped, never overwrite the stored rows.
    let hostile = r#"[{"id":"item-1","kind":"text","title":"HOSTILE","textContent":"Hello, world!","htmlContent":null,"rtfContent":null,"resourcePath":"C:\\evil.txt","previewPath":null,"contentHash":"abc","sourceApp":"Evil","iconPath":null,"sizeBytes":13,"createdAtMs":999999,"lastUsedAtMs":null,"isFavorite":true,"metadataJson":null}]"#;
    let second = import_from_json(hostile, &database).unwrap();
    assert_eq!(second.imported_count, 0);
    assert_eq!(second.skipped_count, 1);

    let stored = database.get_item("item-1").unwrap().unwrap();
    assert_eq!(stored.title, "Hello");
    assert_eq!(stored.created_at_ms, 1000);
    assert_eq!(stored.resource_path, None);
}

#[test]
fn json_import_stores_only_normalized_icon_file_keys() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    let mut items = sample_items();
    items[0].icon_path =
        Some(r"C:\Users\admin\AppData\Local\Clipboard\icons\Notepad.png".to_owned());
    items[1].icon_path = Some("../../foreign/icons/Chrome.png".to_owned());
    let json = serde_json::to_string(&items).unwrap();

    let summary = import_from_json(&json, &database).unwrap();

    assert_eq!(summary.imported_count, 2);
    assert_eq!(
        database.get_item("item-1").unwrap().unwrap().icon_path,
        Some("Notepad.png".to_owned())
    );
    assert_eq!(
        database.get_item("item-2").unwrap().unwrap().icon_path,
        Some("Chrome.png".to_owned())
    );
}

#[test]
fn imported_resource_path_is_sanitized() {
    assert_eq!(sanitize_imported_path(None), None);
    assert_eq!(sanitize_imported_path(Some("   ")), None);
    assert_eq!(sanitize_imported_path(Some("../escape.png")), None);
    assert_eq!(
        sanitize_imported_path(Some("C:/definitely/not/existing/x.png")),
        None
    );
    let temp = std::env::temp_dir().join(format!("clipboard-import-path-{}", std::process::id()));
    std::fs::write(&temp, b"x").unwrap();
    assert_eq!(
        sanitize_imported_path(Some(temp.to_str().unwrap())),
        Some(temp.to_string_lossy().into_owned())
    );
    let _ = std::fs::remove_file(&temp);
}

#[test]
fn imported_icon_key_normalization_preserves_legacy_keys_and_rejects_unsafe_names() {
    assert_eq!(normalize_imported_icon_key(None), None);
    assert_eq!(normalize_imported_icon_key(Some("   ")), None);
    assert_eq!(
        normalize_imported_icon_key(Some("Notepad.png")),
        Some("Notepad.png".to_owned())
    );
    assert_eq!(
        normalize_imported_icon_key(Some("/home/user/.local/share/icons/firefox.png")),
        Some("firefox.png".to_owned())
    );
    assert_eq!(
        normalize_imported_icon_key(Some(r"..\..\icons\微信.png")),
        Some("微信.png".to_owned())
    );
    assert_eq!(normalize_imported_icon_key(Some("../../icons/")), None);
    assert_eq!(normalize_imported_icon_key(Some("..")), None);
    assert_eq!(normalize_imported_icon_key(Some("unsafe?.png")), None);
    assert_eq!(normalize_imported_icon_key(Some("..%2fsecret.png")), None);
}

#[test]
fn export_options_filter_favorites_dates_and_types() {
    let items = sample_items();
    let options = ExportOptions {
        format: ExportFormat::Json,
        include_favorites: false,
        date_from_ms: Some(1500),
        date_to_ms: Some(2500),
        content_types: vec!["link".to_owned()],
    };

    let result = export_items(&items, &options).unwrap();
    assert!(result.contains("item-2"));
    assert!(!result.contains("item-1"));
}

#[test]
fn json_export_import_round_trips_html_content() {
    let mut items = sample_items();
    items[0].html_content = Some("<b>Hello, world!</b>".to_owned());
    items[0].rtf_content = Some("{\\rtf1\\b Hello, world!}".to_owned());
    let json = serde_json::to_string(&items).unwrap();

    let database = crate::storage::Database::open_in_memory().unwrap();
    let summary = import_from_json(&json, &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    assert_eq!(
        database.get_item("item-1").unwrap().unwrap().html_content,
        Some("<b>Hello, world!</b>".to_owned())
    );
    assert_eq!(
        database.get_item("item-1").unwrap().unwrap().rtf_content,
        Some("{\\rtf1\\b Hello, world!}".to_owned())
    );
}

#[test]
fn imports_older_json_without_html_content_field() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    let json = r#"[{"id":"legacy","kind":"text","title":"legacy","textContent":"plain","resourcePath":null,"previewPath":null,"contentHash":"hash","sourceApp":null,"iconPath":null,"sizeBytes":5,"createdAtMs":1,"lastUsedAtMs":null,"isFavorite":false,"metadataJson":null}]"#;

    let summary = import_from_json(json, &database).unwrap();
    assert_eq!(summary.imported_count, 1);
    let item = database.get_item("legacy").unwrap().unwrap();
    assert_eq!(item.html_content, None);
    assert_eq!(item.rtf_content, None);
}

#[test]
fn imports_plain_text_chunks() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    let summary = import_from_plain_text("first\n---\nsecond\n", &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    assert_eq!(database.item_count().unwrap(), 2);
}

#[test]
fn imports_crlf_plain_text_separators_without_changing_record_line_endings() {
    let database = Database::open_in_memory().unwrap();
    let summary =
        import_from_plain_text("first\r\ncontinued\r\n---\r\nsecond\r\n", &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    let items = database
        .list_recent(10, 0, &crate::storage::HistoryFilter::default())
        .unwrap();
    assert!(items
        .iter()
        .any(|item| item.text_content.as_deref() == Some("first\r\ncontinued")));
    assert!(items
        .iter()
        .any(|item| item.text_content.as_deref() == Some("second")));
}

#[test]
fn database_export_reads_more_than_one_page() {
    let database = crate::storage::Database::open_in_memory().unwrap();
    for index in 0..510 {
        let item = ClipboardItem {
            id: format!("item-{index}"),
            kind: ClipboardKind::Text,
            title: format!("item-{index}"),
            text_content: Some(format!("text-{index}")),
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{index}"),
            source_app: None,
            size_bytes: 8,
            created_at_ms: index,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        };
        crate::storage::ClipboardRepository::save_item(&database, &item).unwrap();
    }

    let output = export_database(
        &database,
        &ExportOptions {
            format: ExportFormat::Json,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec![],
        },
    )
    .unwrap();
    let exported: Vec<ClipboardItem> = serde_json::from_str(&output).unwrap();
    assert_eq!(exported.len(), 510);
}

#[test]
fn database_export_keeps_one_snapshot_during_concurrent_usage_updates() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let directory = std::env::temp_dir().join(format!("clipboard-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("history.sqlite3");
    let database = Database::open(&path).unwrap();
    let entries: Vec<_> = (0..510)
        .map(|index| {
            let mut item = sample_items().remove(0);
            item.id = format!("export-{index}");
            item.content_hash = item.id.clone();
            item.created_at_ms = index;
            item.last_used_at_ms = Some(index);
            (item.id.clone(), item)
        })
        .collect();
    assert_eq!(
        database
            .save_items_transactional(&entries)
            .unwrap()
            .imported_count,
        510
    );

    // Commit on a separate real WAL connection while the export's SELECT
    // is already reading. The oldest record jumps across the page boundary.
    let writer = Database::open(&path).unwrap();
    let wrote = Arc::new(AtomicBool::new(false));
    let did_write = wrote.clone();
    database
        .with_connection(|connection| {
            connection.progress_handler(
                1000,
                Some(move || {
                    if !did_write.swap(true, Ordering::SeqCst) {
                        assert!(writer.set_last_used("export-0").unwrap());
                    }
                    false
                }),
            )?;
            Ok(())
        })
        .unwrap();

    let output = export_database(
        &database,
        &ExportOptions {
            format: ExportFormat::Json,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec![],
        },
    )
    .unwrap();
    let exported: Vec<ClipboardItem> = serde_json::from_str(&output).unwrap();
    assert!(
        wrote.load(Ordering::SeqCst),
        "concurrent update must actually run"
    );
    assert_eq!(exported.len(), 510);
    let unique: std::collections::HashSet<_> = exported.iter().map(|item| &item.id).collect();
    assert_eq!(
        unique.len(),
        510,
        "export must neither duplicate nor omit records"
    );
    assert_eq!(
        exported
            .iter()
            .find(|item| item.id == "export-0")
            .unwrap()
            .last_used_at_ms,
        Some(0)
    );
    database
        .with_connection(|connection| {
            connection.progress_handler(0, None::<fn() -> bool>)?;
            Ok(())
        })
        .unwrap();
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn escape_csv_encodes_formula_cells_as_literal_text() {
    assert_eq!(escape_csv("plain"), "plain");
    assert_eq!(escape_csv("=SUM(A1)"), "\"'=SUM(A1)\"");
    assert_eq!(escape_csv("+123"), "\"'+123\"");
    assert_eq!(escape_csv("-danger"), "\"'-danger\"");
    assert_eq!(escape_csv("@cmd"), "\"'@cmd\"");
    assert_eq!(escape_csv(" =SUM(A1)"), "\"' =SUM(A1)\"");
    assert_eq!(escape_csv("a,b"), "\"a,b\"");
    assert_eq!(escape_csv("safe"), "safe");
}

#[test]
fn csv_export_keeps_formula_content_as_text_and_round_trips_it() {
    let mut items = sample_items();
    items[0].title = "=1+1".to_owned();
    items[0].text_content = Some("\t=2+2".to_owned());
    items[0].source_app = Some("'literal source".to_owned());
    items[1].text_content = Some("'=literal text".to_owned());
    let output = export_items(
        &items,
        &ExportOptions {
            format: ExportFormat::Csv,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec![],
        },
    )
    .unwrap();
    let mut reader = CsvReader::new(&output);
    reader.next_record().unwrap();
    let row = reader.next_record().unwrap();
    assert!(
        row[2].starts_with('\''),
        "CSV quoting alone still exposes a formula cell"
    );
    assert!(row[3].starts_with('\''));
    let database = Database::open_in_memory().unwrap();
    assert_eq!(
        import_from_csv(&output, &database).unwrap().imported_count,
        2
    );
    for original in items {
        let restored = database.get_item(&original.id).unwrap().unwrap();
        assert_eq!(restored.title, original.title);
        assert_eq!(restored.text_content, original.text_content);
        assert_eq!(restored.source_app, original.source_app);
    }
}

#[test]
fn csv_without_encoding_marker_preserves_a_literal_apostrophe() {
    let database = Database::open_in_memory().unwrap();
    let csv = "id,kind,title,text_content\nlegacy,text,'title,'=literal\n";
    assert_eq!(import_from_csv(csv, &database).unwrap().imported_count, 1);
    let item = database.get_item("legacy").unwrap().unwrap();
    assert_eq!(item.title, "'title");
    assert_eq!(item.text_content.as_deref(), Some("'=literal"));
}

#[test]
fn csv_reader_preserves_quoted_fields_with_separators_and_newlines() {
    let mut reader = CsvReader::new("a,\"b,c\",\"line1\nline2\",d\n");
    let record = reader.next_record().unwrap();
    assert_eq!(record, vec!["a", "b,c", "line1\nline2", "d"]);
    assert!(reader.next_record().is_none());
}

#[test]
fn csv_reader_unescapes_doubled_quotes() {
    let mut reader = CsvReader::new("\"he said \"\"hi\"\"\",end\n");
    let record = reader.next_record().unwrap();
    assert_eq!(record, vec!["he said \"hi\"", "end"]);
}

#[test]
fn csv_export_import_round_trips_text_and_link_items() {
    let items = sample_items();
    let csv = export_items(
        &items,
        &ExportOptions {
            format: ExportFormat::Csv,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec![],
        },
    )
    .unwrap();

    let database = crate::storage::Database::open_in_memory().unwrap();
    let summary = import_from_csv(&csv, &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    assert_eq!(summary.skipped_count, 0);
    assert!(summary.errors.is_empty());

    let text_item = database.get_item("item-1").unwrap().unwrap();
    assert_eq!(text_item.kind, ClipboardKind::Text);
    assert_eq!(text_item.text_content.as_deref(), Some("Hello, world!"));
    assert_eq!(text_item.source_app.as_deref(), Some("Notepad"));
    assert!(text_item.is_favorite);

    let link_item = database.get_item("item-2").unwrap().unwrap();
    assert_eq!(link_item.kind, ClipboardKind::Link);
    assert_eq!(
        link_item.text_content.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn csv_export_import_keeps_distinct_media_rows() {
    let items = vec![
        ClipboardItem {
            id: "img-1".to_owned(),
            kind: ClipboardKind::Image,
            title: "first".to_owned(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-image-1".to_owned(),
            source_app: None,
            size_bytes: 10,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        },
        ClipboardItem {
            id: "img-2".to_owned(),
            kind: ClipboardKind::Image,
            title: "second".to_owned(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: "hash-image-2".to_owned(),
            source_app: None,
            size_bytes: 20,
            created_at_ms: 2,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        },
    ];
    let csv = export_items(
        &items,
        &ExportOptions {
            format: ExportFormat::Csv,
            include_favorites: true,
            date_from_ms: None,
            date_to_ms: None,
            content_types: vec![],
        },
    )
    .unwrap();

    let database = crate::storage::Database::open_in_memory().unwrap();
    let summary = import_from_csv(&csv, &database).unwrap();
    assert_eq!(summary.imported_count, 2);
    assert_eq!(database.item_count().unwrap(), 2);
    assert_eq!(
        database.get_item("img-1").unwrap().unwrap().content_hash,
        "hash-image-1"
    );
    assert_eq!(
        database.get_item("img-2").unwrap().unwrap().content_hash,
        "hash-image-2"
    );
}
