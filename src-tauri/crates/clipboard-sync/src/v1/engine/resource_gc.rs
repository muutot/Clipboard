//! Remote resource garbage collection.
//!
//! Resources are content-addressed and are not part of the checkpoint pack
//! format. A checkpoint can therefore outlive the metadata that points at its
//! resource objects. This module keeps the live set explicit:
//! - `ReferenceScan` decodes every live checkpoint/snapshot/segment.
//! - `Manifest` trusts a small per-checkpoint reference manifest and falls
//!   back to the reference scan when that manifest is absent or invalid.
//!
//! Both modes scan snapshots and segments, which may cover newer state.
//! v1 retains resource bodies: no publication lease fences a content-addressed
//! object's HEAD reuse against deletion after the reference scan. An upload
//! grace window cannot protect a newly published reference to an old object.
//! Only obsolete reference sidecars can be collected safely here.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::*;
use crate::v1::parse_resource_key;

const RESOURCE_MANIFEST_PREFIX: &str = "v1/resource-refs/";
const RESOURCE_MANIFEST_VERSION: u8 = 1;
const MAX_RESOURCE_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    version: u8,
    generation: u64,
    checkpoint_sha256: String,
    resources: Vec<String>,
}

pub(super) fn publish_resource_manifest(
    store: &impl ObjectStore,
    generation: u64,
    checkpoint_sha256: &str,
    resources: &BTreeSet<String>,
) -> Result<(), String> {
    cancellation::check()?;
    let bytes = serde_json::to_vec(&ResourceManifest {
        version: RESOURCE_MANIFEST_VERSION,
        generation,
        checkpoint_sha256: checkpoint_sha256.to_string(),
        resources: resources.iter().cloned().collect(),
    })
    .map_err(|error| format!("failed to encode sync resource manifest: {error}"))?;
    if bytes.len() > MAX_RESOURCE_MANIFEST_BYTES {
        // The sidecar is an optimisation for the manifest mode. A very large
        // scope falls back to the reference scan instead of failing sync.
        return Ok(());
    }
    store.put(
        &resource_manifest_key(generation, checkpoint_sha256),
        bytes,
        PutCondition::IfAbsent,
    )?;
    Ok(())
}

fn resource_manifest_key(generation: u64, checkpoint_sha256: &str) -> String {
    format!("{RESOURCE_MANIFEST_PREFIX}{generation:020}-{checkpoint_sha256}.json")
}

fn parse_resource_manifest_key(key: &str) -> Option<(u64, String)> {
    let stem = key
        .strip_prefix(RESOURCE_MANIFEST_PREFIX)?
        .strip_suffix(".json")?;
    let (generation, sha256) = stem.split_once('-')?;
    if generation.len() != 20 || !generation.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !is_sha256(sha256) {
        return None;
    }
    Some((generation.parse().ok()?, sha256.to_string()))
}

