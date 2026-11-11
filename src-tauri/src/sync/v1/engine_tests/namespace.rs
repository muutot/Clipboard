//! Namespace identity, first publication and upgrade scenarios.

use super::*;

static MEDIA_SWITCH_TEST_LOCK: Mutex<()> = Mutex::new(());

fn deferred_media_switch_scenario(via_off: bool, legacy: bool, encrypted: bool, mode: &str) {
    use crate::background_operations::BackgroundOperations;
    use crate::commands::sync::{
        SyncCancellation, SyncSettings, persist_sync_config, update_sync_config,
    };
    use crate::config::{ConfigStore, SyncConfig, SyncProvider};
    use clipboard_sync::v1::namespace::resolve_namespace;
    use std::sync::Arc;

    let _test_guard = MEDIA_SWITCH_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let origin_paths = temp_paths("switch-media-origin");
    let mover_paths = temp_paths("switch-media-mover");
    let reader_paths = temp_paths("switch-media-reader");
    let origin = Database::open(&origin_paths.database).unwrap();
    let mover = Database::open(&mover_paths.database).unwrap();
    let reader = Database::open(&reader_paths.database).unwrap();
    let image_path = origin_paths.images.join("source.png");
    let expected = b"target-switch-image-body";
    fs::write(&image_path, expected).unwrap();
    origin
        .save_item(&image_item("deferred-image", &image_path))
        .unwrap();
    let mut config = ConfigStore::load(&mover_paths.project).unwrap();
    config.set_privacy_local_only(false).unwrap();
    let target_a = SyncConfig {
        provider: SyncProvider::S3,
        endpoint: Some("http://127.0.0.1:9000".into()),
        s3_bucket: Some("target-a".into()),
        s3_access_key: Some("fixture-access".into()),
        s3_secret_key: Some("fixture-secret".into()),
        sync_password: encrypted.then(|| "synthetic-switch-password".into()),
        ..SyncConfig::default()
    };
    let mut target_b = target_a.clone();
    target_b.s3_bucket = Some("target-b".into());
    let a_settings = SyncSettings::validated_from_sync_config(&target_a).unwrap();
    let b_settings = SyncSettings::validated_from_sync_config(&target_b).unwrap();
    let store_a = MemoryStore::default();
    let store_b = MemoryStore::default();
    persist_sync_config(&mover, &mut config, target_a.clone()).unwrap();
    if legacy {
        let key = encrypted.then(|| {
            SessionKey::derive("synthetic-switch-password", &a_settings.legacy_scope_id()).unwrap()
        });
        sync_database(
            &store_a,
            &origin,
            &origin_paths,
            &a_settings.legacy_scope_id(),
            key.as_ref(),
            options(),
        )
        .unwrap();
    }
    let (binding, old_key) = resolve_namespace(
        &store_a,
        &a_settings.remote_scope_id(),
        &a_settings.legacy_scope_id(),
        target_a.sync_password.as_deref(),
        None,
    )
    .unwrap();
    mover
        .remember_sync_namespace(&a_settings.remote_scope_id(), &binding.to_bytes())
        .unwrap();
    sync_database(
        &store_a,
        &origin,
        &origin_paths,
        &binding.scope,
        old_key.as_ref(),
        options(),
    )
    .unwrap();
    sync_database(
        &store_a,
        &mover,
        &mover_paths,
        &binding.scope,
        old_key.as_ref(),
        options(),
    )
    .unwrap();
    assert!(
        mover
            .get_item("deferred-image")
            .unwrap()
            .unwrap()
            .resource_path
            .is_none()
    );
    assert_eq!(
        mover
            .get_sync_resource_refs(&binding.scope, "deferred-image")
            .unwrap()
            .len(),
        1
    );
    let reference = mover
        .get_sync_resource_refs(&binding.scope, "deferred-image")
        .unwrap()
        .remove(0);
    if mode == "cached-local-only" {
        let local = materialize_resource(
            &store_a,
            &reference.object_key,
            &mover_paths.images,
            1024 * 1024,
            old_key.as_ref(),
        )
        .unwrap();
        mover
            .mark_sync_resource_materialized(
                &binding.scope,
                "deferred-image",
                &reference,
                &local.path.to_string_lossy(),
            )
            .unwrap();
        config.set_privacy_local_only(true).unwrap();
    } else if mode == "local-only" {
        config.set_privacy_local_only(true).unwrap();
    }
    let config = Mutex::new(config);
    let policy = Arc::new(SyncCancellation::default());
    let operations = Arc::new(BackgroundOperations::default());
    if via_off {
        let mut off = target_a.clone();
        off.provider = SyncProvider::Off;
        update_sync_config(
            &mover,
            &mover_paths,
            &config,
            &policy,
            &operations,
            |_| Ok(off),
            |_| -> Result<MemoryStore, String> {
                panic!("Off with the same source must not download")
            },
        )
        .unwrap();
    }
    if mode == "same-target" {
        let mut same = target_a.clone();
        same.max_sync_image_bytes *= 2;
        update_sync_config(
            &mover,
            &mover_paths,
            &config,
            &policy,
            &operations,
            |_| Ok(same),
            |_| -> Result<MemoryStore, String> { panic!("same target must not download") },
        )
        .unwrap();
    }
    if mode == "missing" {
        store_a
            .objects
            .lock()
            .unwrap()
            .remove(&reference.object_key);
    }
    if mode == "cancel" {
        let policy = policy.clone();
        *store_a.after_download.lock().unwrap() = Some((
            reference.object_key.clone(),
            Box::new(move || policy.cancel_active()),
        ));
    }
    if mode == "task-cancel" {
        let operations = operations.clone();
        *store_a.after_download.lock().unwrap() = Some((
            reference.object_key.clone(),
            Box::new(move || {
                let id = operations
                    .snapshot(crate::background_operations::OperationKind::Sync)
                    .unwrap()
                    .id;
                assert!(operations.cancel(crate::background_operations::OperationKind::Sync, &id));
            }),
        ));
    }
    let other = (mode == "busy").then(|| Database::open(&mover_paths.database).unwrap());
    let lease = other
        .as_ref()
        .map(|database| database.begin_sync_run().unwrap());
    let before = config.lock().unwrap().sync_config();
    let changing_to = if mode == "incremental-off" {
        let mut incomplete = target_b.clone();
        incomplete.provider = SyncProvider::Off;
        incomplete.s3_bucket = None;
        incomplete
    } else {
        target_b.clone()
    };
    let result = update_sync_config(
        &mover,
        &mover_paths,
        &config,
        &policy,
        &operations,
        |_| Ok(changing_to),
        |_| {
            assert!(
                config.try_lock().is_ok(),
                "configuration mutex must be released during I/O"
            );
            if mode == "cached-local-only" {
                panic!("verified local bodies must not access the network")
            }
            if mode == "conflict" {
                let mut newer = before.clone();
                newer.auto_sync = !newer.auto_sync;
                config.lock().unwrap().set_sync_config(newer).unwrap();
            }
            Ok(store_a)
        },
    );
    drop(lease);
    drop(other);
    if matches!(
        mode,
        "missing" | "cancel" | "task-cancel" | "local-only" | "conflict" | "busy"
    ) {
        let error = result.unwrap_err();
        let saved = ConfigStore::load(&mover_paths.project)
            .unwrap()
            .sync_config();
        if mode == "conflict" {
            assert!(error.contains("configuration changed"));
            let mut newer = before;
            newer.auto_sync = !newer.auto_sync;
            assert_eq!(saved, newer);
        } else {
            assert_eq!(
                saved, before,
                "failed preparation must retain the old config on disk"
            );
        }
        if matches!(mode, "cancel" | "task-cancel") {
            assert!(error.contains(clipboard_sync::cancellation::CANCELLED));
            assert_eq!(
                operations
                    .snapshot(crate::background_operations::OperationKind::Sync)
                    .unwrap()
                    .status,
                crate::background_operations::OperationStatus::Cancelled
            );
            assert!(!policy.0.is_cancelled());
        }
        drop(origin);
        drop(mover);
        drop(reader);
        drop(config);
        fs::remove_dir_all(origin_paths.project).unwrap();
        fs::remove_dir_all(mover_paths.project).unwrap();
        fs::remove_dir_all(reader_paths.project).unwrap();
        return;
    }
    result.unwrap();
    if mode == "incremental-off" {
        update_sync_config(
            &mover,
            &mover_paths,
            &config,
            &policy,
            &operations,
            |_| Ok(target_b.clone()),
            |_| -> Result<MemoryStore, String> {
                panic!("old bodies were cached before the incomplete edit")
            },
        )
        .unwrap();
    }
    let (b_binding, new_key) = resolve_namespace(
        &store_b,
        &b_settings.remote_scope_id(),
        &b_settings.legacy_scope_id(),
        target_b.sync_password.as_deref(),
        None,
    )
    .unwrap();
    sync_database(
        &store_b,
        &mover,
        &mover_paths,
        &b_binding.scope,
        new_key.as_ref(),
        options(),
    )
    .unwrap();
    sync_database(
        &store_b,
        &reader,
        &reader_paths,
        &b_binding.scope,
        new_key.as_ref(),
        options(),
    )
    .unwrap();
    let refs = reader
        .get_sync_resource_refs(&b_binding.scope, "deferred-image")
        .unwrap();
    let body = refs.first().and_then(|reference| {
        let resource = materialize_resource(
            &store_b,
            &reference.object_key,
            &reader_paths.images,
            1024 * 1024,
            new_key.as_ref(),
        )
        .ok()?;
        fs::read(resource.path).ok()
    });
    drop(origin);
    drop(mover);
    drop(reader);
    drop(config);
    fs::remove_dir_all(origin_paths.project).unwrap();
    fs::remove_dir_all(mover_paths.project).unwrap();
    fs::remove_dir_all(reader_paths.project).unwrap();
    assert_eq!(
        body.as_deref(),
        Some(expected.as_slice()),
        "B must carry deferred media from A"
    );
}

