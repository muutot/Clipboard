use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use rusqlite::params;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    domain::{ClipboardItem, ClipboardKind},
    performance::{MemoryMetrics, MemoryMonitor},
    storage::{ClipboardRepository, Database, StorageError, StoragePaths},
};

const INITIAL_RECORDS_PER_DEVICE: u64 = 100_000;
const DAILY_RECORDS_PER_DEVICE: u64 = 200;
const EXPECTED_INITIAL_UNION: u64 = INITIAL_RECORDS_PER_DEVICE * 3;
const EXPECTED_DAILY_UNION: u64 = EXPECTED_INITIAL_UNION + DAILY_RECORDS_PER_DEVICE * 3;
const PAGINATION_SEGMENTS: u64 = 1_001;
const CHECKPOINT_SEQUENCE_WAVE: u64 = 50_000;

#[derive(Clone)]
struct BenchS3Config {
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    password: Option<String>,
}

impl BenchS3Config {
    fn from_environment() -> Self {
        Self {
            endpoint: std::env::var("CLIPBOARD_S3_TEST_ENDPOINT")
                .expect("CLIPBOARD_S3_TEST_ENDPOINT must be set"),
            region: std::env::var("CLIPBOARD_S3_TEST_REGION")
                .unwrap_or_else(|_| "us-east-1".to_string()),
            bucket: std::env::var("CLIPBOARD_S3_TEST_BUCKET")
                .expect("CLIPBOARD_S3_TEST_BUCKET must be set"),
            access_key: std::env::var("CLIPBOARD_S3_TEST_ACCESS_KEY")
                .expect("CLIPBOARD_S3_TEST_ACCESS_KEY must be set"),
            secret_key: std::env::var("CLIPBOARD_S3_TEST_SECRET_KEY")
                .expect("CLIPBOARD_S3_TEST_SECRET_KEY must be set"),
            password: std::env::var("CLIPBOARD_S3_TEST_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty()),
        }
    }
}

struct BenchScope {
    config: BenchS3Config,
    remote_prefix: String,
    remote_scope: String,
    cleaned: bool,
}

impl BenchScope {
    fn new(config: BenchS3Config, purpose: &str) -> Self {
        let remote_prefix = format!(
            "clipboard-sync-benchmark/{}/{}",
            uuid::Uuid::new_v4(),
            purpose
        );
        let endpoint = config.endpoint.trim().trim_end_matches('/');
        let identity = format!(
            "clipboard-sync-v1\n{endpoint}\n{}\n{}\n{}\n{}",
            config.region, config.bucket, remote_prefix, config.access_key
        );
        let remote_scope = hex::encode(Sha256::digest(identity.as_bytes()));
        Self {
            config,
            remote_prefix,
            remote_scope,
            cleaned: false,
        }
    }

    fn store(&self, metrics: Option<S3RequestMetrics>) -> S3ObjectStore {
        let store = S3ObjectStore::new(
            self.config.endpoint.clone(),
            self.config.region.clone(),
            self.config.bucket.clone(),
            self.config.access_key.clone(),
            self.config.secret_key.clone(),
            &self.remote_prefix,
        )
        .unwrap();
        match metrics {
            Some(metrics) => store.with_metrics(metrics),
            None => store,
        }
    }

    fn session_key(&self) -> Option<SessionKey> {
        self.config
            .password
            .as_deref()
            .map(|password| SessionKey::derive(password, &self.remote_scope).unwrap())
    }

    fn inventory(&self) -> RemoteInventory {
        RemoteInventory::read(&self.store(None)).unwrap()
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        let store = self.store(None);
        for object in store.list("", None)? {
            store.delete(&object.key)?;
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for BenchScope {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!(
                "[sync-bench] failed to clean isolated prefix {:?}: {error}",
                self.remote_prefix
            );
        }
    }
}

struct BenchDevice {
    label: String,
    paths: StoragePaths,
    database: Option<Database>,
    repository_metrics: RepositoryMetrics,
}

impl BenchDevice {
    fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        let project = std::env::temp_dir().join(format!(
            "clipboard-sync-scale-{}-{}-{}",
            label,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let paths = StoragePaths::initialize(project).unwrap();
        let database = Database::open(&paths.database).unwrap();
        Self {
            label,
            paths,
            database: Some(database),
            repository_metrics: RepositoryMetrics::default(),
        }
    }

    fn database(&self) -> &Database {
        self.database.as_ref().expect("benchmark database is open")
    }

    fn repository(&self) -> MeasuredRepository<'_> {
        MeasuredRepository {
            database: self.database(),
            metrics: &self.repository_metrics,
        }
    }

    fn engine_paths(&self) -> SyncEnginePaths {
        (&self.paths).into()
    }

