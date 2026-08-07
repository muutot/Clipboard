use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

use crate::storage::{Database, StorageFileReferences, StoragePaths};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_version: u32,
    pub backup_type: String,
    pub created_at_ms: i64,
    pub base_sync_ms: Option<i64>,
    pub device_id: String,
    pub app_version: String,
    pub item_count: usize,
    pub resource_count: usize,
    pub total_resource_bytes: u64,
    pub resources: Vec<ResourceEntry>,
    pub oplog_entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceEntry {
    pub path: String,
    pub hash: String,
    pub size_bytes: u64,
}

const SUPPORTED_FORMAT_VERSION: u32 = 2;

/// Creates a full backup package containing the SQLite database and all referenced resource files.
pub fn create_backup(
    paths: &StoragePaths,
    database: &Database,
    output_path: &Path,
) -> Result<BackupManifest, String> {
    database.checkpoint().map_err(|e| e.to_string())?;

    let references = database
        .list_storage_file_references()
        .map_err(|e| e.to_string())?;

    let referenced = resolve_all_references(paths, &references);
    let (resources, total_bytes) = collect_all_resources(paths, &referenced)?;

    let item_count = database
        .count_active_items()
        .map_err(|e| format!("failed to count items: {e}"))?;

    let now_ms = now_ms();

    let manifest = BackupManifest {
        format_version: SUPPORTED_FORMAT_VERSION,
        backup_type: "full".to_string(),
        created_at_ms: now_ms,
        base_sync_ms: None,
        device_id: get_device_id(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        item_count,
        resource_count: resources.len(),
        total_resource_bytes: total_bytes,
        resources,
        oplog_entries: 0,
    };

    write_backup_archive(paths, &manifest, output_path, false)?;

    Ok(manifest)
}

/// Creates a baseline backup containing ONLY the database (no resource files).
/// This is the "anchor" for a new oplog chain. It's much smaller than a full
/// backup because it doesn't include any images/files — those come through
/// oplog entries as they're created.
pub fn create_baseline_backup(
    database: &Database,
    output_path: &Path,
) -> Result<BackupManifest, String> {
    database.checkpoint().map_err(|e| e.to_string())?;

    let items = database
        .export_active_items()
        .map_err(|e| format!("failed to export items: {e}"))?;

    let now_ms = now_ms();

    let manifest = BackupManifest {
        format_version: SUPPORTED_FORMAT_VERSION,
        backup_type: "baseline".to_string(),
        created_at_ms: now_ms,
        base_sync_ms: None,
        device_id: get_device_id(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        item_count: items.len(),
        resource_count: 0,
        total_resource_bytes: 0,
        resources: Vec::new(),
        oplog_entries: 0,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(output_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);

    let opts: SimpleFileOptions = SimpleFileOptions::default();
    zip.start_file("manifest.json", opts)
        .map_err(|e| e.to_string())?;
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    zip.write_all(&manifest_json).map_err(|e| e.to_string())?;

    let opts: SimpleFileOptions = SimpleFileOptions::default();
    zip.start_file("baseline.bin", opts)
        .map_err(|e| e.to_string())?;
    let items_bin = bincode::encode_to_vec(&items, bincode::config::standard())
        .map_err(|e| format!("failed to serialize baseline: {e}"))?;
    use std::io::Write;
    zip.write_all(&items_bin).map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;

    Ok(manifest)
}

/// Creates an incremental (oplog) backup containing only operations since last sync.
/// This is the preferred method for subsequent syncs — it produces tiny packages
/// with just the changelog entries and any new resource files.
pub fn create_oplog_backup(
    paths: &StoragePaths,
    database: &Database,
    output_path: &Path,
    batch_size: usize,
) -> Result<BackupManifest, String> {
    database.checkpoint().map_err(|e| e.to_string())?;

    let entries = database
        .get_unsynced_changelog(batch_size)
        .map_err(|e| format!("failed to read changelog: {e}"))?;

    if entries.is_empty() {
        return Ok(BackupManifest {
            format_version: SUPPORTED_FORMAT_VERSION,
            backup_type: "noop".to_string(),
            created_at_ms: now_ms(),
            base_sync_ms: None,
            device_id: get_device_id(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            item_count: 0,
            resource_count: 0,
            total_resource_bytes: 0,
            resources: Vec::new(),
            oplog_entries: 0,
        });
    }

    let _max_sequence = entries.last().unwrap().sequence;

    let item_ids: Vec<String> = entries.iter().map(|e| e.item_id.clone()).collect();
    let new_refs = database
        .list_storage_file_references_for_items(&item_ids)
        .map_err(|e| e.to_string())?;

    let referenced = resolve_all_references(paths, &new_refs);
    let (resources, total_bytes) = collect_all_resources(paths, &referenced)?;

    let now_ms = now_ms();

    let oplog_bin = bincode::encode_to_vec(&entries, bincode::config::standard())
        .map_err(|e| format!("failed to serialize oplog: {e}"))?;

    let manifest = BackupManifest {
        format_version: SUPPORTED_FORMAT_VERSION,
        backup_type: "oplog".to_string(),
        created_at_ms: now_ms,
        base_sync_ms: None,
        device_id: get_device_id(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        item_count: entries.len(),
        resource_count: resources.len(),
        total_resource_bytes: total_bytes,
        resources,
        oplog_entries: entries.len() as u64,
    };

    write_backup_archive_with_oplog(paths, &manifest, &oplog_bin, output_path)?;

    Ok(manifest)
}

/// Marks the changelog entries as synced after successful upload.
pub fn mark_oplog_synced(database: &Database, max_sequence: i64) -> Result<u64, String> {
    database
        .mark_changelog_synced(max_sequence)
        .map_err(|e| e.to_string())
}

/// Returns the number of unsynced operations waiting for sync.
pub fn count_unsynced(database: &Database) -> Result<u64, String> {
    database
        .count_unsynced_changelog()
        .map_err(|e| e.to_string())
}

/// Purges old synced entries to keep the table small.
pub fn purge_oplog(database: &Database, keep_recent: i64) -> Result<u64, String> {
    database
        .purge_synced_changelog(keep_recent)
        .map_err(|e| e.to_string())
}

fn write_backup_archive(
    paths: &StoragePaths,
    manifest: &BackupManifest,
    output_path: &Path,
    include_db: bool,
) -> Result<(), String> {
    write_backup_archive_internal(paths, manifest, None, output_path, include_db)
}

fn write_backup_archive_with_oplog(
    paths: &StoragePaths,
    manifest: &BackupManifest,
    oplog_data: &[u8],
    output_path: &Path,
) -> Result<(), String> {
    write_backup_archive_internal(paths, manifest, Some(oplog_data), output_path, false)
}

fn write_backup_archive_internal(
    paths: &StoragePaths,
    manifest: &BackupManifest,
    oplog_data: Option<&[u8]>,
    output_path: &Path,
    include_db: bool,
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let file = File::create(output_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);

    let opts: SimpleFileOptions = SimpleFileOptions::default();
    zip.start_file("manifest.json", opts)
        .map_err(|e| e.to_string())?;
    let manifest_json = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    zip.write_all(&manifest_json).map_err(|e| e.to_string())?;

    if include_db {
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("database/clipboard.sqlite3", opts)
            .map_err(|e| e.to_string())?;
        let db_data = fs::read(&paths.database).map_err(|e| e.to_string())?;
        zip.write_all(&db_data).map_err(|e| e.to_string())?;
    }

    if let Some(oplog) = oplog_data {
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("oplog.bin", opts)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        zip.write_all(oplog).map_err(|e| e.to_string())?;
    }

    for entry in &manifest.resources {
        let abs_path = paths.storage.join(&entry.path);
        if !abs_path.exists() {
            continue;
        }
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file(format!("resources/{}", entry.path), opts)
            .map_err(|e| e.to_string())?;
        let data = fs::read(&abs_path).map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn collect_all_resources(
    paths: &StoragePaths,
    referenced: &HashSet<PathBuf>,
) -> Result<(Vec<ResourceEntry>, u64), String> {
    let mut resources = Vec::new();
    let mut total_bytes = 0u64;

    for (dir, cleanup_enabled) in [
        (&paths.images, paths.image_cleanup_enabled),
        (&paths.previews, paths.image_cleanup_enabled),
        (&paths.files, paths.file_cleanup_enabled),
    ] {
        if !cleanup_enabled || !dir.exists() {
            continue;
        }
        collect_dir_resources(
            dir,
            &paths.storage,
            referenced,
            &mut resources,
            &mut total_bytes,
        )?;
    }

    let icons_dir = paths.storage.join("icons");
    if icons_dir.exists() {
        collect_dir_resources(
            &icons_dir,
            &paths.storage,
            referenced,
            &mut resources,
            &mut total_bytes,
        )?;
    }

    Ok((resources, total_bytes))
}

fn collect_dir_resources(
    dir: &Path,
    base: &Path,
    referenced: &HashSet<PathBuf>,
    resources: &mut Vec<ResourceEntry>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !referenced.contains(&normalized) {
            continue;
        }
        let relative = path.strip_prefix(base).unwrap_or(path);
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let hash = compute_file_hash(path)?;

        resources.push(ResourceEntry {
            path: relative.to_string_lossy().replace('\\', "/"),
            hash,
            size_bytes: size,
        });
        *total_bytes += size;
    }
    Ok(())
}

fn compute_file_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize().as_slice()))
}

fn resolve_all_references(paths: &StoragePaths, refs: &StorageFileReferences) -> HashSet<PathBuf> {
    let icons = paths.storage.join("icons");
    let mut set = HashSet::new();
    let bases: [&Path; 4] = [&paths.storage, &paths.images, &paths.files, &icons];

    for path_str in &refs.resource_paths {
        insert_resolved(path_str, &bases, &mut set);
    }
    for path_str in &refs.preview_paths {
        insert_resolved(path_str, &bases, &mut set);
    }
    for path_str in &refs.icon_paths {
        insert_resolved(path_str, &[&paths.storage, &icons], &mut set);
    }
    set
}

fn insert_resolved(path_str: &str, bases: &[&Path], set: &mut HashSet<PathBuf>) {
    if path_str.trim().is_empty() {
        return;
    }
    let path = Path::new(path_str);
    if path.is_absolute() {
        set.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    } else {
        for base in bases {
            let joined = base.join(path);
            set.insert(joined.canonicalize().unwrap_or(joined));
        }
    }
}

fn get_device_id() -> String {
    if let Ok(hostname) = std::env::var("COMPUTERNAME") {
        return hostname.to_lowercase();
    }
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        return hostname.to_lowercase();
    }
    "unknown-device".to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn read_manifest_from_backup(backup_path: &Path) -> Result<BackupManifest, String> {
    let file = File::open(backup_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut manifest_file = archive
        .by_name("manifest.json")
        .map_err(|_| "backup missing manifest.json".to_string())?;
    let mut json = String::new();
    manifest_file
        .read_to_string(&mut json)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| format!("invalid manifest: {e}"))
}

/// Reads baseline items from a downloaded baseline backup zip.
/// Supports both bincode (.bin) and legacy JSON (.json) formats.
pub fn read_baseline_items(
    backup_path: &Path,
) -> Result<Vec<crate::domain::ClipboardItem>, String> {
    let file = File::open(backup_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // Try bincode format first
    if let Ok(mut items_file) = archive.by_name("baseline.bin") {
        let mut data = Vec::new();
        use std::io::Read;
        items_file
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if let Ok((items, _)) = bincode::decode_from_slice::<Vec<crate::domain::ClipboardItem>, _>(
            &data,
            bincode::config::standard(),
        ) {
            return Ok(items);
        }
    }

    // Fallback to JSON format
    let mut items_file = archive
        .by_name("baseline.json")
        .map_err(|_| "baseline missing baseline data".to_string())?;
    let mut json = String::new();
    items_file
        .read_to_string(&mut json)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| format!("invalid baseline: {e}"))
}

// Re-export for backward compatibility
pub use create_oplog_backup as create_incremental_backup;
