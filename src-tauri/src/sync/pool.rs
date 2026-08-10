//! Remote resource pool coordination.
//!
//! Resource files (images, previews, files, icons) are stored on the remote as
//! standalone objects under a dedicated `resources/` folder instead of always
//! being embedded inside baseline/oplog payloads. A refreshed baseline can then
//! reference already-uploaded files (`bytes: None`) instead of re-transferring
//! every file, so rebuilding the index never re-uploads resources that the pool
//! already holds.
//!
//! `PoolManifest` is a per-device, per-remote on-disk cache of which `rel_path`s
//! this device knows exist in that scoped remote pool. It is updated when a
//! file is successfully uploaded to the pool and when a payload that explicitly
//! references a pooled resource is downloaded, so later snapshots can downgrade
//! inline resources to plain pool references without a network round-trip. The
//! manifest is only ever consulted for optimization; payloads keep their inline
//! `bytes` for every resource not confirmed in the same pool, so switching
//! remotes cannot create dangling references.

use std::collections::HashSet;
use std::fs;

use crate::storage::StoragePaths;
use crate::sync::wire::OplogResource;

/// Remote folder that holds standalone resource objects.
pub const POOL_DIR: &str = "resources";

/// A resource pool transport. Each provider backs objects at
/// `resources/<rel_path>` and applies its own encryption when configured.
pub trait PoolStorage {
    /// Stable provider/endpoint/path scope used to isolate the local manifest.
    fn scope_key(&self) -> &str;
    /// Uploads the plaintext resource bytes to the pool object.
    fn upload(&self, rel_path: &str, bytes: &[u8]) -> Result<(), String>;
    /// Downloads the plaintext resource bytes from the pool object.
    fn download(&self, rel_path: &str) -> Result<Vec<u8>, String>;
}

fn manifest_path(paths: &StoragePaths, remote_scope: &str) -> std::path::PathBuf {
    paths
        .data_directory
        .join(format!("sync-pool-manifest-{remote_scope}.json"))
}