    fn cleanup(&mut self) {
        self.database.take();
        let _ = fs::remove_dir_all(&self.paths.project);
    }
}

impl Drop for BenchDevice {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RepositoryTimingSnapshot {
    export_encode_ns: u64,
    checkpoint_apply_ns: u64,
    snapshot_apply_ns: u64,
    segment_apply_ns: u64,
    other_ns: u64,
}

#[derive(Default)]
struct RepositoryMetrics {
    export_encode_ns: AtomicU64,
    checkpoint_apply_ns: AtomicU64,
    snapshot_apply_ns: AtomicU64,
    segment_apply_ns: AtomicU64,
    other_ns: AtomicU64,
}

impl RepositoryMetrics {
    fn reset(&self) {
        for counter in [
            &self.export_encode_ns,
            &self.checkpoint_apply_ns,
            &self.snapshot_apply_ns,
            &self.segment_apply_ns,
            &self.other_ns,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    fn snapshot(&self) -> RepositoryTimingSnapshot {
        RepositoryTimingSnapshot {
            export_encode_ns: self.export_encode_ns.load(Ordering::Relaxed),
            checkpoint_apply_ns: self.checkpoint_apply_ns.load(Ordering::Relaxed),
            snapshot_apply_ns: self.snapshot_apply_ns.load(Ordering::Relaxed),
            segment_apply_ns: self.segment_apply_ns.load(Ordering::Relaxed),
            other_ns: self.other_ns.load(Ordering::Relaxed),
        }
    }

    fn time<T>(
        counter: &AtomicU64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let started = Instant::now();
        let result = action();
        counter.fetch_add(
            started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        result
    }
}

struct MeasuredRepository<'a> {
    database: &'a Database,
    metrics: &'a RepositoryMetrics,
}

impl SyncRepository for MeasuredRepository<'_> {
    fn initialize_sync(&self) -> Result<bool, String> {
        RepositoryMetrics::time(&self.metrics.other_ns, || {
            <Database as SyncRepository>::initialize_sync(self.database)
        })
    }

    fn get_sync_device_id(&self) -> Result<String, String> {
        <Database as SyncRepository>::get_sync_device_id(self.database)
    }

    fn get_or_create_sync_remote_state(
        &self,
        remote_scope: &str,
    ) -> Result<SyncRemoteState, String> {
        <Database as SyncRepository>::get_or_create_sync_remote_state(self.database, remote_scope)
    }

    fn reset_sync_remote_state(&self, remote_scope: &str) -> Result<SyncRemoteState, String> {
        <Database as SyncRepository>::reset_sync_remote_state(self.database, remote_scope)
    }

    fn mark_sync_remote_prepared(&self, remote_scope: &str) -> Result<(), String> {
        <Database as SyncRepository>::mark_sync_remote_prepared(self.database, remote_scope)
    }

    fn get_sync_outbox_batch_for_scope(
        &self,
        remote_scope: &str,
        limit: usize,
    ) -> Result<Option<SyncOutboxBatch>, String> {
        <Database as SyncRepository>::get_sync_outbox_batch_for_scope(
            self.database,
            remote_scope,
            limit,
        )
    }

    fn commit_sync_bootstrap_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        snapshot: &ObjectRef,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String> {
        <Database as SyncRepository>::commit_sync_bootstrap_published(
            self.database,
            remote_scope,
            expected_epoch,
            snapshot,
            through_sequence,
        )
    }

    fn commit_sync_segment_published(
        &self,
        remote_scope: &str,
        expected_epoch: &str,
        last_segment_key: &str,
        through_sequence: u64,
    ) -> Result<SyncRemoteState, String> {
        <Database as SyncRepository>::commit_sync_segment_published(
            self.database,
            remote_scope,
            expected_epoch,
            last_segment_key,
            through_sequence,
        )
    }

    fn get_sync_cursor(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<DeviceCursor>, String> {
        <Database as SyncRepository>::get_sync_cursor(self.database, remote_scope, device_id)
    }

    fn list_sync_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String> {
        <Database as SyncRepository>::list_sync_cursors(self.database, remote_scope)
    }

    fn get_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
    ) -> Result<Option<SyncHeadCache>, String> {
        <Database as SyncRepository>::get_sync_head_cache(self.database, remote_scope, device_id)
    }

    fn record_sync_head_cache(
        &self,
        remote_scope: &str,
        device_id: &str,
        etag: &str,
        stored_size_bytes: u64,
        modified_ms: Option<i64>,
        head: &DeviceHead,
    ) -> Result<(), String> {
        <Database as SyncRepository>::record_sync_head_cache(
            self.database,
            remote_scope,
            device_id,
            etag,
            stored_size_bytes,
            modified_ms,
            head,
        )
    }

    fn get_sync_checkpoint_state(
        &self,
        remote_scope: &str,
    ) -> Result<Option<(u64, String)>, String> {
        <Database as SyncRepository>::get_sync_checkpoint_state(self.database, remote_scope)
    }

    fn get_sync_checkpoint_cursors(&self, remote_scope: &str) -> Result<Vec<DeviceCursor>, String> {
        <Database as SyncRepository>::get_sync_checkpoint_cursors(self.database, remote_scope)
    }

    fn record_sync_checkpoint_published(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
    ) -> Result<(), String> {
        <Database as SyncRepository>::record_sync_checkpoint_published(
            self.database,
            remote_scope,
            generation,
            checkpoint_sha256,
            cursors,
        )
    }

    fn record_sync_resource_refs(
        &self,
        remote_scope: &str,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<(), String> {
        <Database as SyncRepository>::record_sync_resource_refs(
            self.database,
            remote_scope,
            mutations,
            resource_refs,
        )
    }

    fn visit_sync_snapshot_for_scope(
        &self,
        remote_scope: &str,
        batch_size: usize,
        temporary_directory: &Path,
        visit: &mut dyn FnMut(MutationBatch) -> Result<(), String>,
    ) -> Result<SyncSnapshotExport, String> {
        RepositoryMetrics::time(&self.metrics.export_encode_ns, || {
            <Database as SyncRepository>::visit_sync_snapshot_for_scope(
                self.database,
                remote_scope,
                batch_size,
                temporary_directory,
                visit,
            )
        })
    }

    fn apply_sync_checkpoint_batches(
        &self,
        remote_scope: &str,
        generation: u64,
        checkpoint_sha256: &str,
        cursors: &[DeviceCursor],
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String> {
        RepositoryMetrics::time(&self.metrics.checkpoint_apply_ns, || {
            <Database as SyncRepository>::apply_sync_checkpoint_batches(
                self.database,
                remote_scope,
                generation,
                checkpoint_sha256,
                cursors,
                batches,
            )
        })
    }

    fn apply_sync_snapshot_batches(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        snapshot_sha256: &str,
        batches: &mut dyn Iterator<Item = SyncIncomingBatch>,
    ) -> Result<u64, String> {
        RepositoryMetrics::time(&self.metrics.snapshot_apply_ns, || {
            <Database as SyncRepository>::apply_sync_snapshot_batches(
                self.database,
                remote_scope,
                cursor,
                snapshot_sha256,
                batches,
            )
        })
    }

    fn apply_sync_segment_with_resources(
        &self,
        remote_scope: &str,
        cursor: &DeviceCursor,
        mutations: &MutationBatch,
        resource_refs: &SyncResourceReferences,
    ) -> Result<u64, String> {
        RepositoryMetrics::time(&self.metrics.segment_apply_ns, || {
            <Database as SyncRepository>::apply_sync_segment_with_resources(
                self.database,
                remote_scope,
                cursor,
                mutations,
                resource_refs,
            )
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RemoteInventory {
    object_count: u64,
    total_bytes: u64,
    snapshot_count: u64,
    segment_count: u64,
    checkpoint_count: u64,
    largest_snapshot_bytes: u64,
    largest_segment_bytes: u64,
    largest_checkpoint_bytes: u64,
}

impl RemoteInventory {
    fn read(store: &impl ObjectStore) -> Result<Self, String> {
        let mut inventory = Self::default();
        for object in store.list("", None)? {
            inventory.object_count += 1;
            let size = object.size_bytes.unwrap_or_default();
            inventory.total_bytes = inventory.total_bytes.saturating_add(size);
            if object.key.starts_with("v1/snapshots/") {
                inventory.snapshot_count += 1;
                inventory.largest_snapshot_bytes = inventory.largest_snapshot_bytes.max(size);
            } else if object.key.starts_with("v1/segments/") {
                inventory.segment_count += 1;
                inventory.largest_segment_bytes = inventory.largest_segment_bytes.max(size);
            } else if object.key.starts_with("v1/checkpoints/") {
                inventory.checkpoint_count += 1;
                inventory.largest_checkpoint_bytes = inventory.largest_checkpoint_bytes.max(size);
            }
        }
        Ok(inventory)
    }
}

#[derive(Debug, Clone)]
struct DbFileSizes {
    label: String,
    sqlite_bytes: u64,
    wal_bytes: u64,
    shm_bytes: u64,
}

#[derive(Debug, Clone)]
struct RunSample {
    label: String,
    elapsed: Duration,
    engine: SyncEngineResult,
    transport: S3RequestMetricsSnapshot,
    repository: RepositoryTimingSnapshot,
    memory: MemoryMetrics,
}

struct MemorySampler {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    monitor: Arc<MemoryMonitor>,
}

impl MemorySampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let monitor = Arc::new(MemoryMonitor::new());
        let worker_stop = Arc::clone(&stop);
        let worker_monitor = Arc::clone(&monitor);
        let handle = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                worker_monitor.record_snapshot();
                thread::sleep(Duration::from_millis(250));
            }
            worker_monitor.record_snapshot();
        });
        Self {
            stop,
            handle: Some(handle),
            monitor,
        }
    }

    fn finish(mut self) -> MemoryMetrics {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
        self.monitor.snapshot()
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn options(segment_max_entries: usize) -> SyncEngineOptions {
    SyncEngineOptions {
        segment_max_entries,
        resource_limits: ResourceLimits {
            image_bytes: 64 * 1024 * 1024,
            file_bytes: 256 * 1024 * 1024,
            icon_bytes: 4 * 1024 * 1024,
        },
    }
}

fn seed_initial_records(device: &BenchDevice, device_index: u64) {
    device
        .database()
        .with_connection(|connection| {
            connection.execute_batch(
                "PRAGMA synchronous = OFF;
                 PRAGMA temp_store = MEMORY;",
            )?;
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(
                    "INSERT INTO clipboard_items (
                        id, kind, title, text_content, content_hash, source_app,
                        size_bytes, created_at_ms, last_used_at_ms, is_favorite,
                        metadata_json
                     ) VALUES (?1, 'text', ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9)",
                )?;
                let base_timestamp = 1_700_000_000_000i64
                    .saturating_add(i64::try_from(device_index).unwrap() * 10_000_000);
                for record_index in 0..INITIAL_RECORDS_PER_DEVICE {
                    let id = format!("d{device_index}-initial-{record_index:06}");
                    let text = format!(
                        "device {device_index} initial clipboard record {record_index:06} with stable benchmark text"
                    );
                    let content_hash = hex::encode(Sha256::digest(text.as_bytes()));
                    let title = format!("Initial {device_index}/{record_index:06}");
                    let metadata =
                        json!({"benchmark": true, "device": device_index}).to_string();
                    statement.execute(params![
                        id,
                        title,
                        text,
                        content_hash,
                        format!("benchmark-device-{device_index}"),
                        80i64,
                        base_timestamp.saturating_add(i64::try_from(record_index).unwrap()),
                        record_index.is_multiple_of(100),
                        metadata,
                    ])?;
                }
            }
            transaction.execute_batch(
                "DELETE FROM search_outbox;
                 DELETE FROM sqlite_sequence WHERE name = 'search_outbox';",
            )?;
            transaction.commit()?;
            connection.execute_batch(
                "PRAGMA synchronous = NORMAL;
                 VACUUM;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )?;
            Ok(())
        })
        .unwrap();
}

fn incremental_item(device_index: u64, phase: &str, record_index: u64) -> ClipboardItem {
    let text = format!(
        "device {device_index} {phase} clipboard record {record_index:06} with stable benchmark text"
    );
    let timestamp = 1_800_000_000_000i64
        .saturating_add(i64::try_from(device_index).unwrap() * 10_000_000)
        .saturating_add(i64::try_from(record_index).unwrap());
    ClipboardItem {
        id: format!("d{device_index}-{phase}-{record_index:06}"),
        kind: ClipboardKind::Text,
        title: format!("{phase} {device_index}/{record_index:06}"),
        text_content: Some(text.clone()),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: hex::encode(Sha256::digest(text.as_bytes())),
        source_app: Some(format!("benchmark-device-{device_index}")),
        icon_path: None,
        size_bytes: text.len() as u64,
        created_at_ms: timestamp,
        last_used_at_ms: Some(timestamp),
        is_favorite: record_index.is_multiple_of(100),
        metadata_json: Some(
            json!({"benchmark": true, "device": device_index, "phase": phase}).to_string(),
        ),
    }
}

fn add_incremental_records(device: &BenchDevice, device_index: u64, phase: &str, count: u64) {
    for record_index in 0..count {
        device
            .database()
            .save_item(&incremental_item(device_index, phase, record_index))
            .unwrap();
    }
}

fn active_count(database: &Database) -> u64 {
    database
        .with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE deleted = 0",
                [],
                |row| row.get(0),
            )?;
            u64::try_from(count).map_err(|_| StorageError::ValueOutOfRange {
                field: "benchmark active record count",
            })
        })
        .unwrap()
}

fn content_digest(database: &Database) -> (u64, String) {
    database
        .with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, kind, title, text_content, html_content, rtf_content,
                        content_hash, source_app, size_bytes, created_at_ms,
                        is_favorite, metadata_json
                   FROM clipboard_items
                  WHERE deleted = 0
                  ORDER BY id",
            )?;
            let mut rows = statement.query([])?;
            let mut count = 0u64;
            let mut digest = Sha256::new();
            while let Some(row) = rows.next()? {
                for column in 0..12 {
                    let value = row.get_ref(column)?;
                    match value {
                        rusqlite::types::ValueRef::Null => digest.update([0]),
                        rusqlite::types::ValueRef::Integer(value) => {
                            digest.update([1]);
                            digest.update(value.to_le_bytes());
                        }
                        rusqlite::types::ValueRef::Real(value) => {
                            digest.update([2]);
                            digest.update(value.to_le_bytes());
                        }
                        rusqlite::types::ValueRef::Text(value) => {
                            digest.update([3]);
                            digest.update((value.len() as u64).to_le_bytes());
                            digest.update(value);
                        }
                        rusqlite::types::ValueRef::Blob(value) => {
                            digest.update([4]);
                            digest.update((value.len() as u64).to_le_bytes());
                            digest.update(value);
                        }
                    }
                }
                count += 1;
            }
            Ok((count, hex::encode(digest.finalize())))
        })
        .unwrap()
}

