use std::io::Read;

use rusqlite::Connection;

use crate::content::hash::compute_media_hash;
use crate::content::resource_metadata::RESOURCE_METADATA_SCHEMA_VERSION;
use crate::domain::{ClipboardItem, ClipboardKind};
use crate::export::ImportSummary;
use crate::storage::{ClipboardRepository, Database, StoragePaths};

pub(crate) const BACKUP_EXTENSION: &str = ".pastebackup";

/// Maximum uncompressed size for a single entry read from a PPaste backup
/// (a 76 MB SQLite database or PNG is already unusually large). Guards against
/// a crafted archive forcing unbounded heap allocation.
const MAX_PPASTE_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

struct PpRow {
    kind: ClipboardKind,
    text_content: Option<String>,
    html_content: Option<String>,
    rtf_content: Option<String>,
    image_filename: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    is_favorite: bool,
    created_at_ms: i64,
    source_app: Option<String>,
}

pub(crate) fn import_from_ppaste_backup(
    path: &str,
    database: &Database,
    paths: &StoragePaths,
) -> Result<ImportSummary, String> {
    let file =
        std::fs::File::open(path).map_err(|error| format!("failed to open {path}: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("invalid PPaste backup {path}: {error}"))?;

    let db_bytes = read_zip_entry(&mut archive, "PPaste2.db3")?;
    let db_path = extract_db_to_temp(&db_bytes)?;
    let result = read_records(&db_path).and_then(|(rows, files_dropped)| {
        let _ = std::fs::remove_file(&db_path);
        let mut summary = import_rows(&rows, &mut archive, database, paths)?;
        if files_dropped > 0 {
            summary.skipped_count += files_dropped;
            summary.errors.push(format!(
                "{files_dropped} file record(s) were skipped: they reference files on the original machine and their contents are not included in the backup"
            ));
        }
        Ok(summary)
    });
    let _ = std::fs::remove_file(&db_path);
    result
}

fn extract_db_to_temp(db_bytes: &[u8]) -> Result<String, String> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let db_path =
        std::env::temp_dir().join(format!("ppaste_import_{}_{unique}.db3", std::process::id()));
    std::fs::write(&db_path, db_bytes).map_err(|error| error.to_string())?;
    Ok(db_path.to_string_lossy().to_string())
}

fn read_records(db_path: &str) -> Result<(Vec<PpRow>, u64), String> {
    let connection = Connection::open(db_path)
        .map_err(|error| format!("failed to read PPaste database: {error}"))?;
    let mut statement = connection
        .prepare(
            "SELECT PUID, TYPE, TYPE_CHILD, VALUE, SEARCH, FAVORITE, CREATE_TIME,
                    SOURCEPATH, WIDTH, HEIGHT
             FROM PPaste_Main",
        )
        .map_err(|error| format!("PPaste database has unexpected schema: {error}"))?;

    let rows = statement
        .query_map([], |row| {
            let kind: Option<String> = row.get(1)?;
            let child: Option<String> = row.get(2)?;
            let value: Option<String> = row.get(3)?;
            let search: Option<String> = row.get(4)?;
            let favorite: Option<i64> = row.get(5)?;
            let created: Option<String> = row.get(6)?;
            let source: Option<String> = row.get(7)?;
            let width: Option<i64> = row.get(8)?;
            let height: Option<i64> = row.get(9)?;
            let is_files = kind.as_deref() == Some("Files");
            Ok((
                map_row(
                    kind, child, value, search, favorite, created, source, width, height,
                ),
                is_files,
            ))
        })
        .map_err(|error| format!("failed to read PPaste records: {error}"))?;

    let mut out = Vec::new();
    let mut files_dropped = 0u64;
    for row in rows {
        let Ok((record, is_files)) = row else {
            continue;
        };
        if is_files {
            files_dropped += 1;
            continue;
        }
        if let Some(record) = record {
            out.push(record);
        }
    }
    Ok((out, files_dropped))
}

#[allow(clippy::too_many_arguments)]
fn map_row(
    kind: Option<String>,
    child: Option<String>,
    value: Option<String>,
    search: Option<String>,
    favorite: Option<i64>,
    created: Option<String>,
    source: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
) -> Option<PpRow> {
    let kind = kind.as_deref()?;
    let child = child.as_deref().unwrap_or("");
    let value = value.as_deref();
    let search = search.as_deref();
    let is_favorite = favorite.unwrap_or(0) != 0;
    let created_at_ms = created
        .as_deref()
        .and_then(ppaste_datetime_to_ms)
        .unwrap_or_else(now_ms);
    let source_app = clean(source.as_deref());

    let row = match (kind, child) {
        ("Text", "Image") | ("Image", "Image") | ("Image", "") => PpRow {
            kind: ClipboardKind::Image,
            text_content: None,
            html_content: None,
            rtf_content: None,
            image_filename: clean(value),
            width,
            height,
            is_favorite,
            created_at_ms,
            source_app,
        },
        ("Text", "Links") | ("Text", "Url") => PpRow {
            kind: ClipboardKind::Link,
            text_content: text_value(search, value),
            html_content: None,
            rtf_content: None,
            image_filename: None,
            width: None,
            height: None,
            is_favorite,
            created_at_ms,
            source_app,
        },
        ("Text", "HtmlText") => {
            let html = clean(value);
            let text = text_value(search, value);
            PpRow {
                kind: ClipboardKind::Text,
                text_content: text,
                html_content: html,
                rtf_content: None,
                image_filename: None,
                width: None,
                height: None,
                is_favorite,
                created_at_ms,
                source_app,
            }
        }
        ("Text", _) => PpRow {
            kind: ClipboardKind::Text,
            text_content: text_value(search, value),
            html_content: None,
            rtf_content: None,
            image_filename: None,
            width: None,
            height: None,
            is_favorite,
            created_at_ms,
            source_app,
        },
        // "Files" records reference the source machine's absolute paths and are
        // not self-contained in the backup, so they are skipped.
        _ => return None,
    };
    Some(row)
}

fn import_rows(
    rows: &[PpRow],
    archive: &mut zip::ZipArchive<std::fs::File>,
    database: &Database,
    paths: &StoragePaths,
) -> Result<ImportSummary, String> {
    let mut imported = 0u64;
    let mut skipped = 0u64;
    let mut errors = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        let id = format!("ppaste_{index}");
        match build_item(row, &id, archive, paths) {
            Ok(item) => {
                let already_exists = database
                    .content_exists(item.kind, &item.content_hash)
                    .map_err(|error| {
                        format!("failed to check existing record for {id}: {error}")
                    })?;
                if already_exists {
                    // Same content already present: skip instead of upserting so a
                    // duplicate import does not add or touch the existing record.
                    skipped += 1;
                    continue;
                }
                match ClipboardRepository::save_item(database, &item) {
                    Ok(_) => imported += 1,
                    Err(error) => {
                        skipped += 1;
                        errors.push(format!("failed to import {id}: {error}"));
                    }
                }
            }
            Err(error) => {
                skipped += 1;
                errors.push(format!("failed to import {id}: {error}"));
            }
        }
    }

    Ok(ImportSummary {
        imported_count: imported,
        skipped_count: skipped,
        errors,
        pending_truncation: 0,
        max_items: 0,
    })
}