#[test]
fn deferred_media_survives_target_switch() {
    deferred_media_switch_scenario(false, false, false, "success");
}
#[test]
fn deferred_media_survives_off_target_switch() {
    deferred_media_switch_scenario(true, false, false, "success");
}
#[test]
fn deferred_media_survives_legacy_encrypted_target_switch() {
    deferred_media_switch_scenario(false, true, true, "success");
}
#[test]
fn deferred_media_survives_incremental_off_edit() {
    deferred_media_switch_scenario(true, false, false, "incremental-off");
}
#[test]
fn deferred_media_failure_retains_source_config() {
    deferred_media_switch_scenario(false, false, false, "missing");
}
#[test]
fn deferred_media_cancellation_retains_source_config() {
    deferred_media_switch_scenario(false, false, false, "cancel");
}
#[test]
fn deferred_media_task_cancellation_retains_source_config() {
    deferred_media_switch_scenario(false, false, false, "task-cancel");
}
#[test]
fn deferred_media_busy_database_retains_source_config() {
    deferred_media_switch_scenario(false, false, false, "busy");
}
#[test]
fn deferred_media_conflicting_save_retains_newer_config() {
    deferred_media_switch_scenario(false, false, false, "conflict");
}
#[test]
fn deferred_media_local_only_blocks_download_and_retains_config() {
    deferred_media_switch_scenario(false, false, false, "local-only");
}
#[test]
fn deferred_media_verified_cache_allows_local_only_target_edit() {
    deferred_media_switch_scenario(false, false, false, "cached-local-only");
}
#[test]
fn deferred_media_same_target_save_skips_download() {
    deferred_media_switch_scenario(false, false, false, "same-target");
}

