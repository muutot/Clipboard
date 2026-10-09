//! Portable active-history bundles. No configuration, credentials, recycle bin or derived previews.
use super::{stream::atomic_output, ImportSummary};
use crate::background_operations::OperationPhase;
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    storage::{ClipboardRepository, Database, StorageError, StoragePaths},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

type Progress<'a> = &'a mut dyn FnMut(OperationPhase, u64, Option<u64>) -> Result<(), String>;

const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: u64 = 512 * 1024 * 1024;
const MAX_RESOURCE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 16 * 1024 * 1024 * 1024;
const MAX_LINE: u64 = 32 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: u32,
    item_count: usize,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    name: String,
    bytes: u64,
    sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    pub fingerprint: String,
    pub item_count: usize,
    pub duplicate_count: usize,
    pub resource_count: usize,
    pub resource_bytes: u64,
}

/// Each scratch directory is created exclusively and only its owner removes it.
struct Scratch(PathBuf);
impl Scratch {
    fn at(parent: &Path) -> Result<Self, String> {
        fs::create_dir_all(parent).map_err(err)?;
        let path = parent.join(format!("clipboard-backup-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).map_err(err)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn invalid(error: String) -> StorageError {
    StorageError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Hash exactly the bytes read/written, with a hard expanded-size ceiling.
fn transfer(
    mut reader: impl Read,
    mut writer: impl Write,
    limit: u64,
    progress: Progress<'_>,
) -> Result<(u64, String), String> {
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        progress(OperationPhase::Transferring, size, None)?;
        let count = reader.read(&mut buffer).map_err(err)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > limit {
            return Err("backup entry exceeds its size limit".into());
        }
        writer.write_all(&buffer[..count]).map_err(err)?;
        hash.update(&buffer[..count]);
    }
    Ok((size, hex::encode(hash.finalize())))
}

/// Rewrite only known resource fields. Foreign paths are never retained during restore.
fn remap_item(
    item: &mut ClipboardItem,
    map: &mut impl FnMut(&str) -> Result<String, String>,
) -> Result<(), String> {
    if matches!(item.kind, ClipboardKind::Image | ClipboardKind::File)
        && item.resource_path.as_deref().is_none_or(str::is_empty)
    {
        return Err(format!("media record {} has no resource", item.id));
    }
    if let Some(path) = item.resource_path.as_mut() {
        *path = map(path)?;
    }
    item.preview_path = None;
    item.icon_path = None;
    if let Some(raw) = &item.metadata_json {
        let mut value: serde_json::Value = serde_json::from_str(raw).map_err(err)?;
        if let Some(object) = value.as_object_mut() {
            for key in ["resourcePath", "storagePath"] {
                if let Some(path) = object
                    .get(key)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    let mapped = map(path)?;
                    object.insert(key.into(), mapped.into());
                }
            }
            object.remove("previewPath");
            object.remove("iconPath");
            if object.contains_key("originalPath") {
                object.insert("originalPath".into(), item.resource_path.clone().into());
            }
            if let Some(files) = object.get_mut("files").and_then(|v| v.as_array_mut()) {
                for file in files {
                    let source = file["storagePath"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| file["originalPath"].as_str())
                        .ok_or("file entry has no resource path")?;
                    let path = map(source)?;
                    file["storagePath"] = path.clone().into();
                    file["originalPath"] = path.into();
                    file["copied"] = true.into();
                }
            }
        }
        item.metadata_json = Some(value.to_string());
    }
    if item.kind == ClipboardKind::File {
        if let Some(text) = &item.text_content {
            let paths: Vec<String> = serde_json::from_str(text).map_err(err)?;
            item.text_content = Some(
                serde_json::to_string(
                    &paths
                        .iter()
                        .map(|path| map(path))
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .map_err(err)?,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn create(database: &Database, target: &Path) -> Result<u64, String> {
    create_with_progress(database, target, &mut |_, _, _| Ok(()))
}
pub fn create_with_progress(
    database: &Database,
    target: &Path,
    progress: Progress<'_>,
) -> Result<u64, String> {
    progress(OperationPhase::Creating, 0, None)?;
    let scratch = Scratch::at(&std::env::temp_dir())?;
    atomic_output(target, |output| {
        let mut zip = ZipWriter::new(output);
        let mut records = File::create(scratch.0.join("records.jsonl")).map_err(err)?;
        let mut paths: HashMap<String, String> = HashMap::new();
        let mut manifest = Manifest {
            version: 1,
            item_count: 0,
            entries: Vec::new(),
        };
        let mut total = 0;
        let mut record_bytes = 0;
        database
            .visit_active_items(|mut item| {
                progress(OperationPhase::Creating, manifest.item_count as u64, None)
                    .map_err(invalid)?;
                Database::validate_restore_items(std::slice::from_ref(&item))?;
                remap_item(&mut item, &mut |path| {
                    if let Some(name) = paths.get(path) {
                        return Ok(name.clone());
                    }
                    let source = File::open(path)
                        .map_err(|e| format!("missing or unreadable resource {path}: {e}"))?;
                    if !source.metadata().map_err(err)?.is_file() {
                        return Err("resource is not a regular file".into());
                    }
                    // Names contain no source path and only a safe extension.
                    let ext = Path::new(path)
                        .extension()
                        .and_then(|x| x.to_str())
                        .unwrap_or("bin")
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .take(16)
                        .collect::<String>();
                    let name = format!(
                        "resources/{}.{}",
                        manifest.entries.len(),
                        if ext.is_empty() { "bin" } else { &ext }
                    );
                    zip.start_file(
                        &name,
                        SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Stored),
                    )
                    .map_err(err)?;
                    let (bytes, sha256) = transfer(source, &mut zip, MAX_RESOURCE, progress)?;
                    total += bytes;
                    if total > MAX_TOTAL {
                        return Err("backup resources exceed 16 GiB".into());
                    }
                    manifest.entries.push(Entry {
                        name: name.clone(),
                        bytes,
                        sha256,
                    });
                    paths.insert(path.into(), name.clone());
                    Ok(name)
                })
                .map_err(invalid)?;
                let line = serde_json::to_vec(&item)?;
                record_bytes += line.len() as u64 + 1;
                if line.len() as u64 >= MAX_LINE
                    || record_bytes > MAX_RECORDS
                    || manifest.item_count >= MAX_ITEMS
                {
                    return Err(invalid("backup record limit exceeded".into()));
                }
                records.write_all(&line)?;
                records.write_all(b"\n")?;
                manifest.item_count += 1;
                Ok(())
            })
            .map_err(err)?;
        drop(records);
        zip.start_file("records.jsonl", SimpleFileOptions::default())
            .map_err(err)?;
        let (bytes, sha256) = transfer(
            File::open(scratch.0.join("records.jsonl")).map_err(err)?,
            &mut zip,
            MAX_RECORDS,
            progress,
        )?;
        manifest.entries.push(Entry {
            name: "records.jsonl".into(),
            bytes,
            sha256,
        });
        let encoded = serde_json::to_vec(&manifest).map_err(err)?;
        if encoded.len() as u64 > MAX_MANIFEST {
            return Err("backup manifest exceeds 16 MiB".into());
        }
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .map_err(err)?;
        zip.write_all(&encoded).map_err(err)?;
        zip.finish().map_err(err)?;
        progress(
            OperationPhase::Creating,
            manifest.item_count as u64,
            Some(manifest.item_count as u64),
        )?;
        Ok(())
    })
}

fn safe_entry(name: &str) -> bool {
    if name == "records.jsonl" {
        return true;
    }
    name.strip_prefix("resources/").is_some_and(|file| {
        !file.is_empty()
            && !file.starts_with('.')
            && file.len() < 100
            && file.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.')
    })
}

struct Validated {
    scratch: Scratch,
    items: Vec<ClipboardItem>,
    resources: HashMap<String, PathBuf>,
    preview: BackupPreview,
}
enum ValidationMode {
    Preview,
    Restore,
}
fn validate(
    path: &Path,
    database: &Database,
    mode: ValidationMode,
    progress: Progress<'_>,
) -> Result<Validated, String> {
    progress(OperationPhase::Validating, 0, None)?;
    let scratch = Scratch::at(&std::env::temp_dir())?;
    let mut file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(scratch.0.join("archive.zip"))
        .map_err(err)?;
    let (_, fingerprint) = transfer(
        File::open(path).map_err(err)?,
        &mut file,
        MAX_TOTAL + MAX_RECORDS + MAX_MANIFEST * 2,
        progress,
    )?;
    file.seek(SeekFrom::Start(0)).map_err(err)?;
    let mut zip = ZipArchive::new(file).map_err(err)?;
    if zip.len() > MAX_ITEMS * 10 {
        return Err("too many archive entries".into());
    }
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        progress(
            OperationPhase::Validating,
            index as u64,
            Some(zip.len() as u64),
        )?;
        let entry = zip.by_index(index).map_err(err)?;
        if !names.insert(entry.name().to_owned())
            || (entry.name() != "manifest.json" && !safe_entry(entry.name()))
            || entry.is_dir()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("unsafe or duplicate archive entry".into());
        }
    }
    let mut manifest_bytes = Vec::new();
    transfer(
        zip.by_name("manifest.json").map_err(err)?,
        &mut manifest_bytes,
        MAX_MANIFEST,
        progress,
    )?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(err)?;
    if manifest.version != 1
        || manifest.item_count > MAX_ITEMS
        || manifest.entries.len() + 1 != names.len()
    {
        return Err("unsupported or inconsistent backup manifest".into());
    }
    let mut resources = HashMap::new();
    let mut total = 0;
    let mut seen = HashSet::new();
    for entry in &manifest.entries {
        if !safe_entry(&entry.name) || !seen.insert(entry.name.clone()) {
            return Err("invalid manifest entry".into());
        }
        let target = scratch.0.join(format!("entry-{}", seen.len()));
        let mut output = File::create(&target).map_err(err)?;
        let limit = if entry.name == "records.jsonl" {
            MAX_RECORDS
        } else {
            MAX_RESOURCE
        };
        if entry.bytes > limit {
            return Err("manifest entry exceeds size limit".into());
        }
        let (bytes, hash) = transfer(
            zip.by_name(&entry.name).map_err(err)?,
            &mut output,
            entry.bytes,
            progress,
        )?;
        if hash != entry.sha256 || bytes != entry.bytes {
            return Err(format!("checksum mismatch: {}", entry.name));
        }
        total += bytes;
        if total > MAX_TOTAL + MAX_RECORDS {
            return Err("backup exceeds total size limit".into());
        }
        resources.insert(entry.name.clone(), target);
    }
    let record_file = resources
        .remove("records.jsonl")
        .ok_or("missing records.jsonl")?;
    let mut reader = BufReader::new(File::open(record_file).map_err(err)?);
    let mut items = Vec::new();
    let mut ids = HashSet::new();
    let mut hashes = std::collections::BTreeSet::new();
    let mut duplicates = 0;
    let mut item_count = 0usize;
    loop {
        progress(
            OperationPhase::Validating,
            item_count as u64,
            Some(manifest.item_count as u64),
        )?;
        let mut line = Vec::new();
        let bytes = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
            .map_err(err)?;
        if bytes == 0 {
            break;
        }
        if bytes as u64 > MAX_LINE {
            return Err("record exceeds size limit".into());
        }
        let mut item: ClipboardItem = serde_json::from_slice(&line).map_err(err)?;
        Database::validate_restore_items(std::slice::from_ref(&item)).map_err(err)?;
        if !ids.insert(item.id.clone()) || !hashes.insert((item.kind, item.content_hash.clone())) {
            return Err("duplicate record identity in backup".into());
        }
        // Validate every path reference even when this record is already present locally.
        remap_item(&mut item, &mut |name| {
            resources
                .contains_key(name)
                .then(|| name.to_owned())
                .ok_or_else(|| format!("resource missing from manifest: {name}"))
        })?;
        if database
            .content_exists(item.kind, &item.content_hash)
            .map_err(err)?
        {
            duplicates += 1;
        } else if database.get_item(&item.id).map_err(err)?.is_some() {
            return Err(format!(
                "record id conflicts with local content: {}",
                item.id
            ));
        }
        item_count += 1;
        if item_count > MAX_ITEMS {
            return Err("too many records".into());
        }
        // A preview needs identities/counts, never the complete clipboard bodies.
        if matches!(mode, ValidationMode::Restore) {
            items.push(item);
        }
    }
    if item_count != manifest.item_count {
        return Err("record count does not match manifest".into());
    }
    let resource_bytes = manifest
        .entries
        .iter()
        .filter(|e| e.name != "records.jsonl")
        .map(|e| e.bytes)
        .sum();
    Ok(Validated {
        scratch,
        items,
        preview: BackupPreview {
            fingerprint,
            item_count: manifest.item_count,
            duplicate_count: duplicates,
            resource_count: resources.len(),
            resource_bytes,
        },
        resources,
    })
}

#[cfg(test)]
pub fn preview(path: &Path, database: &Database) -> Result<BackupPreview, String> {
    preview_with_progress(path, database, &mut |_, _, _| Ok(()))
}
pub fn preview_with_progress(
    path: &Path,
    database: &Database,
    progress: Progress<'_>,
) -> Result<BackupPreview, String> {
    Ok(validate(path, database, ValidationMode::Preview, progress)?.preview)
}

#[cfg(test)]
pub fn restore(
    path: &Path,
    fingerprint: &str,
    database: &Database,
    paths: &StoragePaths,
) -> Result<ImportSummary, String> {
    restore_with_progress(path, fingerprint, database, paths, &mut |_, _, _| Ok(()))
}
pub fn restore_with_progress(
    path: &Path,
    fingerprint: &str,
    database: &Database,
    paths: &StoragePaths,
    progress: Progress<'_>,
) -> Result<ImportSummary, String> {
    let _resource_publication = database.begin_resource_write();
    let mut validated = validate(path, database, ValidationMode::Restore, progress)?;
    if validated.preview.fingerprint != fingerprint {
        return Err("backup changed since preview; preview it again".into());
    }
    struct Published {
        paths: Vec<PathBuf>,
        committed: bool,
    }
    impl Drop for Published {
        fn drop(&mut self) {
            if !self.committed {
                for path in &self.paths {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    let mut published = Published {
        paths: Vec::new(),
        committed: false,
    };
    let prefix = uuid::Uuid::new_v4();
    let mut local_paths = HashMap::<(bool, String), String>::new();
    for item in &mut validated.items {
        // Sync validates resources against their role's root. One archive entry
        // may serve both an image and a file record, so cache per role as well
        // as entry name instead of making either role reference the wrong root.
        let image = item.kind == ClipboardKind::Image;
        let root = if image { &paths.images } else { &paths.files };
        remap_item(item, &mut |name| {
            let key = (image, name.to_owned());
            if let Some(path) = local_paths.get(&key) {
                return Ok(path.clone());
            }
            let source = validated
                .resources
                .get(name)
                .ok_or("missing restored resource")?;
            progress(OperationPhase::Restoring, local_paths.len() as u64, None)?;
            let target = root.join(format!(
                "backup-{prefix}-{}",
                name.strip_prefix("resources/").ok_or("invalid resource")?
            ));
            atomic_output(&target, |out| {
                let input = File::open(source).map_err(err)?;
                transfer(input, out, MAX_RESOURCE, progress)?;
                Ok(())
            })?;
            published.paths.push(target.clone());
            let path = target.to_string_lossy().into_owned();
            local_paths.insert(key, path.clone());
            Ok(path)
        })?;
    }
    let result = database
        .restore_items_transactional_with_progress(&validated.items, |completed, total| {
            progress(OperationPhase::Applying, completed, Some(total)).map_err(invalid)
        })
        .map_err(err)?;
    // Flat managed paths participate in existing orphan cleanup. Any unused duplicate resources
    // are eligible after its grace period; a failed DB transaction removes all newly published files.
    published.committed = result.imported_count > 0;
    drop(validated.scratch);
    Ok(ImportSummary {
        imported_count: result.imported_count,
        skipped_count: result.skipped_count,
        errors: result.errors,
        pending_truncation: 0,
        max_items: 0,
    })
}

#[cfg(test)]
mod scale_bench;

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_no_published_resources(paths: &StoragePaths) {
        for root in [&paths.images, &paths.files] {
            assert!(!fs::read_dir(root).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("backup-")));
        }
    }
    pub(super) fn item(id: &str, kind: ClipboardKind, path: Option<String>) -> ClipboardItem {
        ClipboardItem {
            id: id.into(),
            kind,
            title: format!("{id} 中文"),
            text_content: (kind == ClipboardKind::Text).then(|| "original\r\ntext".into()),
            html_content: Some("<b>original</b>".into()),
            rtf_content: Some("{\\rtf1 original}".into()),
            resource_path: path,
            preview_path: Some("discarded-preview".into()),
            content_hash: id.into(),
            source_app: Some("Editor".into()),
            icon_path: Some("discarded-icon".into()),
            size_bytes: 8,
            created_at_ms: 1000,
            last_used_at_ms: Some(2000),
            is_favorite: true,
            metadata_json: Some(r#"{"tags":["manual"],"width":2}"#.into()),
        }
    }
    fn rewrite(path: &Path, alter: impl Fn(&str, &mut Vec<u8>)) {
        let mut original = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut entries = Vec::new();
        for i in 0..original.len() {
            let mut entry = original.by_index(i).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            alter(&name, &mut bytes);
            entries.push((name, bytes));
        }
        drop(original);
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn portable_backup_restores_media_rich_text_tags_and_skips_duplicates() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let source = Database::open_in_memory().unwrap();
        let binary = root.0.join("image.png");
        fs::write(&binary, b"synthetic image bytes").unwrap();
        let image = item(
            "image",
            ClipboardKind::Image,
            Some(binary.to_string_lossy().into()),
        );
        let text = item("text", ClipboardKind::Text, None);
        let mut files = item("files", ClipboardKind::File, image.resource_path.clone());
        files.text_content = Some(serde_json::json!([binary]).to_string());
        files.metadata_json = Some(serde_json::json!({"tags":["manual"], "files":[{"name":"photo.png", "storagePath":binary, "originalPath":"Z:/foreign/original.png"}]}).to_string());
        for entry in [&image, &text, &files] {
            source.save_item(entry).unwrap();
        }
        let archive = root.0.join("bundle.clipbackup");
        create(&source, &archive).unwrap();
        fs::remove_file(binary).unwrap(); // Restoring cannot depend on the old machine.
        let target = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize(root.0.join("target")).unwrap();
        let checked = preview(&archive, &target).unwrap();
        assert_eq!((checked.item_count, checked.resource_count), (3, 1));
        assert_eq!(target.item_count().unwrap(), 0);
        assert_eq!(
            restore(&archive, &checked.fingerprint, &target, &paths)
                .unwrap()
                .imported_count,
            3
        );
        let restored = target.get_item("image").unwrap().unwrap();
        let path = Path::new(restored.resource_path.as_ref().unwrap());
        assert!(path.starts_with(&paths.images));
        assert_eq!(fs::read(path).unwrap(), b"synthetic image bytes");
        assert_eq!(restored.preview_path, None);
        let restored = target.get_item("text").unwrap().unwrap();
        assert_eq!(restored.text_content, text.text_content);
        assert_eq!(restored.html_content, text.html_content);
        assert_eq!(restored.rtf_content, text.rtf_content);
        assert_eq!(restored.metadata_json, text.metadata_json);
        let restored = target.get_item("files").unwrap().unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(restored.metadata_json.as_ref().unwrap()).unwrap();
        assert_eq!(
            metadata["files"][0]["storagePath"],
            metadata["files"][0]["originalPath"]
        );
        let checked = preview(&archive, &target).unwrap();
        assert_eq!(checked.duplicate_count, 3);
        assert_eq!(
            restore(&archive, &checked.fingerprint, &target, &paths)
                .unwrap()
                .skipped_count,
            3
        );
    }
    #[test]
    fn corrupt_or_changed_bundles_and_traversal_are_rejected_without_writes() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("text", ClipboardKind::Text, None))
            .unwrap();
        let archive = root.0.join("bundle.clipbackup");
        create(&database, &archive).unwrap();
        let checked = preview(&archive, &database).unwrap();
        let paths = StoragePaths::initialize(root.0.join("target")).unwrap();
        assert!(restore(&archive, "old fingerprint", &database, &paths)
            .unwrap_err()
            .contains("changed"));
        rewrite(&archive, |name, bytes| {
            if name == "records.jsonl" {
                bytes[3] ^= 1;
            }
        });
        assert!(restore(&archive, &checked.fingerprint, &database, &paths)
            .unwrap_err()
            .contains("checksum"));
        let mut zip = ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file("../escape", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();
        assert!(preview(&archive, &database).unwrap_err().contains("unsafe"));
        assert_eq!(database.item_count().unwrap(), 1);
        assert!(!root.0.join("escape").exists());
    }

    #[test]
    fn preview_and_restore_validate_the_final_record_even_when_already_present() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let database = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize(root.0.join("target")).unwrap();
        let archive = root.0.join("bundle.clipbackup");
        let first = item("first", ClipboardKind::Text, None);
        let mut last = item("last", ClipboardKind::Text, None);
        database.save_item(&first).unwrap();
        database.save_item(&last).unwrap();
        // Valid checksum and count, but the final duplicate row is invalid.
        // Both modes must still reject it before any publication or database change.
        last.created_at_ms = -1;
        let records = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&last).unwrap()
        );
        let manifest = Manifest {
            version: 1,
            item_count: 2,
            entries: vec![Entry {
                name: "records.jsonl".into(),
                bytes: records.len() as u64,
                sha256: hex::encode(Sha256::digest(records.as_bytes())),
            }],
        };
        let mut zip = ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file("records.jsonl", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(records.as_bytes()).unwrap();
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        zip.finish().unwrap();
        assert!(preview(&archive, &database)
            .unwrap_err()
            .contains("negative"));
        assert!(restore(&archive, "unused", &database, &paths)
            .unwrap_err()
            .contains("negative"));
        assert_eq!(database.item_count().unwrap(), 2);
    }

    #[test]
    fn preview_cancellation_at_eof_keeps_counts_and_database_intact() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let database = Database::open_in_memory().unwrap();
        database
            .save_item(&item("text", ClipboardKind::Text, None))
            .unwrap();
        let archive = root.0.join("bundle.clipbackup");
        create(&database, &archive).unwrap();
        let result = preview_with_progress(&archive, &database, &mut |phase, completed, total| {
            if matches!(phase, OperationPhase::Validating) && total == Some(1) && completed == 1 {
                return Err("operation cancelled".into());
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("cancelled"));
        let result = preview(&archive, &database).unwrap();
        assert_eq!((result.item_count, result.duplicate_count), (1, 1));
        assert_eq!(database.item_count().unwrap(), 1);
    }
    #[test]
    fn failed_restore_rolls_back_rows_and_published_files() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let source = Database::open_in_memory().unwrap();
        let binary = root.0.join("asset.png");
        fs::write(&binary, b"asset").unwrap();
        source
            .save_item(&item(
                "a",
                ClipboardKind::Image,
                Some(binary.to_string_lossy().into()),
            ))
            .unwrap();
        source
            .save_item(&item("b", ClipboardKind::Text, None))
            .unwrap();
        let archive = root.0.join("bundle.clipbackup");
        create(&source, &archive).unwrap();
        let target = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize(root.0.join("target")).unwrap();
        let checked = preview(&archive, &target).unwrap();
        target.with_connection(|c| { c.execute_batch("CREATE TRIGGER reject_a BEFORE INSERT ON clipboard_items WHEN NEW.id = 'a' BEGIN SELECT RAISE(ABORT, 'injected'); END;")?; Ok(()) }).unwrap();
        assert!(restore(&archive, &checked.fingerprint, &target, &paths).is_err());
        assert_eq!(target.item_count().unwrap(), 0);
        assert_no_published_resources(&paths);
    }

    #[test]
    fn cancellation_during_resource_copy_preserves_existing_backup() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let database = Database::open_in_memory().unwrap();
        let binary = root.0.join("asset.png");
        fs::write(&binary, vec![0u8; 128 * 1024]).unwrap();
        database
            .save_item(&item(
                "image",
                ClipboardKind::Image,
                Some(binary.to_string_lossy().into()),
            ))
            .unwrap();
        let archive = root.0.join("bundle.clipbackup");
        fs::write(&archive, b"previous backup").unwrap();
        let error = create_with_progress(&database, &archive, &mut |phase, completed, _| {
            if matches!(phase, OperationPhase::Transferring) && completed >= 64 * 1024 {
                return Err("operation cancelled".into());
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("cancelled"));
        assert_eq!(fs::read(archive).unwrap(), b"previous backup");
    }

    #[test]
    fn cancellation_before_restore_commit_removes_rows_and_published_resources() {
        let root = Scratch::at(&std::env::temp_dir()).unwrap();
        let source = Database::open_in_memory().unwrap();
        let binary = root.0.join("asset.png");
        fs::write(&binary, b"synthetic bytes").unwrap();
        source
            .save_item(&item(
                "image",
                ClipboardKind::Image,
                Some(binary.to_string_lossy().into()),
            ))
            .unwrap();
        source
            .save_item(&item("text", ClipboardKind::Text, None))
            .unwrap();
        let archive = root.0.join("bundle.clipbackup");
        create(&source, &archive).unwrap();
        let target = Database::open_in_memory().unwrap();
        let paths = StoragePaths::initialize(root.0.join("target")).unwrap();
        let checked = preview(&archive, &target).unwrap();
        let error = restore_with_progress(
            &archive,
            &checked.fingerprint,
            &target,
            &paths,
            &mut |phase, completed, total| {
                if matches!(phase, OperationPhase::Applying) && Some(completed) == total {
                    return Err("operation cancelled".into());
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("cancelled"));
        assert_eq!(target.item_count().unwrap(), 0);
        assert_no_published_resources(&paths);
        assert_eq!(
            restore(&archive, &checked.fingerprint, &target, &paths)
                .unwrap()
                .imported_count,
            2
        );
    }
}