fn build_item(
    row: &PpRow,
    id: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    paths: &StoragePaths,
) -> Result<ClipboardItem, String> {
    let mut item = ClipboardItem {
        id: id.to_owned(),
        kind: row.kind,
        title: default_title(row),
        text_content: row.text_content.clone(),
        html_content: row.html_content.clone(),
        rtf_content: row.rtf_content.clone(),
        resource_path: None,
        preview_path: None,
        content_hash: String::new(),
        source_app: row.source_app.clone(),
        icon_path: None,
        size_bytes: 0,
        created_at_ms: row.created_at_ms,
        last_used_at_ms: None,
        is_favorite: row.is_favorite,
        metadata_json: None,
    };

    if row.kind == ClipboardKind::Image {
        let filename = row
            .image_filename
            .as_deref()
            .ok_or_else(|| "image record missing filename".to_owned())?;
        let bytes = read_zip_entry(archive, &format!("PasteData/{filename}"))
            .map_err(|_| format!("image file PasteData/{filename} is missing from the backup"))?;
        let content_hash = compute_media_hash("image", &bytes);

        let image_dir = paths.images.clone();
        std::fs::create_dir_all(&image_dir).map_err(|e| e.to_string())?;
        let img_path = image_dir.join(format!("{content_hash}.png"));
        if !img_path.exists() {
            std::fs::write(&img_path, &bytes).map_err(|e| e.to_string())?;
        }
        let resource_path = img_path.to_string_lossy().to_string();

        let (width, height) = match (row.width, row.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
            _ => image::load_from_memory(&bytes)
                .map(|image| (i64::from(image.width()), i64::from(image.height())))
                .unwrap_or((0, 0)),
        };

        let metadata = serde_json::json!({
            "schemaVersion": RESOURCE_METADATA_SCHEMA_VERSION,
            "width": width,
            "height": height,
            "mimeType": "image/png",
            "extension": "png",
            "sizeBytes": bytes.len(),
            "resourcePath": resource_path,
            "previewPath": resource_path,
            "storagePath": resource_path,
            "contentHash": content_hash,
        });

        item.resource_path = Some(resource_path.clone());
        item.preview_path = Some(resource_path);
        item.content_hash = content_hash.clone();
        item.size_bytes = bytes.len() as u64;
        item.title = content_hash[..24.min(content_hash.len())].to_owned();
        item.metadata_json = Some(metadata.to_string());
    } else {
        let text = item
            .text_content
            .as_deref()
            .or(item.html_content.as_deref())
            .unwrap_or_default();
        item.content_hash =
            crate::content::hash::compute_content_hash(pp_kind_name(item.kind), text, None);
        item.size_bytes = text.len() as u64;
    }

    Ok(item)
}