/// Loads the persisted pool manifest (rel_paths known to exist remotely).
/// Missing or unreadable manifests yield an empty set.
pub fn load_pool_manifest(paths: &StoragePaths, remote_scope: &str) -> HashSet<String> {
    let path = manifest_path(paths, remote_scope);
    let Ok(content) = fs::read(&path) else {
        return HashSet::new();
    };
    serde_json::from_slice::<Vec<String>>(&content)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Persists the pool manifest to disk.
pub fn save_pool_manifest(
    paths: &StoragePaths,
    remote_scope: &str,
    manifest: &HashSet<String>,
) -> Result<(), String> {
    let path = manifest_path(paths, remote_scope);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut entries: Vec<String> = manifest.iter().cloned().collect();
    entries.sort();
    let json = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Merges `rel_path`s that an incoming payload explicitly references from the
/// pool (`bytes: None`) into the local pool manifest. Those objects are known
/// to exist remotely because the sender only downgraded files after uploading
/// them. Inline (`bytes: Some`) resources are not recorded: an older or
/// non-pooling sender embeds them without a standalone pool object, so claiming
/// them would let a later snapshot refresh emit a dangling reference. Best-effort;
/// manifest write failures are ignored.
pub fn absorb_pool_paths(
    paths: &StoragePaths,
    resources: &[OplogResource],
    pool: &dyn PoolStorage,
) {
    if resources.is_empty() {
        return;
    }
    let mut manifest = load_pool_manifest(paths, pool.scope_key());
    let mut changed = false;
    for resource in resources {
        if resource.bytes.is_some() {
            continue;
        }
        if manifest.insert(resource.rel_path.clone()) {
            changed = true;
        }
    }
    if changed {
        let _ = save_pool_manifest(paths, pool.scope_key(), &manifest);
    }
}

/// Uploads to the pool any resource bytes that are not yet known to exist
/// there, records successful uploads in the local manifest, and keeps the
/// inline `bytes` so the payload stays self-sufficient. Upload failures are
/// logged and skipped (the payload keeps its inline copy); the pool is an
/// optimization, never a correctness dependency.
///
/// Returns the number of resource objects newly uploaded to the pool.
pub fn ensure_pool_uploads(
    paths: &StoragePaths,
    resources: &mut [OplogResource],
    pool: &dyn PoolStorage,
) -> usize {
    let mut manifest = load_pool_manifest(paths, pool.scope_key());
    let mut changed = false;
    let mut uploaded = 0;
    for resource in resources.iter_mut() {
        if manifest.contains(&resource.rel_path) {
            continue;
        }
        let Some(bytes) = resource.bytes.clone() else {
            continue;
        };
        match pool.upload(&resource.rel_path, &bytes) {
            Ok(()) => {
                if manifest.insert(resource.rel_path.clone()) {
                    changed = true;
                }
                uploaded += 1;
            }
            Err(e) => {
                println!(
                    "[pool] upload failed for {}: {e}; keeping inline bytes",
                    resource.rel_path
                );
            }
        }
    }
    if changed {
        let _ = save_pool_manifest(paths, pool.scope_key(), &manifest);
    }
    uploaded
}

/// Downgrades every resource whose `rel_path` is already in the pool to a plain
/// reference (`bytes: None`). New resources keep their inline bytes so the
/// payload stays self-sufficient for its first occurrence.
pub fn mark_pool_references(resources: &mut [OplogResource], known_pool: &HashSet<String>) {
    for resource in resources {
        if resource.bytes.is_some() && known_pool.contains(&resource.rel_path) {
            resource.bytes = None;
        }
    }
}

/// Uploads any resource bytes not yet known to the pool and downgrades only the
/// resources that were already pooled before this run. Newly uploaded resources
/// keep their inline bytes so the current payload stays self-sufficient for the
/// file's first occurrence, while future payloads reference the pool.
/// Returns the number of objects newly uploaded.
pub fn prepare_pool_refs(
    paths: &StoragePaths,
    resources: &mut [OplogResource],
    pool: &dyn PoolStorage,
) -> usize {
    let known_before = load_pool_manifest(paths, pool.scope_key());
    let uploaded = ensure_pool_uploads(paths, resources, pool);
    mark_pool_references(resources, &known_before);
    uploaded
}

/// Remote object path for a pooled resource (`resources/<rel_path>`).
pub fn pool_object_path(rel_path: &str) -> String {
    format!("{POOL_DIR}/{rel_path}")
}

/// Local cache file path for the pool manifest. Exposed for tests only.
pub fn pool_manifest_file(paths: &StoragePaths, remote_scope: &str) -> std::path::PathBuf {
    manifest_path(paths, remote_scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoragePaths;

    fn temporary_storage(label: &str) -> StoragePaths {
        let project = std::env::temp_dir()
            .join("clipboard-sync-pool")
            .join(label)
            .join("project");
        let _ = std::fs::remove_dir_all(project.parent().unwrap());
        StoragePaths::initialize(project).unwrap()
    }

    #[test]
    fn manifest_round_trips() {
        let paths = temporary_storage("manifest");
        let mut manifest = HashSet::new();
        manifest.insert("image/a.png".to_string());
        manifest.insert("file/b.pdf".to_string());

        save_pool_manifest(&paths, "remote-a", &manifest).unwrap();
        let loaded = load_pool_manifest(&paths, "remote-a");
        assert_eq!(loaded, manifest);
        assert!(load_pool_manifest(&paths, "remote-b").is_empty());

        save_pool_manifest(&paths, "remote-a", &HashSet::new()).unwrap();
        assert!(load_pool_manifest(&paths, "remote-a").is_empty());
    }

    #[test]
    fn missing_manifest_yields_empty_set() {
        let paths = temporary_storage("missing");
        assert!(load_pool_manifest(&paths, "remote-a").is_empty());
    }

    #[test]
    fn mark_pool_references_downgrades_only_known() {
        let mut resources = vec![
            OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: Some(b"a".to_vec()),
            },
            OplogResource {
                rel_path: "image/b.png".to_string(),
                bytes: Some(b"b".to_vec()),
            },
        ];
        let mut known = HashSet::new();
        known.insert("image/b.png".to_string());

        mark_pool_references(&mut resources, &known);

        assert!(resources[0].bytes.is_some());
        assert!(resources[1].bytes.is_none());
    }

    struct MemoryPool {
        objects: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
        fail_uploads: bool,
    }

    impl MemoryPool {
        fn new() -> Self {
            Self {
                objects: std::sync::Mutex::new(std::collections::HashMap::new()),
                fail_uploads: false,
            }
        }
    }

    impl PoolStorage for MemoryPool {
        fn scope_key(&self) -> &str {
            "test-remote"
        }

        fn upload(&self, rel_path: &str, bytes: &[u8]) -> Result<(), String> {
            if self.fail_uploads {
                return Err("upload failed".to_string());
            }
            self.objects
                .lock()
                .unwrap()
                .insert(rel_path.to_string(), bytes.to_vec());
            Ok(())
        }
        fn download(&self, rel_path: &str) -> Result<Vec<u8>, String> {
            self.objects
                .lock()
                .unwrap()
                .get(rel_path)
                .cloned()
                .ok_or_else(|| format!("missing {rel_path}"))
        }
    }

    fn inline_resources() -> Vec<OplogResource> {
        vec![
            OplogResource {
                rel_path: "image/a.png".to_string(),
                bytes: Some(b"a".to_vec()),
            },
            OplogResource {
                rel_path: "image/b.png".to_string(),
                bytes: Some(b"b".to_vec()),
            },
        ]
    }

    #[test]
    fn ensure_uploads_uploads_new_and_keeps_inline() {
        let paths = temporary_storage("ensure-uploads");
        let pool = MemoryPool::new();
        let mut resources = inline_resources();

        let uploaded = ensure_pool_uploads(&paths, &mut resources, &pool);
        assert_eq!(uploaded, 2);
        assert!(resources[0].bytes.is_some(), "inline preserved");
        assert!(resources[1].bytes.is_some());

        // A second run now has everything pooled, so nothing is re-uploaded.
        let again = ensure_pool_uploads(&paths, &mut resources, &pool);
        assert_eq!(again, 0);
    }

    #[test]
    fn ensure_uploads_skips_failed_objects() {
        let paths = temporary_storage("ensure-fail");
        let mut pool = MemoryPool::new();
        pool.fail_uploads = true;
        let mut resources = inline_resources();

        let uploaded = ensure_pool_uploads(&paths, &mut resources, &pool);
        assert_eq!(uploaded, 0);
        assert!(resources[0].bytes.is_some());
        assert!(resources[1].bytes.is_some());
    }

    #[test]
    fn mark_references_after_ensure_downgrades() {
        let paths = temporary_storage("mark-after-ensure");
        let pool = MemoryPool::new();
        let mut resources = inline_resources();
        ensure_pool_uploads(&paths, &mut resources, &pool);

        let known = load_pool_manifest(&paths, pool.scope_key());
        mark_pool_references(&mut resources, &known);
        assert!(resources[0].bytes.is_none());
        assert!(resources[1].bytes.is_none());
    }

    #[test]
    fn absorb_only_claims_explicit_pool_references() {
        let paths = temporary_storage("absorb");
        let resources = vec![
            OplogResource {
                rel_path: "image/pooled.png".to_string(),
                bytes: None,
            },
            OplogResource {
                rel_path: "image/inline.png".to_string(),
                bytes: Some(b"x".to_vec()),
            },
        ];
        let pool = MemoryPool::new();
        absorb_pool_paths(&paths, &resources, &pool);
        let manifest = load_pool_manifest(&paths, pool.scope_key());
        assert!(manifest.contains("image/pooled.png"));
        assert!(!manifest.contains("image/inline.png"));
    }

    #[test]
    fn pool_object_path_uses_resources_prefix() {
        assert_eq!(pool_object_path("image/abc.png"), "resources/image/abc.png");
    }
}