fn assert_converged(devices: &[&BenchDevice], expected_count: u64) -> String {
    let digests = devices
        .iter()
        .map(|device| (device.label.clone(), content_digest(device.database())))
        .collect::<Vec<_>>();
    for (label, (count, _)) in &digests {
        assert_eq!(*count, expected_count, "unexpected record count on {label}");
    }
    let expected_digest = digests[0].1 .1.clone();
    for (label, (_, digest)) in &digests[1..] {
        assert_eq!(digest, &expected_digest, "content diverged on {label}");
    }
    expected_digest
}

fn sync_once(
    scope: &BenchScope,
    device: &BenchDevice,
    segment_max_entries: usize,
    label: impl Into<String>,
) -> RunSample {
    let transport_metrics = S3RequestMetrics::default();
    let store = scope.store(Some(transport_metrics.clone()));
    device.repository_metrics.reset();
    let sampler = MemorySampler::start();
    let started = Instant::now();
    let repository = device.repository();
    let paths = device.engine_paths();
    let session_key = scope.session_key();
    let engine = engine::sync_database(
        &store,
        &repository,
        &paths,
        &scope.remote_scope,
        session_key.as_ref(),
        options(segment_max_entries),
    )
    .unwrap();
    RunSample {
        label: label.into(),
        elapsed: started.elapsed(),
        engine,
        transport: transport_metrics.snapshot(),
        repository: device.repository_metrics.snapshot(),
        memory: sampler.finish(),
    }
}