fn read_resource_manifest(
    store: &impl ObjectStore,
    generation: u64,
    checkpoint_sha256: &str,
) -> Result<Option<BTreeSet<String>>, String> {
    cancellation::check()?;
    let Some(object) = store.get(&resource_manifest_key(generation, checkpoint_sha256))? else {
        return Ok(None);
    };
    if object.bytes.len() > MAX_RESOURCE_MANIFEST_BYTES {
        return Ok(None);
    }
    let Ok(manifest) = serde_json::from_slice::<ResourceManifest>(&object.bytes) else {
        return Ok(None);
    };
    if manifest.version != RESOURCE_MANIFEST_VERSION
        || manifest.generation != generation
        || manifest.checkpoint_sha256 != checkpoint_sha256
        || manifest
            .resources
            .iter()
            .any(|key| parse_resource_key(key).is_err())
    {
        return Ok(None);
    }
    Ok(Some(manifest.resources.into_iter().collect()))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn garbage_collect_resources(
    store: &impl ObjectStore,
    head: &CheckpointHead,
    mode: ResourceGcMode,
    session_key: Option<&SessionKey>,
    temporary_directory: &std::path::Path,
    result: &mut SyncEngineResult,
) -> Result<u64, String> {
    if mode == ResourceGcMode::Off {
        return Ok(0);
    }
    cancellation::check()?;
    let mut keep = BTreeSet::new();

    let mut checkpoints = BTreeSet::new();
    for info in store.list("v1/checkpoints/", None)? {
        cancellation::check()?;
        let Ok(parsed) = parse_checkpoint_key(&info.key) else {
            continue;
        };
        checkpoints.insert((parsed.generation, parsed.sha256.clone()));
        if mode == ResourceGcMode::Manifest {
            if let Some(manifest) =
                read_resource_manifest(store, parsed.generation, &parsed.sha256)?
            {
                keep.extend(manifest);
                continue;
            }
        }
        collect_checkpoint_references(
            store,
            &info,
            &parsed.sha256,
            session_key,
            temporary_directory,
            result,
            &mut keep,
        )?;
    }

    for info in store.list("v1/snapshots/", None)? {
        cancellation::check()?;
        let Some(sha256) = snapshot_sha256(&info.key) else {
            continue;
        };
        let file = get_verified_object_to_file(
            store,
            &info.key,
            sha256,
            info.size_bytes,
            temporary_directory,
            result,
        )?;
        let mut reader = open_snapshot_pack(&file.path, session_key)?;
        for batch in reader.by_ref() {
            cancellation::check()?;
            add_mutation_resource_keys(&mut keep, &batch?)?;
        }
        if !reader.is_complete() {
            return Err("snapshot pack is incomplete during resource GC".to_string());
        }
    }

    for info in store.list("v1/segments/", None)? {
        cancellation::check()?;
        let Ok(parsed) = parse_segment_key(&info.key) else {
            continue;
        };
        let bytes = get_verified_object(store, &info.key, &parsed.sha256, info.size_bytes, result)?;
        let segment = decode_segment(&bytes, session_key)?;
        add_mutation_resource_keys(&mut keep, &segment.mutations)?;
    }

    // A peer can reuse any existing resource immediately after this scan.
    // Neither another scan nor LastModified makes an unconditional DELETE
    // atomic with that peer's HEAD and subsequent head publication. Keep all
    // bodies until a protocol with publication/deletion fencing is available.
    let mut deleted = 0u64;
    for info in store.list(RESOURCE_MANIFEST_PREFIX, None)? {
        cancellation::check()?;
        let Some((generation, sha256)) = parse_resource_manifest_key(&info.key) else {
            continue;
        };
        if checkpoints.contains(&(generation, sha256)) || generation >= head.generation {
            continue;
        }
        store.delete(&info.key)?;
        deleted = checked_add(deleted, 1, "deleted remote object count")?;
    }

    Ok(deleted)
}

#[allow(clippy::too_many_arguments)]
fn collect_checkpoint_references(
    store: &impl ObjectStore,
    info: &ObjectInfo,
    sha256: &str,
    session_key: Option<&SessionKey>,
    temporary_directory: &std::path::Path,
    result: &mut SyncEngineResult,
    keep: &mut BTreeSet<String>,
) -> Result<(), String> {
    let file = get_verified_object_to_file(
        store,
        &info.key,
        sha256,
        info.size_bytes,
        temporary_directory,
        result,
    )?;
    let mut reader = open_checkpoint_pack(&file.path, session_key)?;
    for batch in reader.by_ref() {
        cancellation::check()?;
        add_mutation_resource_keys(keep, &batch?)?;
    }
    if !reader.is_complete() {
        return Err("checkpoint pack is incomplete during resource GC".to_string());
    }
    Ok(())
}

fn add_mutation_resource_keys(
    keep: &mut BTreeSet<String>,
    mutations: &MutationBatch,
) -> Result<(), String> {
    let references = collect_mutation_resource_refs(mutations)?;
    for reference in references.values().flatten() {
        keep.insert(reference.object_key.clone());
    }
    Ok(())
}

fn snapshot_sha256(key: &str) -> Option<&str> {
    let filename = key.rsplit('/').next()?;
    let sha256 = filename.strip_suffix(".pack")?;
    is_sha256(sha256).then_some(sha256)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
