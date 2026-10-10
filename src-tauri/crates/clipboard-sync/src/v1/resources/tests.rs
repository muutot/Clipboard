//! Resource transfer tests, moved verbatim with the module.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write,
};

use super::*;
use crate::v1::{
    remote::{DownloadedFile, DownloadedObject, ObjectInfo},
    MutationBatch, RecordVersion, ReplicatedItem, SyncItem, SyncItemKind,
};

#[derive(Default)]
struct MemoryStore {
    objects: RefCell<BTreeMap<String, Vec<u8>>>,
    file_gets: Cell<u64>,
    file_puts: Cell<u64>,
}

impl ObjectStore for MemoryStore {
    fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
        Ok(self
            .objects
            .borrow()
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .filter(|(key, _)| start_after.is_none_or(|cursor| key.as_str() > cursor))
            .map(|(key, bytes)| ObjectInfo {
                key: key.clone(),
                size_bytes: Some(bytes.len() as u64),
                modified_ms: None,
                etag: Some(format!("\"{}\"", hex::encode(Sha256::digest(bytes)))),
            })
            .collect())
    }

    fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
        Ok(self
            .objects
            .borrow()
            .get(key)
            .cloned()
            .map(|bytes| DownloadedObject { bytes, etag: None }))
    }

    fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
        Ok(self.objects.borrow().get(key).map(|bytes| ObjectMetadata {
            size_bytes: Some(bytes.len() as u64),
            etag: None,
        }))
    }

    fn get_to_file(
        &self,
        key: &str,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<Option<DownloadedFile>, String> {
        let Some(bytes) = self.objects.borrow().get(key).cloned() else {
            return Ok(None);
        };
        if bytes.len() as u64 > max_bytes {
            return Err("memory resource exceeds download limit".to_string());
        }
        self.file_gets.set(self.file_gets.get() + 1);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        Ok(Some(DownloadedFile {
            size_bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            etag: None,
        }))
    }

    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let mut objects = self.objects.borrow_mut();
        if matches!(condition, PutCondition::IfAbsent) && objects.contains_key(key) {
            return Ok(PutOutcome::PreconditionFailed);
        }
        objects.insert(key.to_string(), bytes);
        Ok(PutOutcome::Stored { etag: None })
    }

    fn put_file(
        &self,
        key: &str,
        path: &Path,
        sha256: &str,
        size_bytes: u64,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        self.file_puts.set(self.file_puts.get() + 1);
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        if bytes.len() as u64 != size_bytes || hex::encode(Sha256::digest(&bytes)) != sha256 {
            return Err("memory resource fingerprint mismatch".to_string());
        }
        self.put(key, bytes, condition)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.objects.borrow_mut().remove(key);
        Ok(())
    }
}

fn temporary_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "clipboard-v1-resource-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn sample_item(id: &str, kind: SyncItemKind) -> ReplicatedItem {
    ReplicatedItem {
        item: SyncItem {
            id: id.to_string(),
            kind,
            title: id.to_string(),
            text_content: None,
            html_content: None,
            rtf_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: None,
            icon_path: None,
            size_bytes: 0,
            created_at_ms: 1,
            last_used_at_ms: None,
            is_favorite: false,
            metadata_json: None,
        },
        version: RecordVersion {
            modified_at_ms: 1,
            writer_device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        },
    }
}

