use super::{clipboard_kind_name, escape_csv, ExportFormat, ExportOptions, CSV_TEXT_ENCODING};
use crate::{
    domain::ClipboardItem,
    storage::{Database, StorageError},
};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) fn matches(item: &ClipboardItem, options: &ExportOptions) -> bool {
    (options.include_favorites || !item.is_favorite)
        && options
            .date_from_ms
            .is_none_or(|from| item.created_at_ms >= from)
        && options.date_to_ms.is_none_or(|to| item.created_at_ms <= to)
        && (options.content_types.is_empty()
            || options
                .content_types
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case(clipboard_kind_name(item.kind))))
}

struct ExportWriter<'a, W> {
    writer: W,
    options: &'a ExportOptions,
    has_text: bool,
    count: usize,
}
impl<'a, W: Write> ExportWriter<'a, W> {
    fn new(mut writer: W, options: &'a ExportOptions) -> Result<Self, StorageError> {
        match options.format {
            ExportFormat::Json => writer.write_all(b"[")?,
            ExportFormat::Csv => writer.write_all(b"id,kind,title,text_content,source_app,created_at_ms,is_favorite,content_hash,clipboard_text_encoding\n")?,
            ExportFormat::PlainText => {},
        }
        Ok(Self {
            writer,
            options,
            count: 0,
            has_text: false,
        })
    }
    fn item(&mut self, item: &ClipboardItem) -> Result<(), StorageError> {
        if !matches(item, self.options) {
            return Ok(());
        }
        match self.options.format {
            ExportFormat::Json => {
                if self.count > 0 {
                    self.writer.write_all(b",")?;
                }
                self.writer.write_all(b"\n")?;
                serde_json::to_writer_pretty(&mut self.writer, item)?;
            }
            ExportFormat::Csv => writeln!(
                self.writer,
                "{},{},{},{},{},{},{},{},{}",
                escape_csv(&item.id),
                clipboard_kind_name(item.kind),
                escape_csv(&item.title),
                escape_csv(item.text_content.as_deref().unwrap_or("")),
                escape_csv(item.source_app.as_deref().unwrap_or("")),
                item.created_at_ms,
                item.is_favorite,
                escape_csv(&item.content_hash),
                CSV_TEXT_ENCODING
            )?,
            ExportFormat::PlainText => {
                if let Some(text) = &item.text_content {
                    if self.has_text {
                        self.writer.write_all(b"\n---\n")?;
                    }
                    self.writer.write_all(text.as_bytes())?;
                    self.has_text |= !text.is_empty();
                }
            }
        }
        self.count += 1;
        Ok(())
    }
    fn finish(mut self) -> Result<(), StorageError> {
        if self.options.format == ExportFormat::Json {
            self.writer
                .write_all(if self.count == 0 { b"]" } else { b"\n]" })?;
        }
        self.writer.flush()?;
        Ok(())
    }
}

pub fn export_database_to_writer(
    database: &Database,
    options: &ExportOptions,
    writer: impl Write,
) -> Result<(), StorageError> {
    let mut export = ExportWriter::new(writer, options)?;
    database.visit_active_items(|item| export.item(&item))?;
    export.finish()
}

pub(super) fn export_slice(
    items: &[ClipboardItem],
    options: &ExportOptions,
) -> Result<String, String> {
    let mut output = Vec::new();
    let mut export = ExportWriter::new(&mut output, options).map_err(|e| e.to_string())?;
    for item in items {
        export.item(item).map_err(|e| e.to_string())?;
    }
    export.finish().map_err(|e| e.to_string())?;
    String::from_utf8(output).map_err(|e| e.to_string())
}

/// Publish only after the complete payload is flushed and synced. Failed exports preserve the destination.
pub(crate) fn atomic_output(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<(), String>,
) -> Result<u64, String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(
        ".clipboard-export-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        write(&mut file)?;
        file.sync_all().map_err(|e| e.to_string())?;
        file.metadata().map(|m| m.len()).map_err(|e| e.to_string())
    })();
    drop(file);
    let result = result.and_then(|size| {
        crate::storage::replace_file(&temporary, path)
            .map(|()| size)
            .map_err(|e| e.to_string())
    });
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn export_database_to_file(
    database: &Database,
    options: &ExportOptions,
    path: &Path,
) -> Result<u64, String> {
    atomic_output(path, |file| {
        export_database_to_writer(database, options, BufWriter::new(file))
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_output_preserves_previous_file() {
        let root = std::env::temp_dir().join(format!("clipboard-stream-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("backup.json");
        std::fs::write(&path, "previous").unwrap();
        assert!(atomic_output(&path, |file| {
            file.write_all(b"partial").unwrap();
            Err("disk full".into())
        })
        .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