fn target_switch_scenario(via_off: bool, legacy: bool) {
    use crate::commands::sync::{SyncSettings, persist_sync_config};
    use crate::config::{ConfigStore, SyncConfig, SyncProvider};
    use clipboard_sync::v1::namespace::resolve_namespace;

    let source_paths = temp_paths("target-switch-source");
    let reader_paths = temp_paths("target-switch-reader");
    let source = Database::open(&source_paths.database).unwrap();
    let reader = Database::open(&reader_paths.database).unwrap();
    let mut config = ConfigStore::load(&source_paths.project).unwrap();
    let target_b = SyncConfig {
        provider: SyncProvider::S3,
        endpoint: Some("http://127.0.0.1:9000".into()),
        s3_bucket: Some("target-b".into()),
        s3_access_key: Some("fixture-access".into()),
        s3_secret_key: Some("fixture-secret".into()),
        ..SyncConfig::default()
    };
    let mut target_a = target_b.clone();
    target_a.s3_bucket = Some("target-a".into());
    let b_settings = SyncSettings::validated_from_sync_config(&target_b).unwrap();
    let a_settings = SyncSettings::validated_from_sync_config(&target_a).unwrap();
    let store_b = MemoryStore::default();
    let store_a = MemoryStore::default();
    source.save_item(&text_item("shared", "original")).unwrap();

    persist_sync_config(&source, &mut config, target_b.clone()).unwrap();
    // A legacy remote acquires a descriptor with its historical scope, which
    // differs from the credential-independent settings identity.
    if legacy {
        sync_database(
            &store_b,
            &source,
            &source_paths,
            &b_settings.legacy_scope_id(),
            None,
            options(),
        )
        .unwrap();
    }
    let (binding, key) = resolve_namespace(
        &store_b,
        &b_settings.remote_scope_id(),
        &b_settings.legacy_scope_id(),
        None,
        None,
    )
    .unwrap();
    source
        .remember_sync_namespace(&b_settings.remote_scope_id(), &binding.to_bytes())
        .unwrap();
    sync_database(
        &store_b,
        &source,
        &source_paths,
        &binding.scope,
        key.as_ref(),
        options(),
    )
    .unwrap();

    let published = source
        .get_or_create_sync_remote_state(&binding.scope)
        .unwrap();
    persist_sync_config(&source, &mut config, target_b.clone()).unwrap();
    let unchanged = source
        .get_or_create_sync_remote_state(&binding.scope)
        .unwrap();
    assert_eq!(unchanged.epoch, published.epoch);
    assert!(
        unchanged.initialized,
        "saving the same target must retain its publication"
    );

    persist_sync_config(&source, &mut config, target_a.clone()).unwrap();
    sync_database(
        &store_a,
        &source,
        &source_paths,
        &a_settings.remote_scope_id(),
        None,
        options(),
    )
    .unwrap();
    source.set_favorite("shared", true).unwrap();
    source
        .save_item(&text_item("new-at-a", "new payload"))
        .unwrap();
    sync_database(
        &store_a,
        &source,
        &source_paths,
        &a_settings.remote_scope_id(),
        None,
        options(),
    )
    .unwrap();
    assert!(source.get_sync_outbox_batch(100).unwrap().is_none());

    if via_off {
        target_a.provider = SyncProvider::Off;
        persist_sync_config(&source, &mut config, target_a).unwrap();
    }
    persist_sync_config(&source, &mut config, target_b).unwrap();
    sync_database(
        &store_b,
        &source,
        &source_paths,
        &binding.scope,
        key.as_ref(),
        options(),
    )
    .unwrap();
    // A new peer reads B alone: local state at the switching device cannot
    // conceal a missed publication.
    sync_database(
        &store_b,
        &reader,
        &reader_paths,
        &binding.scope,
        key.as_ref(),
        options(),
    )
    .unwrap();
    let shared = reader.get_item("shared").unwrap().unwrap();
    let added = reader.get_item("new-at-a").unwrap();
    drop(source);
    drop(reader);
    drop(config);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(reader_paths.project).unwrap();
    assert!(
        shared.is_favorite && added.is_some(),
        "B must receive changes acknowledged by A: favorite={}, insertion={}",
        shared.is_favorite,
        added.is_some()
    );
}