#[test]
fn fingerprint_and_upload_stream_only_one_new_resource() {
    let root = temporary_directory("upload");
    let managed = root.join("managed");
    fs::create_dir_all(&managed).unwrap();
    let source = managed.join("payload.DATA");
    let bytes = vec![0x4d; 2 * 1024 * 1024 + 3];
    fs::write(&source, &bytes).unwrap();

    let resource = fingerprint_resource(
        &managed,
        &source,
        ResourceCategory::File,
        3 * 1024 * 1024,
        None,
    )
    .unwrap();
    assert_eq!(resource.extension, "data");
    assert_eq!(resource.size_bytes, bytes.len() as u64);

    let store = MemoryStore::default();
    assert!(
        ensure_resource_uploaded(&store, &resource, None)
            .unwrap()
            .uploaded
    );
    assert!(
        !ensure_resource_uploaded(&store, &resource, None)
            .unwrap()
            .uploaded
    );
    assert_eq!(store.file_puts.get(), 1);
    assert_eq!(store.objects.borrow().len(), 1);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_resources_are_deterministic_private_and_password_scoped() {
    let root = temporary_directory("encrypted-upload");
    let managed = root.join("managed");
    fs::create_dir_all(&managed).unwrap();
    let source = managed.join("private.bin");
    let plaintext = b"clipboard secret body".repeat(96 * 1024);
    fs::write(&source, &plaintext).unwrap();
    let first_key = SessionKey::derive("password", "scope-a").unwrap();
    let same_key = SessionKey::derive("password", "scope-a").unwrap();
    let other_key = SessionKey::derive("different", "scope-a").unwrap();

    let first = fingerprint_resource(
        &managed,
        &source,
        ResourceCategory::File,
        plaintext.len() as u64,
        Some(&first_key),
    )
    .unwrap();
    let same = fingerprint_resource(
        &managed,
        &source,
        ResourceCategory::File,
        plaintext.len() as u64,
        Some(&same_key),
    )
    .unwrap();
    let other = fingerprint_resource(
        &managed,
        &source,
        ResourceCategory::File,
        plaintext.len() as u64,
        Some(&other_key),
    )
    .unwrap();
    assert_eq!(first.object_key, same.object_key);
    assert_ne!(first.object_key, other.object_key);

    let store = MemoryStore::default();
    let uploaded = ensure_resource_uploaded(&store, &first, Some(&first_key)).unwrap();
    assert!(uploaded.uploaded);
    let stored = store
        .objects
        .borrow()
        .get(&first.object_key)
        .cloned()
        .unwrap();
    assert_eq!(stored.len() as u64, uploaded.size_bytes);
    assert_ne!(stored, plaintext);
    assert!(!stored
        .windows(b"clipboard secret body".len())
        .any(|window| window == b"clipboard secret body"));

    let retry_store = MemoryStore::default();
    ensure_resource_uploaded(&retry_store, &same, Some(&same_key)).unwrap();
    assert_eq!(
        retry_store.objects.borrow().get(&same.object_key),
        Some(&stored)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_resource_round_trip_rejects_wrong_password_and_corruption() {
    let root = temporary_directory("encrypted-round-trip");
    let managed = root.join("managed");
    let cache = root.join("cache");
    let wrong_cache = root.join("wrong-cache");
    let corrupt_cache = root.join("corrupt-cache");
    fs::create_dir_all(&managed).unwrap();
    let source = managed.join("private.bin");
    let plaintext = b"authenticated remote resource".repeat(80 * 1024);
    fs::write(&source, &plaintext).unwrap();
    let key = SessionKey::derive("password", "scope-a").unwrap();
    let wrong = SessionKey::derive("wrong", "scope-a").unwrap();
    let descriptor = fingerprint_resource(
        &managed,
        &source,
        ResourceCategory::Image,
        plaintext.len() as u64,
        Some(&key),
    )
    .unwrap();
    let store = MemoryStore::default();
    ensure_resource_uploaded(&store, &descriptor, Some(&key)).unwrap();

    let materialized = materialize_resource(
        &store,
        &descriptor.object_key,
        &cache,
        plaintext.len() as u64,
        Some(&key),
    )
    .unwrap();
    assert_eq!(fs::read(&materialized.path).unwrap(), plaintext);
    assert!(verify_local_resource(
        &materialized.path,
        &descriptor.object_key,
        plaintext.len() as u64,
        Some(&key),
    )
    .unwrap());
    assert!(!verify_local_resource(
        &materialized.path,
        &descriptor.object_key,
        plaintext.len() as u64,
        Some(&wrong),
    )
    .unwrap());

    assert!(materialize_resource(
        &store,
        &descriptor.object_key,
        &wrong_cache,
        plaintext.len() as u64,
        Some(&wrong),
    )
    .is_err());
    assert!(fs::read_dir(&wrong_cache).unwrap().next().is_none());

    let corrupt_store = MemoryStore::default();
    let mut corrupted = store
        .objects
        .borrow()
        .get(&descriptor.object_key)
        .cloned()
        .unwrap();
    let middle = corrupted.len() / 2;
    corrupted[middle] ^= 0x80;
    corrupt_store
        .objects
        .borrow_mut()
        .insert(descriptor.object_key.clone(), corrupted);
    assert!(materialize_resource(
        &corrupt_store,
        &descriptor.object_key,
        &corrupt_cache,
        plaintext.len() as u64,
        Some(&key),
    )
    .is_err());
    assert!(fs::read_dir(&corrupt_cache).unwrap().next().is_none());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_empty_resource_round_trips_without_phantom_chunks() {
    let root = temporary_directory("encrypted-empty");
    let managed = root.join("managed");
    let cache = root.join("cache");
    fs::create_dir_all(&managed).unwrap();
    let source = managed.join("empty.bin");
    fs::write(&source, []).unwrap();
    let key = SessionKey::derive("password", "scope-a").unwrap();
    let descriptor =
        fingerprint_resource(&managed, &source, ResourceCategory::File, 0, Some(&key)).unwrap();
    let store = MemoryStore::default();
    let uploaded = ensure_resource_uploaded(&store, &descriptor, Some(&key)).unwrap();
    assert_eq!(uploaded.size_bytes, RESOURCE_HEADER_LEN as u64);
    let materialized =
        materialize_resource(&store, &descriptor.object_key, &cache, 0, Some(&key)).unwrap();
    assert_eq!(materialized.size_bytes, 0);
    assert_eq!(materialized.transferred_bytes, RESOURCE_HEADER_LEN as u64);
    assert!(fs::read(materialized.path).unwrap().is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn materialization_reuses_valid_cache_and_repairs_corruption() {
    let root = temporary_directory("download");
    let cache = root.join("cache");
    let bytes = b"resource bytes".repeat(4096);
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let key = resource_object_key(ResourceCategory::Image, &sha256, "png").unwrap();
    let store = MemoryStore::default();
    store
        .objects
        .borrow_mut()
        .insert(key.clone(), bytes.clone());

    let first = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
    assert!(!first.reused_local_file);
    assert_eq!(fs::read(&first.path).unwrap(), bytes);
    let second = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
    assert!(second.reused_local_file);
    assert_eq!(store.file_gets.get(), 1);

    fs::write(&first.path, b"corrupt").unwrap();
    let repaired = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
    assert!(!repaired.reused_local_file);
    assert_eq!(fs::read(&repaired.path).unwrap(), bytes);
    assert_eq!(store.file_gets.get(), 2);

    fs::write(&first.path, vec![0u8; 2 * 1024 * 1024]).unwrap();
    let repaired_oversized = materialize_resource(&store, &key, &cache, 1024 * 1024, None).unwrap();
    assert!(!repaired_oversized.reused_local_file);
    assert_eq!(fs::read(&repaired_oversized.path).unwrap(), bytes);
    assert_eq!(store.file_gets.get(), 3);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_verification_rejects_missing_corrupt_and_oversized_files() {
    let root = temporary_directory("verify-local");
    let path = root.join("cached.bin");
    fs::write(&path, b"expected").unwrap();
    let key = resource_object_key(
        ResourceCategory::File,
        &hex::encode(Sha256::digest(b"expected")),
        "bin",
    )
    .unwrap();

    assert!(verify_local_resource(&path, &key, 1024, None).unwrap());
    fs::write(&path, b"corrupt").unwrap();
    assert!(!verify_local_resource(&path, &key, 1024, None).unwrap());
    assert!(!verify_local_resource(&path, &key, 3, None).unwrap());
    fs::remove_file(&path).unwrap();
    assert!(!verify_local_resource(&path, &key, 1024, None).unwrap());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fingerprint_rejects_outside_and_oversized_files() {
    let root = temporary_directory("bounds");
    let managed = root.join("managed");
    fs::create_dir_all(&managed).unwrap();
    let outside = root.join("outside.bin");
    fs::write(&outside, vec![0u8; 32]).unwrap();
    assert!(fingerprint_resource(&managed, &outside, ResourceCategory::File, 1024, None).is_err());

    let oversized = managed.join("oversized.bin");
    fs::write(&oversized, vec![0u8; 2048]).unwrap();
    assert!(
        fingerprint_resource(&managed, &oversized, ResourceCategory::File, 1024, None).is_err()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_remote_resource_never_replaces_the_cache() {
    let root = temporary_directory("corrupt-remote");
    let cache = root.join("cache");
    let expected = b"expected";
    let sha256 = hex::encode(Sha256::digest(expected));
    let key = resource_object_key(ResourceCategory::File, &sha256, "bin").unwrap();
    let store = MemoryStore::default();
    store
        .objects
        .borrow_mut()
        .insert(key.clone(), b"corrupt".to_vec());

    assert!(materialize_resource(&store, &key, &cache, 1024, None).is_err());
    let parsed = parse_resource_key(&key).unwrap();
    let expected_path = cache.join(format!("sha256-{}.{}", parsed.sha256, parsed.extension));
    assert!(!expected_path.exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mutation_resources_round_trip_without_machine_local_paths() {
    let root = temporary_directory("mutation-round-trip");
    let source_root = root.join("source");
    let target_root = root.join("target");
    let source_roots = ResourceRoots::new(
        source_root.join("images"),
        source_root.join("files"),
        source_root.join("icons"),
    );
    let target_roots = ResourceRoots::new(
        target_root.join("images"),
        target_root.join("files"),
        target_root.join("icons"),
    );
    for directory in [
        &source_roots.images,
        &source_roots.files,
        &source_roots.icons,
        &target_roots.images,
        &target_roots.files,
        &target_roots.icons,
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    let image_path = source_roots.images.join("image.png");
    let preview_path = source_root.join("previews/image.jpg");
    let file_path = source_roots.files.join("document.txt");
    fs::create_dir_all(preview_path.parent().unwrap()).unwrap();
    fs::write(&image_path, b"image-bytes").unwrap();
    fs::write(&preview_path, b"preview-bytes").unwrap();
    fs::write(&file_path, b"file-bytes").unwrap();
    fs::write(source_roots.icons.join("app.png"), b"icon-bytes").unwrap();

    let mut image = sample_item("image", SyncItemKind::Image);
    image.item.resource_path = Some(image_path.to_string_lossy().to_string());
    image.item.preview_path = Some(preview_path.to_string_lossy().to_string());
    image.item.icon_path = Some("app.png".to_string());
    image.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": image_path,
            "storagePath": image_path,
            "previewPath": preview_path,
        })
        .to_string(),
    );

    let mut file = sample_item("file", SyncItemKind::File);
    let source_file = file_path.to_string_lossy().to_string();
    file.item.resource_path = Some(source_file.clone());
    file.item.text_content = Some(serde_json::to_string(&[&source_file]).unwrap());
    file.item.metadata_json = Some(
        serde_json::json!({
            "resourcePath": source_file,
            "files": [{
                "storagePath": source_file,
                "originalPath": "C:/original/document.txt",
            }],
        })
        .to_string(),
    );

    let store = MemoryStore::default();
    let limits = ResourceLimits {
        image_bytes: 1024,
        file_bytes: 1024,
        icon_bytes: 1024,
    };
    let mut batch = MutationBatch {
        upserts: vec![image, file],
        tombstones: Vec::new(),
    };
    let uploaded =
        prepare_mutation_resources(&store, &mut batch, &source_roots, limits, None).unwrap();
    assert_eq!(uploaded.referenced_resources, 3);
    assert_eq!(uploaded.transferred_resources, 3);
    assert!(batch.upserts.iter().all(|item| {
        item.item
            .resource_path
            .as_deref()
            .is_none_or(|path| path.starts_with("v1/resources/"))
    }));
    assert!(batch.upserts[0].item.preview_path.is_none());
    assert!(!store
        .objects
        .borrow()
        .keys()
        .any(|key| key.starts_with("v1/resources/preview/")));
    assert!(batch.upserts[0]
        .item
        .icon_path
        .as_deref()
        .unwrap()
        .starts_with("v1/resources/icon/"));

    let downloaded =
        materialize_mutation_resources(&store, &mut batch, &target_roots, limits, None).unwrap();
    assert_eq!(downloaded.referenced_resources, 3);
    assert_eq!(downloaded.transferred_resources, 3);
    assert!(batch.upserts[0].item.preview_path.is_none());
    let target_images = fs::canonicalize(&target_roots.images).unwrap();
    let target_files = fs::canonicalize(&target_roots.files).unwrap();
    assert!(
        Path::new(batch.upserts[0].item.resource_path.as_deref().unwrap())
            .starts_with(&target_images)
    );
    assert!(
        Path::new(batch.upserts[1].item.resource_path.as_deref().unwrap())
            .starts_with(&target_files)
    );
    let local_icon = batch.upserts[0].item.icon_path.as_deref().unwrap();
    assert!(local_icon.starts_with("sha256-"));
    assert!(local_icon.ends_with(".png"));
    let metadata: serde_json::Value =
        serde_json::from_str(batch.upserts[1].item.metadata_json.as_deref().unwrap()).unwrap();
    assert!(
        Path::new(metadata["files"][0]["storagePath"].as_str().unwrap()).starts_with(&target_files)
    );
    assert_eq!(
        metadata["files"][0]["originalPath"],
        "C:/original/document.txt"
    );
    let image_metadata: serde_json::Value =
        serde_json::from_str(batch.upserts[0].item.metadata_json.as_deref().unwrap()).unwrap();
    assert!(image_metadata.get("previewPath").is_none());

    fs::remove_dir_all(root).unwrap();
}