fn db_file_sizes(device: &BenchDevice) -> DbFileSizes {
    DbFileSizes {
        label: device.label.clone(),
        sqlite_bytes: file_size(&device.paths.database),
        wal_bytes: file_size(&PathBuf::from(format!(
            "{}-wal",
            device.paths.database.display()
        ))),
        shm_bytes: file_size(&PathBuf::from(format!(
            "{}-shm",
            device.paths.database.display()
        ))),
    }
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

fn print_sample(sample: &RunSample) {
    eprintln!(
        "[sync-bench] {} elapsed_ms={} uploaded_entries={} downloaded_entries={} applied_entries={} engine_up_bytes={} engine_down_bytes={} http_put={} http_get={} http_head={} http_list={} http_delete={} http_up_bytes={} http_down_bytes={} encode_ms={:.3} snapshot_apply_ms={:.3} checkpoint_apply_ms={:.3} segment_apply_ms={:.3} repository_other_ms={:.3} http_get_ms={:.3} http_put_ms={:.3} http_list_ms={:.3} peak_rss_bytes={}",
        sample.label,
        sample.elapsed.as_millis(),
        sample.engine.uploaded_entries,
        sample.engine.downloaded_entries,
        sample.engine.applied_entries,
        sample.engine.bytes_uploaded,
        sample.engine.bytes_downloaded,
        sample.transport.put_requests,
        sample.transport.get_requests,
        sample.transport.head_requests,
        sample.transport.list_requests,
        sample.transport.delete_requests,
        sample.transport.uploaded_bytes,
        sample.transport.downloaded_bytes,
        ns_to_ms(sample.repository.export_encode_ns),
        ns_to_ms(sample.repository.snapshot_apply_ns),
        ns_to_ms(sample.repository.checkpoint_apply_ns),
        ns_to_ms(sample.repository.segment_apply_ns),
        ns_to_ms(sample.repository.other_ns),
        ns_to_ms(sample.transport.get_elapsed_ns),
        ns_to_ms(sample.transport.put_elapsed_ns),
        ns_to_ms(sample.transport.list_elapsed_ns),
        sample.memory.peak_bytes,
    );
}

fn ns_to_ms(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}

fn print_inventory(label: &str, inventory: RemoteInventory) {
    eprintln!(
        "[sync-bench] {label} objects={} total_bytes={} snapshots={} segments={} checkpoints={} largest_snapshot_bytes={} largest_segment_bytes={} largest_checkpoint_bytes={}",
        inventory.object_count,
        inventory.total_bytes,
        inventory.snapshot_count,
        inventory.segment_count,
        inventory.checkpoint_count,
        inventory.largest_snapshot_bytes,
        inventory.largest_segment_bytes,
        inventory.largest_checkpoint_bytes,
    );
}

fn print_db_sizes(devices: &[&BenchDevice]) {
    for device in devices {
        let sizes = db_file_sizes(device);
        eprintln!(
            "[sync-bench] db={} sqlite_bytes={} wal_bytes={} shm_bytes={}",
            sizes.label, sizes.sqlite_bytes, sizes.wal_bytes, sizes.shm_bytes
        );
    }
}

fn print_checkpoint_state(scope: &BenchScope, devices: &[&BenchDevice], label: &str) {
    let session_key = scope.session_key();
    let store = scope.store(None);
    let remote = store
        .get(CHECKPOINT_HEAD_KEY)
        .unwrap()
        .map(|downloaded| decode_checkpoint_head(&downloaded.bytes, session_key.as_ref()).unwrap());
    match remote {
        Some(head) => eprintln!(
            "[sync-bench] checkpoint_state label={label} remote_generation={} remote_digest={} remote_vector={:?}",
            head.generation, head.checkpoint.sha256, head.vector
        ),
        None => eprintln!(
            "[sync-bench] checkpoint_state label={label} remote_generation=none"
        ),
    }
    for device in devices {
        let local = device
            .database()
            .get_sync_checkpoint_state(&scope.remote_scope)
            .unwrap();
        let cursors = device
            .database()
            .get_sync_checkpoint_cursors(&scope.remote_scope)
            .unwrap();
        eprintln!(
            "[sync-bench] checkpoint_state label={label} device={} local={local:?} cursors={cursors:?}",
            device.label
        );
    }
}

fn sync_outbox_count(database: &Database) -> u64 {
    database.count_sync_outbox().unwrap()
}

fn generate_checkpoint_sequence_wave(device: &BenchDevice) -> Duration {
    let started = Instant::now();
    device
        .database()
        .with_connection(|connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction
                    .prepare_cached("UPDATE clipboard_items SET size_bytes = ?2 WHERE id = ?1")?;
                for sequence in 0..CHECKPOINT_SEQUENCE_WAVE {
                    let size_bytes = if sequence.is_multiple_of(2) {
                        81i64
                    } else {
                        80i64
                    };
                    statement.execute(params!["d1-initial-000000", size_bytes])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .unwrap();
    started.elapsed()
}

fn record_sample(samples: &mut Vec<RunSample>, sample: RunSample) {
    print_sample(&sample);
    samples.push(sample);
}

fn assert_idle_sample(sample: &RunSample) {
    assert_eq!(
        sample.engine.uploaded_entries, 0,
        "idle sync uploaded entries"
    );
    assert_eq!(
        sample.engine.downloaded_entries, 0,
        "idle sync downloaded entries"
    );
    assert_eq!(
        sample.engine.applied_entries, 0,
        "idle sync applied entries"
    );
    assert_eq!(sample.engine.bytes_uploaded, 0, "idle sync uploaded bodies");
    assert_eq!(
        sample.engine.bytes_downloaded, 0,
        "idle sync downloaded object bodies"
    );
    assert_eq!(sample.transport.put_requests, 0, "idle sync issued PUT");
    assert_eq!(sample.transport.get_requests, 0, "idle sync issued GET");
    assert_eq!(sample.transport.head_requests, 0, "idle sync issued HEAD");
    assert_eq!(
        sample.transport.delete_requests, 0,
        "idle sync issued DELETE"
    );
    assert_eq!(
        sample.transport.list_requests, 1,
        "idle sync should perform one heads LIST page"
    );
}

fn prepare_s3(config: &BenchS3Config) {
    clipboard_sync::s3::ensure_test_bucket(
        &config.endpoint,
        &config.region,
        &config.bucket,
        &config.access_key,
        &config.secret_key,
    )
    .unwrap();
}

#[test]
#[ignore = "requires a disposable S3-compatible server and release-mode scale runtime"]
fn s3_three_device_target_scale_benchmark() {
    let config = BenchS3Config::from_environment();
    prepare_s3(&config);
    let mut scope = BenchScope::new(config, "three-device-scale");
    let mut first = BenchDevice::new("first");
    let mut second = BenchDevice::new("second");
    let mut third = BenchDevice::new("third");

    let seed_started = Instant::now();
    seed_initial_records(&first, 1);
    seed_initial_records(&second, 2);
    seed_initial_records(&third, 3);
    eprintln!(
        "[sync-bench] seed_three_databases elapsed_ms={} records_per_device={INITIAL_RECORDS_PER_DEVICE}",
        seed_started.elapsed().as_millis()
    );
    assert_eq!(active_count(first.database()), INITIAL_RECORDS_PER_DEVICE);
    assert_eq!(active_count(second.database()), INITIAL_RECORDS_PER_DEVICE);
    assert_eq!(active_count(third.database()), INITIAL_RECORDS_PER_DEVICE);

    let mut samples = Vec::new();
    for (device, label) in [
        (&first, "bootstrap-first"),
        (&second, "bootstrap-second"),
        (&third, "bootstrap-third"),
        (&first, "initial-pull-first"),
        (&second, "initial-pull-second"),
        (&third, "initial-pull-third"),
    ] {
        record_sample(&mut samples, sync_once(&scope, device, 512, label));
    }

    let initial_digest = assert_converged(&[&first, &second, &third], EXPECTED_INITIAL_UNION);
    for device in [&first, &second, &third] {
        assert_eq!(sync_outbox_count(device.database()), 0);
    }
    eprintln!(
        "[sync-bench] initial_convergence records={EXPECTED_INITIAL_UNION} digest={initial_digest}"
    );
    print_inventory("after_initial_convergence", scope.inventory());

    add_incremental_records(&first, 1, "daily", DAILY_RECORDS_PER_DEVICE);
    add_incremental_records(&second, 2, "daily", DAILY_RECORDS_PER_DEVICE);
    add_incremental_records(&third, 3, "daily", DAILY_RECORDS_PER_DEVICE);
    for (device, label) in [
        (&first, "daily-publish-first"),
        (&second, "daily-publish-second"),
        (&third, "daily-publish-third"),
        (&first, "daily-pull-first"),
        (&second, "daily-pull-second"),
        (&third, "daily-pull-third"),
    ] {
        record_sample(&mut samples, sync_once(&scope, device, 512, label));
    }

    let daily_digest = assert_converged(&[&first, &second, &third], EXPECTED_DAILY_UNION);
    for device in [&first, &second, &third] {
        assert_eq!(sync_outbox_count(device.database()), 0);
    }
    eprintln!(
        "[sync-bench] daily_convergence records={EXPECTED_DAILY_UNION} digest={daily_digest}"
    );

    // One unmeasured pass warms every peer-head cache; the next pass is the
    // steady-state request/traffic assertion.
    for device in [&first, &second, &third] {
        let _ = sync_once(&scope, device, 512, format!("cache-warm-{}", device.label));
    }
    for device in [&first, &second, &third] {
        let sample = sync_once(&scope, device, 512, format!("idle-{}", device.label));
        assert_idle_sample(&sample);
        record_sample(&mut samples, sample);
    }

    let generation_before_waves = first
        .database()
        .get_sync_checkpoint_state(&scope.remote_scope)
        .unwrap()
        .expect("stable three-device scope should already have a checkpoint")
        .0;
    let first_wave = generate_checkpoint_sequence_wave(&first);
    eprintln!(
        "[sync-bench] checkpoint_wave=1 sequence_updates={CHECKPOINT_SEQUENCE_WAVE} final_size_bytes=80 elapsed_ms={}",
        first_wave.as_millis()
    );
    let first_checkpoint = sync_once(&scope, &first, 10_000, "checkpoint-generation-one");
    assert_eq!(
        first_checkpoint.engine.uploaded_entries,
        CHECKPOINT_SEQUENCE_WAVE / 10_000
    );
    let generation_after_first_wave = first
        .database()
        .get_sync_checkpoint_state(&scope.remote_scope)
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(generation_after_first_wave, generation_before_waves + 1);
    let first_checkpoint_inventory = scope.inventory();
    assert_eq!(first_checkpoint_inventory.checkpoint_count, 2);

    let second_wave = generate_checkpoint_sequence_wave(&first);
    eprintln!(
        "[sync-bench] checkpoint_wave=2 sequence_updates={CHECKPOINT_SEQUENCE_WAVE} final_size_bytes=80 elapsed_ms={}",
        second_wave.as_millis()
    );
    let second_checkpoint = sync_once(&scope, &first, 10_000, "checkpoint-generation-two-gc");
    assert_eq!(
        second_checkpoint.engine.uploaded_entries,
        CHECKPOINT_SEQUENCE_WAVE / 10_000
    );
    let generation_after_second_wave = first
        .database()
        .get_sync_checkpoint_state(&scope.remote_scope)
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(generation_after_second_wave, generation_before_waves + 2);
    assert!(
        first_checkpoint.engine.deleted_remote_objects > 0,
        "first threshold checkpoint should reclaim pre-daily history"
    );
    assert!(
        second_checkpoint.engine.deleted_remote_objects > 0,
        "second threshold checkpoint should reclaim the first sequence wave"
    );
    record_sample(&mut samples, first_checkpoint);
    record_sample(&mut samples, second_checkpoint);
    let compacted_inventory = scope.inventory();
    assert_eq!(
        compacted_inventory.segment_count,
        CHECKPOINT_SEQUENCE_WAVE / 10_000,
        "the current checkpoint generation retains only its five uncovered segments"
    );
    assert_eq!(compacted_inventory.snapshot_count, 0);
    assert_eq!(compacted_inventory.checkpoint_count, 2);
    print_inventory("after_checkpoint_gc", compacted_inventory);

    let checkpoint_before_fourth = first
        .database()
        .get_sync_checkpoint_state(&scope.remote_scope)
        .unwrap()
        .unwrap();
    let mut fourth = BenchDevice::new("fourth");
    let fourth_bootstrap = sync_once(&scope, &fourth, 512, "bootstrap-fourth-from-checkpoint");
    assert!(
        fourth_bootstrap.engine.downloaded_entries >= EXPECTED_DAILY_UNION,
        "fourth device did not consume the retained checkpoint"
    );
    assert_eq!(active_count(fourth.database()), EXPECTED_DAILY_UNION);
    assert_eq!(content_digest(fourth.database()).1, daily_digest);
    assert_eq!(fourth_bootstrap.engine.uploaded_entries, 0);
    assert_eq!(fourth_bootstrap.engine.deleted_remote_objects, 0);
    assert_eq!(fourth_bootstrap.transport.put_requests, 2);
    assert!(
        fourth_bootstrap.engine.bytes_uploaded < 64 * 1024,
        "an empty peer rewrote a full checkpoint"
    );
    assert_eq!(
        fourth
            .database()
            .get_sync_checkpoint_state(&scope.remote_scope)
            .unwrap()
            .unwrap(),
        checkpoint_before_fourth
    );
    record_sample(&mut samples, fourth_bootstrap);

    eprintln!("[sync-bench] recorded_samples={}", samples.len());
    print_db_sizes(&[&first, &second, &third, &fourth]);
    print_inventory("after_fourth_bootstrap", scope.inventory());

    fourth.cleanup();
    first.cleanup();
    second.cleanup();
    third.cleanup();
    scope.cleanup().unwrap();
}

#[test]
#[ignore = "requires a disposable S3-compatible server and release-mode pagination runtime"]
fn s3_segment_listing_paginates_beyond_one_thousand_objects() {
    let config = BenchS3Config::from_environment();
    prepare_s3(&config);
    let mut scope = BenchScope::new(config, "segment-pagination");
    let mut source = BenchDevice::new("pagination-source");
    let mut target = BenchDevice::new("pagination-target");

    add_incremental_records(&source, 9, "base", 1);
    print_sample(&sync_once(
        &scope,
        &source,
        1,
        "pagination-bootstrap-source",
    ));
    print_checkpoint_state(&scope, &[&source], "after-source-bootstrap");
    print_sample(&sync_once(
        &scope,
        &target,
        1,
        "pagination-bootstrap-target",
    ));
    print_checkpoint_state(&scope, &[&source, &target], "after-target-bootstrap");

    add_incremental_records(&source, 9, "pagination", PAGINATION_SEGMENTS);
    let publish = sync_once(&scope, &source, 1, "publish-1001-single-entry-segments");
    assert_eq!(publish.engine.uploaded_entries, PAGINATION_SEGMENTS);
    let before_pull = scope.inventory();
    assert_eq!(before_pull.segment_count, PAGINATION_SEGMENTS);

    let pull = sync_once(&scope, &target, 1, "pull-1001-segments-with-pagination");
    assert_eq!(pull.engine.downloaded_entries, PAGINATION_SEGMENTS);
    assert_eq!(pull.engine.applied_entries, PAGINATION_SEGMENTS);
    assert!(
        pull.transport.list_requests >= 3,
        "expected one heads page plus at least two segment pages, got {}",
        pull.transport.list_requests
    );
    assert_eq!(active_count(target.database()), PAGINATION_SEGMENTS + 1);
    assert_eq!(
        content_digest(source.database()),
        content_digest(target.database())
    );

    print_sample(&publish);
    print_sample(&pull);
    print_inventory("pagination_before_cleanup", before_pull);
    print_db_sizes(&[&source, &target]);

    source.cleanup();
    target.cleanup();
    scope.cleanup().unwrap();
}
