pub mod backup;
pub mod crypto;
pub mod pool;
pub mod resources;
pub mod s3;
pub mod v1;
pub mod webdav;
pub mod wire;

pub use backup::{
    count_unsynced, create_baseline_backup, create_oplog_backup, mark_oplog_synced,
    merge_baseline_archives, purge_oplog, read_baseline_archive_bytes, read_baseline_items,
    read_baseline_with_resources, read_manifest_from_backup, write_baseline_zip, BackupManifest,
    ResourceEntry,
};
pub use pool::{
    absorb_pool_paths, ensure_pool_uploads, load_pool_manifest, mark_pool_references,
    pool_object_path, prepare_pool_refs, PoolStorage,
};
pub use resources::{
    collect_entry_resources, collect_item_resources, materialize_resources,
    rewrite_item_paths_to_local, rewrite_to_local,
};
pub use s3::{
    delete_from_s3, download_from_s3, get_s3_object, list_s3_objects, list_s3_objects_after,
    put_s3_object, test_s3_connection, upload_to_s3, S3DownloadedObject, S3Entry, S3PutCondition,
    S3PutOutcome, S3TestResult,
};
pub use webdav::{
    delete_from_webdav, download_from_webdav, list_webdav_files, test_webdav_connection,
    upload_to_webdav, WebDavEntry, WebDavTestResult,
};
pub use wire::{
    deserialize_baseline_with_resources, deserialize_oplog, deserialize_oplog_with_resources,
    merge_baseline_contents, merge_baselines, serialize_baseline_with_resources, serialize_oplog,
    serialize_oplog_with_resources, OplogResource,
};