fn pp_kind_name(kind: ClipboardKind) -> &'static str {
    match kind {
        ClipboardKind::Text => "text",
        ClipboardKind::Link => "link",
        ClipboardKind::Image => "image",
        ClipboardKind::File => "file",
    }
}

fn default_title(row: &PpRow) -> String {
    row.text_content
        .as_deref()
        .or(row.html_content.as_deref())
        .map(|text| {
            let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let cut: String = compact.chars().take(120).collect();
            if cut.len() < compact.len() {
                format!("{cut}…")
            } else {
                cut
            }
        })
        .unwrap_or_else(|| match row.kind {
            ClipboardKind::Image => "Image".to_owned(),
            ClipboardKind::Link => "Link".to_owned(),
            _ => "Clipboard".to_owned(),
        })
}

fn text_value(search: Option<&str>, value: Option<&str>) -> Option<String> {
    clean(search).or_else(|| clean(value))
}

fn clean(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() || trimmed == "NULL" {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, String> {
    // PPaste backups are produced on Windows and store zip entry paths with
    // backslashes (e.g. `PasteData\1745.png`). Some exports use forward
    // slashes. Try both separators so image files are found either way.
    let mut candidates: Vec<String> = vec![name.to_owned()];
    if name.contains('/') {
        candidates.push(name.replace('/', "\\"));
    } else if name.contains('\\') {
        candidates.push(name.replace('\\', "/"));
    }
    for candidate in candidates {
        if let Ok(entry) = archive.by_name(&candidate) {
            let declared = entry.size();
            // Reject a single entry whose uncompressed size is unreasonably
            // large for a PPaste backup (a 76 MB DB or PNG is already large),
            // so a crafted archive cannot force unbounded heap allocation.
            if declared > MAX_PPASTE_ENTRY_BYTES {
                return Err(format!(
                    "entry {candidate} in backup declares {declared} bytes, exceeding the {MAX_PPASTE_ENTRY_BYTES} limit"
                ));
            }
            let mut bytes = Vec::new();
            entry
                .take(MAX_PPASTE_ENTRY_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("failed to read {candidate} in backup: {error}"))?;
            if bytes.len() as u64 > MAX_PPASTE_ENTRY_BYTES {
                return Err(format!(
                    "entry {candidate} in backup exceeds the {MAX_PPASTE_ENTRY_BYTES} limit"
                ));
            }
            return Ok(bytes);
        }
    }
    Err(format!("missing {name} in backup"))
}

fn ppaste_datetime_to_ms(datetime: &str) -> Option<i64> {
    let numbers: Vec<i64> = datetime
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    if numbers.len() < 3 {
        return None;
    }
    let year = numbers[0];
    let month = numbers[1];
    let day = numbers[2];
    let hour = numbers.get(3).copied().unwrap_or(0);
    let minute = numbers.get(4).copied().unwrap_or(0);
    let second = numbers.get(5).copied().unwrap_or(0);
    // PPaste timestamps are ordinary calendar times; reject adversarial or
    // malformed fields rather than truncating or overflowing on them.
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
        || !(1..=9999).contains(&year)
    {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    let seconds = days
        .saturating_mul(86_400)
        .saturating_add(hour.saturating_mul(3_600))
        .saturating_add(minute.saturating_mul(60))
        .saturating_add(second);
    Some(seconds.saturating_mul(1_000))
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let yoe = adjusted_year - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn make_source_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE PPaste_Main (
                PUID TEXT PRIMARY KEY,
                TYPE TEXT, TYPE_CHILD TEXT, VALUE TEXT, SEARCH TEXT,
                FAVORITE INTEGER, CREATE_TIME DATETIME,
                SOURCEPATH TEXT, WIDTH INTEGER, HEIGHT INTEGER
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO PPaste_Main
                (PUID, TYPE, TYPE_CHILD, VALUE, SEARCH, FAVORITE, CREATE_TIME, SOURCEPATH, WIDTH, HEIGHT)
             VALUES
                ('T1', 'Text', 'UnicodeText', 'hello world', 'hello world', 1, '2026-04-27 12:15:22', 'Zen', 0, 0),
                ('T2', 'Text', 'HtmlText', '<html><body><p>hi</p></body></html>', 'hi', 0, '2026-04-27 12:15:23', 'Zen', 0, 0),
                ('L1', 'Text', 'Links', 'https://example.com', 'https://example.com', 0, '2026-04-27 12:15:24', 'Chrome', 0, 0),
                ('I1', 'Image', 'Image', 'pixel.png', NULL, 0, '2026-04-27 12:15:25', 'Zen', 2, 2),
                ('F1', 'Files', 'Files', '[\"C:\\\\missing.txt\"]', NULL, 0, '2026-04-27 12:15:26', 'Explorer', 0, 0);",
            [],
        )
        .unwrap();
    }

    fn make_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn make_backup(temp: &Path) -> PathBuf {
        let db_path = temp.join("source.db3");
        make_source_db(&db_path);
        let db_bytes = std::fs::read(&db_path).unwrap();

        let backup = temp.join("test.Pastebackup");
        let file = std::fs::File::create(&backup).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("PPaste2.db3", options).unwrap();
        writer.write_all(&db_bytes).unwrap();
        writer.start_file("PasteData/pixel.png", options).unwrap();
        writer.write_all(&make_png()).unwrap();
        writer.finish().unwrap();
        backup
    }

    #[test]
    fn imports_text_and_images_from_ppaste_backup() {
        let temp = std::env::temp_dir().join(format!(
            "ppaste-import-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let backup = make_backup(&temp);

        let database = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            temp.clone(),
            None,
            None,
            None,
        )
        .unwrap();

        let summary =
            import_from_ppaste_backup(backup.to_str().unwrap(), &database, &paths).unwrap();

        // 3 text/link + 1 image imported; the non-portable Files record is skipped with a note.
        assert_eq!(summary.imported_count, 4);
        assert_eq!(summary.skipped_count, 1);
        assert_eq!(summary.errors.len(), 1);
        assert!(summary.errors[0].contains("file record(s) were skipped"));
        assert_eq!(database.item_count().unwrap(), 4);

        let stored = database
            .list_recent(10, 0, &crate::storage::HistoryFilter::default())
            .unwrap();
        assert!(stored.iter().any(|i| i.kind == ClipboardKind::Link));
        assert!(stored.iter().any(|i| i.kind == ClipboardKind::Image));
        let image_item = stored
            .iter()
            .find(|i| i.kind == ClipboardKind::Image)
            .unwrap();
        assert!(image_item
            .resource_path
            .as_deref()
            .unwrap()
            .ends_with(".png"));
        assert!(std::path::Path::new(image_item.resource_path.as_deref().unwrap()).exists());
        let metadata: serde_json::Value =
            serde_json::from_str(image_item.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["schemaVersion"], RESOURCE_METADATA_SCHEMA_VERSION);
        assert_eq!(metadata["width"], 2);
        assert_eq!(metadata["height"], 2);
        assert_eq!(metadata["mimeType"], "image/png");
        assert_eq!(
            metadata["resourcePath"],
            image_item.resource_path.as_deref().unwrap()
        );
        assert_eq!(metadata["contentHash"], image_item.content_hash);

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn reimporting_same_backup_skips_existing_records() {
        let temp = std::env::temp_dir().join(format!(
            "ppaste-reimport-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let backup = make_backup(&temp);

        let database = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            temp.clone(),
            None,
            None,
            None,
        )
        .unwrap();

        let first = import_from_ppaste_backup(backup.to_str().unwrap(), &database, &paths).unwrap();
        assert_eq!(first.imported_count, 4);
        assert_eq!(first.skipped_count, 1);
        assert_eq!(database.item_count().unwrap(), 4);

        let second =
            import_from_ppaste_backup(backup.to_str().unwrap(), &database, &paths).unwrap();
        assert_eq!(second.imported_count, 0);
        assert_eq!(second.skipped_count, 5);
        assert_eq!(second.errors.len(), 1);
        assert!(second.errors[0].contains("file record(s) were skipped"));
        assert_eq!(database.item_count().unwrap(), 4);

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn imports_images_when_zip_uses_backslash_paths() {
        let temp = std::env::temp_dir().join(format!(
            "ppaste-backslash-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let db_path = temp.join("source.db3");
        make_source_db(&db_path);
        let db_bytes = std::fs::read(&db_path).unwrap();

        // PPaste exports entry names with Windows backslashes (PasteData\...).
        let backup = temp.join("backslash.Pastebackup");
        let file = std::fs::File::create(&backup).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("PPaste2.db3", options).unwrap();
        writer.write_all(&db_bytes).unwrap();
        writer.start_file("PasteData\\pixel.png", options).unwrap();
        writer.write_all(&make_png()).unwrap();
        writer.finish().unwrap();

        let database = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize_with_resource_directories_for_configuration(
            temp.clone(),
            None,
            None,
            None,
        )
        .unwrap();

        let summary =
            import_from_ppaste_backup(backup.to_str().unwrap(), &database, &paths).unwrap();
        assert_eq!(summary.imported_count, 4);
        assert_eq!(summary.skipped_count, 1);
        assert_eq!(summary.errors.len(), 1);
        assert!(database
            .list_recent(10, 0, &crate::storage::HistoryFilter::default())
            .unwrap()
            .iter()
            .any(|i| i.kind == ClipboardKind::Image));

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn ppaste_datetime_is_parsed_to_millis() {
        // PPaste stores CREATE_TIME as local time; we parse it as UTC so all
        // imported timestamps share one deterministic interpretation and keep
        // relative order (a fixed local/UTC offset shift is preserved).
        assert_eq!(
            ppaste_datetime_to_ms("2026-04-27 12:15:22"),
            Some(1777292122000)
        );
    }

    #[test]
    fn ppaste_datetime_rejects_out_of_range_fields_without_overflow() {
        assert_eq!(ppaste_datetime_to_ms("2026-04-45 12:15:22"), None);
        assert_eq!(ppaste_datetime_to_ms("2026-00-27 12:15:22"), None);
        assert_eq!(ppaste_datetime_to_ms("2026-04-27 25:15:22"), None);
        assert_eq!(ppaste_datetime_to_ms("2026-04-27 12:99:22"), None);
        // Adversarially large numbers must not truncate, wrap, or overflow.
        assert_eq!(
            ppaste_datetime_to_ms("9223372036854775807-04-27 12:15:22"),
            None
        );
        assert_eq!(ppaste_datetime_to_ms("0000-04-27 12:15:22"), None);
    }
}