#[test]
fn target_switch_directly_republishes_acknowledged_changes() {
    target_switch_scenario(false, false);
}

#[test]
fn target_switch_through_off_republishes_acknowledged_changes() {
    target_switch_scenario(true, false);
}

#[test]
fn target_switch_to_legacy_binding_republishes_acknowledged_changes() {
    target_switch_scenario(false, true);
}

#[test]
fn target_switch_through_off_to_legacy_binding_republishes_acknowledged_changes() {
    target_switch_scenario(true, true);
}

#[test]
fn portable_backup_restored_shared_media_remains_publishable() {
    use crate::export::backup;
    let paths = temp_paths("backup-source");
    let restored_paths = temp_paths("backup-restored");
    let source = Database::open_in_memory().unwrap();
    let restored = Database::open_in_memory().unwrap();
    let binary = paths.images.join("shared.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
        .save(&binary)
        .unwrap();
    let expected = fs::read(&binary).unwrap();
    let image = image_item("image", &binary);
    let mut file = image_item("file", &binary);
    file.kind = ClipboardKind::File;
    file.text_content = Some(serde_json::json!([binary]).to_string());
    source.save_item(&image).unwrap();
    source.save_item(&file).unwrap();
    let archive = paths.project.join("shared.clipbackup");
    backup::create(&source, &archive).unwrap();
    let preview = backup::preview(&archive, &restored).unwrap();
    assert_eq!(preview.resource_count, 1);
    backup::restore(&archive, &preview.fingerprint, &restored, &restored_paths).unwrap();
    let mut mutations = MutationBatch {
        upserts: Vec::new(),
        tombstones: Vec::new(),
    };
    for id in ["image", "file"] {
        mutations.upserts.push(ReplicatedItem {
            item: restored.get_item(id).unwrap().unwrap().into(),
            version: RecordVersion {
                modified_at_ms: 1,
                writer_device_id: restored.get_sync_device_id().unwrap(),
            },
        });
    }
    let store = MemoryStore::default();
    let roots = SyncEnginePaths::from(&restored_paths).resource_roots;
    let stats = prepare_mutation_resources(
        &store,
        &mut mutations,
        &roots,
        options().resource_limits,
        None,
    )
    .unwrap();
    assert_eq!(stats.skipped_resources, 0);
    assert_eq!(stats.transferred_resources, 2);
    for (row, category) in mutations.upserts.iter().zip(["image", "file"]) {
        let key = row.item.resource_path.as_ref().unwrap();
        assert!(key.starts_with(&format!("v1/resources/{category}/")));
        assert_eq!(store.objects.lock().unwrap().get(key), Some(&expected));
    }
    fs::remove_dir_all(paths.project).unwrap();
    fs::remove_dir_all(restored_paths.project).unwrap();
}

#[test]
fn namespace_upgrade_preserves_legacy_encryption_and_accepts_rotated_credentials() {
    use clipboard_sync::v1::namespace::{NAMESPACE_KEY, Namespace, resolve_namespace};
    let store = MemoryStore::default();
    let source_paths = temp_paths("namespace-source");
    let target_paths = temp_paths("namespace-target");
    let source = Database::open_in_memory().unwrap();
    let target = Database::open_in_memory().unwrap();
    let stable = "c".repeat(64);
    let rotated_legacy = "b".repeat(64);
    let password_text = "namespace test password";
    let password = Some(password_text);
    let original_key = SessionKey::derive(password_text, REMOTE_SCOPE).unwrap();
    source
        .save_item(&text_item("legacy", "legacy payload"))
        .unwrap();
    sync_database(
        &store,
        &source,
        &source_paths,
        REMOTE_SCOPE,
        Some(&original_key),
        options(),
    )
    .unwrap();
    let writes = store.puts.lock().unwrap().len();
    assert!(resolve_namespace(&store, &stable, &rotated_legacy, password, None).is_err());
    assert_eq!(store.puts.lock().unwrap().len(), writes);
    assert!(!store.objects.lock().unwrap().contains_key(NAMESPACE_KEY));
    let (binding, _) = resolve_namespace(&store, &stable, REMOTE_SCOPE, password, None).unwrap();
    assert_eq!(binding.scope, REMOTE_SCOPE);
    let (other_binding, key) =
        resolve_namespace(&store, &stable, &rotated_legacy, password, None).unwrap();
    assert_eq!(binding, other_binding);
    target
        .remember_sync_namespace(&stable, &binding.to_bytes())
        .unwrap();
    sync_database(
        &store,
        &target,
        &target_paths,
        &binding.scope,
        key.as_ref(),
        options(),
    )
    .unwrap();
    assert!(target.get_item("legacy").unwrap().is_some());
    let cached = target.sync_namespace_binding(&stable).unwrap().unwrap();
    assert_eq!(Namespace::from_bytes(&cached).unwrap(), binding);
    let writes = store.puts.lock().unwrap().len();
    for wrong in [None, Some("changed password")] {
        assert!(
            resolve_namespace(&store, &stable, &rotated_legacy, wrong, Some(&binding)).is_err()
        );
        assert_eq!(store.puts.lock().unwrap().len(), writes);
    }
    // A lost descriptor can be repaired from the durable local binding after
    // credentials rotate, retaining every old pack and reference scope.
    store.delete(NAMESPACE_KEY).unwrap();
    let (repaired, _) =
        resolve_namespace(&store, &stable, &rotated_legacy, password, Some(&binding)).unwrap();
    assert_eq!(repaired, binding);
    fs::remove_dir_all(source_paths.project).unwrap();
    fs::remove_dir_all(target_paths.project).unwrap();
}

#[test]
fn fresh_namespace_is_credential_independent_and_encryption_mode_is_bound() {
    use clipboard_sync::v1::namespace::{NAMESPACE_KEY, Namespace, resolve_namespace};
    for password in [None, Some("namespace fixture password")] {
        let store = MemoryStore::default();
        let stable = "c".repeat(64);
        let (first, _) = resolve_namespace(&store, &stable, REMOTE_SCOPE, password, None).unwrap();
        let (second, _) =
            resolve_namespace(&store, &stable, &"b".repeat(64), password, None).unwrap();
        assert_eq!(first.scope, stable);
        assert_eq!(first, second);
        let other_mode = if password.is_some() {
            None
        } else {
            Some("new password")
        };
        assert!(resolve_namespace(&store, &stable, REMOTE_SCOPE, other_mode, None).is_err());
        let bytes = store
            .objects
            .lock()
            .unwrap()
            .get(NAMESPACE_KEY)
            .unwrap()
            .clone();
        assert_eq!(Namespace::from_bytes(&bytes).unwrap(), first);
        assert!(Namespace::from_bytes(b"{\"version\":99}").is_err());
        assert!(Namespace::from_bytes(&vec![b' '; 4097]).is_err());
    }
}
